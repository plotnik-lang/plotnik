# ADR-0008: Tree Navigation

- **Status**: Accepted
- **Date**: 2025-01-13

## Context

Plotnik's query execution engine ([ADR-0006](ADR-0006-dynamic-query-execution.md)) navigates tree-sitter syntax trees. This ADR covers:

1. Which tree-sitter API to use (TreeCursor vs Node)
2. How `PreNav` encodes navigation and anchor constraints
3. How transitions execute navigation deterministically

Key insight: navigation decisions can be resolved at graph construction time, not runtime. Each transition carries its own `PreNav` instruction—no need to track previous matcher state.

## Decision

### API Choice: TreeCursor with `descendant_index` Checkpoints

```rust
struct InterpreterState<'tree> {
    cursor: TreeCursor<'tree>,  // created once at tree root, never reset
}

struct BacktrackCheckpoint {
    descendant_index: u32,  // 4 bytes, O(1) save
    // ... other state from ADR-0006
}
```

**Critical constraint**: The cursor must be created at the tree root and never call `reset()`. The `descendant_index` is relative to the cursor's root—`reset(node)` invalidates all checkpoints.

### PreNav

Navigation and anchor constraints unified into a single enum:

```rust
#[repr(C)]
struct PreNav {
    kind: PreNavKind,  // 1 byte
    level: u8,         // 1 byte - ascent level count for Up*, ignored otherwise
}
// 2 bytes total

#[repr(u8)]
enum PreNavKind {
    // No movement (first transition only, cursor at root)
    Stay = 0,

    // Sibling traversal (horizontal)
    Next = 1,          // skip any nodes to find match
    NextSkipTrivia = 2, // skip trivia only, fail if non-trivia skipped
    NextExact = 3,     // no skipping, current sibling must match

    // Enter children (descend)
    Down = 4,          // skip any among children
    DownSkipTrivia = 5, // skip trivia only among children
    DownExact = 6,     // first child must match, no skip

    // Exit children (ascend)
    Up = 7,            // ascend `level` levels, no constraint
    UpSkipTrivia = 8,  // validate last non-trivia, ascend `level` levels
    UpExact = 9,       // validate last child, ascend `level` levels
}
```

For non-Up variants, `level` is ignored (conventionally 0). For Up variants, `level >= 1`.

**Design note**: Multi-level `Up(n)` with n>1 is an optimization for the common case (no intermediate anchors). When anchors exist at intermediate nesting levels, decompose into separate `Up*` transitions at each level.

### Trivia

**Trivia** = anonymous nodes + language-specific ignored named nodes (e.g., `comment`).

The ignored kinds list is populated from the `Lang` binding during IR construction and stored in the `ignored_kinds` segment ([ADR-0004](ADR-0004-query-ir-binary-format.md)). Zero offset means no ignored kinds.

**Skip invariant**: A node is never skipped if its kind matches the current transition's matcher target. This ensures `(comment)` explicitly in a query still matches comment nodes, even though comments are typically ignored.

### Execution Semantics

Navigation and matching are intertwined in a search loop. The `PreNav` determines initial movement and skip policy for the loop.

**Stay**: No cursor movement. Used only for the first transition when cursor is already positioned at root. Then attempt match.

**Next variants**: Move to next sibling, enter search loop:

- `Next`: Try match; on fail, advance to next sibling and retry; exhausted → fail
- `NextSkipTrivia`: Try match; on fail, if current node is non-trivia → fail, else advance and retry
- `NextExact`: Try match; on fail → fail (no retry)

**Down variants**: Move to first child, enter search loop:

- `Down`: Try match; on fail, advance to next sibling and retry; exhausted → fail
- `DownSkipTrivia`: Try match; on fail, if current node is non-trivia → fail, else advance and retry
- `DownExact`: Try match; on fail → fail (no retry)

**Up variants**: Validate exit constraint, then ascend N levels (no search loop):

- `Up`: No constraint, ascend
- `UpSkipTrivia`: Fail if non-trivia siblings follow current position, then ascend
- `UpExact`: Fail if any siblings follow current position, then ascend

Example: `(foo (bar))` matching `(foo (foo) (foo) (bar))`:

1. `[Down]` → goto_first_child (cursor at first `foo` child)
2. Try match `bar` → fail
3. Mode is `Down` (skip any) → goto_next_sibling (cursor at second `foo`)
4. Try match `bar` → fail
5. goto_next_sibling (cursor at `bar`)
6. Try match `bar` → success, exit loop

### Skip Mode Symmetry

| Mode       | Entry/Search (Next/Down)                | Exit (Up)                        |
| ---------- | --------------------------------------- | -------------------------------- |
| None       | skip any nodes                          | no constraint on siblings        |
| SkipTrivia | skip trivia, fail if non-trivia skipped | must be at last non-trivia child |
| Exact      | no skip, immediate position             | must be at last child            |

### Anchor Lowering

The anchor operator (`.`) in the query language compiles to `PreNav` variants:

| Query Pattern        | PreNav on Following Transition |
| -------------------- | ------------------------------ |
| `(foo) (bar)`        | `Next`                         |
| `(foo) . (bar)`      | `NextSkipTrivia`               |
| `"x" . (bar)`        | `NextExact`                    |
| `(parent (child))`   | `Down` on child's transition   |
| `(parent . (child))` | `DownSkipTrivia`               |
| `(parent (child) .)` | `UpSkipTrivia` on exit         |
| `(parent "x" .)`     | `UpExact` on exit              |

Mode determined by what **precedes** the anchor:

| Precedes `.`                     | Mode       |
| -------------------------------- | ---------- |
| Named node `(foo)`, wildcard `_` | SkipTrivia |
| String literal `"foo"`           | Exact      |
| Start of children (prefix `.`)   | SkipTrivia |

### Multi-Level Ascent

Closing multiple nesting levels uses `Up` with a level count. For `(a (b (c (d))))`:

```
T3: [Down]       Node(d)   → T4
T4: [Up level=3] Epsilon   → Accept
```

When anchors exist at intermediate levels, decompose. For `(a (b (c) .) .)`:

```
T2: [Down]           Node(c)   → T3
T3: [UpSkipTrivia]   Epsilon   → T4   // c must be last non-trivia in b
T4: [UpSkipTrivia]   Epsilon   → Accept  // b must be last non-trivia in a
```

Cannot combine into `UpSkipTrivia(2)` because constraints apply at each level.

### Execution Flow

```
1. MOVE        pre_nav → initial cursor movement
2. SEARCH      loop: try matcher, on fail check skip policy, advance or fail
3. EFFECTS     on match success: execute effects list (including explicit CaptureNode)
```

For `Up*` variants, step 2 is replaced by: validate exit constraint, ascend N levels.

No post-validation phase. Exit constraints are front-loaded into `Up*` variants.

### Field Handling

**Field constraints** are part of the match attempt within the search loop. A node that doesn't satisfy field constraints is treated as a match failure, triggering the skip policy:

```rust
// Inside search loop, before structural match:
if let Some(required) = pattern.field {
    if cursor.field_id() != Some(required) {
        // Field mismatch = match fail, apply skip policy
        continue;
    }
}
// Then check node kind, negated fields, etc.
```

**Negated fields** are also part of match—checked after field/kind match succeeds:

```rust
// After node kind matches:
for &fid in pattern.negated_fields {
    if node.child_by_field_id(fid).is_some() {
        // Negated field present = match fail, apply skip policy
        continue;
    }
}
// Match succeeds, exit search loop
```

### Examples

**Simple**: `(function (identifier) @name)`

```
T0: [Stay]  Node(function)                        → T1
T1: [Down]  Node(identifier) [CaptureNode]        → T2
T2: [Up]    Epsilon          [Field("name")]      → Accept
```

**Anchored first child**: `(function . (identifier))`

```
T0: [Stay]          Node(function)   → T1
T1: [DownSkipTrivia] Node(identifier) → T2
T2: [Up]            Epsilon          → Accept
```

**Anchored last child**: `(function (identifier) .)`

```
T0: [Stay]         Node(function)   → T1
T1: [Down]         Node(identifier) → T2
T2: [UpSkipTrivia] Epsilon          → Accept
```

**Siblings**: `(block (stmt) (stmt))`

```
T0: [Stay] Node(block) → T1
T1: [Down] Node(stmt)  → T2
T2: [Next] Node(stmt)  → T3
T3: [Up]   Epsilon     → Accept
```

**Adjacent siblings**: `(block (stmt) . (stmt))`

```
T0: [Stay]          Node(block) → T1
T1: [Down]          Node(stmt)  → T2
T2: [NextSkipTrivia] Node(stmt) → T3
T3: [Up]            Epsilon     → Accept
```

**Deep nesting**: `(a (b (c (d))))`

```
T0: [Stay]       Node(a) → T1
T1: [Down]       Node(b) → T2
T2: [Down]       Node(c) → T3
T3: [Down]       Node(d) → T4
T4: [Up level=3] Epsilon → Accept
```

**Mixed anchors**: `(a (b) . (c) .)`

```
T0: [Stay]           Node(a) → T1
T1: [Down]           Node(b) → T2
T2: [NextSkipTrivia] Node(c) → T3   // . before (c): adjacent to b
T3: [UpSkipTrivia]   Epsilon → Accept  // . after (c): c is last non-trivia
```

**Intermediate anchor**: `(foo (foo (bar) .)) (baz)`

```
T0: [Stay]         Node(foo_outer) → T1
T1: [Down]         Node(foo_inner) → T2
T2: [Down]         Node(bar)       → T3
T3: [UpSkipTrivia] Epsilon         → T4   // bar must be last non-trivia in foo_inner
T4: [Up]           Epsilon         → T5   // no constraint on foo_inner in foo_outer
T5: [Next]         Node(baz)       → Accept
```

## Alternatives Considered

### Pure Node API

Rejected: `next_sibling()` is O(siblings), no efficient backtracking.

### Cursor Cloning

Rejected: `TreeCursor::clone()` heap-allocates, O(depth) memory per checkpoint.

### Runtime Navigation Dispatch

Previous design used `(prev_matcher, curr_matcher)` pairs to determine movement at runtime. Rejected:

- Required tracking `prev_matcher` in interpreter state and backtrack checkpoints
- Complex dispatch table
- Navigation decisions can be resolved at compile time

### Separate Post-Anchor Validation

Previous design had `post_anchor` field validated after match. Rejected:

- Extra phase in execution loop
- Exit constraints naturally encode as `Up*` variants
- "Must be last child" is validated before ascending, not after matching

## Complexity Analysis

| Operation               | Cursor       | Node        |
| ----------------------- | ------------ | ----------- |
| `goto_first_child()`    | O(1)         | —           |
| `goto_next_sibling()`   | O(1)         | O(siblings) |
| `goto_parent()`         | O(1)         | O(1)        |
| `field_id()`            | O(field_map) | —           |
| `child_by_field_id(id)` | —            | O(children) |
| `descendant_index()`    | O(1)         | —           |
| `goto_descendant(idx)`  | O(depth)     | —           |

- Checkpoint save: O(1)
- Checkpoint restore: O(depth)—cold path only

## Consequences

**Positive**:

- O(1) sibling traversal
- 4-byte checkpoints
- No `prev_matcher` tracking—navigation fully determined by `PreNav`
- Simpler execution loop: navigate → search → match (no post-validation)
- Anchor constraints resolved at graph construction time

**Negative**:

- Single cursor constraint requires careful state management
- O(depth) restore cost on backtrack
- Intermediate anchors prevent multi-level `Up(n)` optimization

## References

- [ADR-0005: Transition Graph Format](ADR-0005-transition-graph-format.md)
- [ADR-0006: Dynamic Query Execution](ADR-0006-dynamic-query-execution.md)
- `tree-sitter/lib/src/tree_cursor.c`
