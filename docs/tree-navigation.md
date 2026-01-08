# Tree Navigation

How the VM navigates tree-sitter syntax trees. This covers API choice, search loop mechanics, and anchor lowering. For execution semantics, see [runtime-engine.md](runtime-engine.md). For instruction encoding, see [06-transitions.md](binary-format/06-transitions.md).

## TreeCursor API

The VM uses `TreeCursor` exclusively, never the `Node` API for traversal.

```rust
struct VM<'t> {
    cursor: TreeCursor<'t>,          // created at tree root, never reset
    ip: StepId,                      // current step index
    frames: Vec<Frame>,              // call stack
    effects: EffectLog<'t>,          // side-effect log
    matched_node: Option<Node<'t>>,  // current match slot
}

struct Checkpoint {
    descendant_index: u32,           // cursor position (4 bytes)
    effect_watermark: usize,         // effect log length
    frame_index: Option<u32>,        // call stack state
    ip: StepId,                      // resume point
}
```

**Critical constraint**: The cursor must be created at the tree root and never call `reset()`. The `descendant_index` is relative to the cursor's root — `reset(node)` would invalidate all checkpoints.

### Why TreeCursor

| Operation             | TreeCursor | Node        |
| --------------------- | ---------- | ----------- |
| `goto_first_child()`  | O(1)       | —           |
| `goto_next_sibling()` | O(1)       | O(siblings) |
| `goto_parent()`       | O(1)       | O(1)        |
| `descendant_index()`  | O(1)       | —           |
| `goto_descendant()`   | O(depth)   | —           |

The `Node` API's `next_sibling()` is O(siblings) — unacceptable for repeated backtracking. TreeCursor provides O(1) sibling traversal and 4-byte checkpoints via `descendant_index`.

- Checkpoint save: O(1)
- Checkpoint restore: O(depth) — cold path only

## Nav Encoding

`Nav` is a single byte encoding movement and skip policy. See [06-transitions.md § 3.1](binary-format/06-transitions.md) for bit layout.

| Nav               | Dump Symbol | Movement                      |
| ----------------- | ----------- | ----------------------------- |
| `Stay`            | (space)     | No movement                   |
| `StayExact`       | `!`         | No movement, exact match only |
| `Down`            | `↓*`        | First child, skip any         |
| `DownSkip`        | `↓~`        | First child, skip trivia only |
| `DownExact`       | `↓.`        | First child, exact            |
| `Next`            | `*`         | Next sibling, skip any        |
| `NextSkip`        | `~`         | Next sibling, skip trivia     |
| `NextExact`       | `.`         | Next sibling, exact           |
| `Up(n)`           | `*↑ⁿ`       | Ascend n levels               |
| `UpSkipTrivia(n)` | `~↑ⁿ`       | Ascend n, last non-trivia     |
| `UpExact(n)`      | `.↑ⁿ`       | Ascend n, last child          |

## Search Loop

Navigation and matching are intertwined. The `Nav` mode determines initial movement and skip policy.

### Algorithm

```
1. MOVE    Execute nav (goto_first_child, goto_next_sibling, etc.)
2. SEARCH  Loop: try match, on fail apply skip policy
3. EFFECTS On success: set matched_node, execute post_effects
```

For `Up*` variants, step 2 becomes: validate exit constraint, ascend n levels.

### Skip Policy

Each mode defines what happens when a match fails:

**Down/Next variants** (search loop):

| Mode        | On Match Fail                               |
| ----------- | ------------------------------------------- |
| `*` (any)   | Advance and retry until exhausted           |
| `~` (skip)  | If current is non-trivia → fail; else retry |
| `.` (exact) | Fail immediately                            |

**Up variants** (exit validation):

| Mode              | Constraint                                    |
| ----------------- | --------------------------------------------- |
| `Up(n)`           | None — just ascend n levels                   |
| `UpSkipTrivia(n)` | Must be at last non-trivia child, then ascend |
| `UpExact(n)`      | Must be at last child, then ascend            |

### Example: `(foo (bar))` vs `(foo (foo) (foo) (bar))`

With `Nav::Down` (skip any):

1. `goto_first_child` → cursor at first `foo`
2. Try match `bar` → fail
3. Skip policy: any → `goto_next_sibling`
4. Try match `bar` → fail
5. `goto_next_sibling` → cursor at `bar`
6. Try match `bar` → success

With `Nav::DownExact`:

1. `goto_first_child` → cursor at first `foo`
2. Try match `bar` → fail
3. Skip policy: exact → fail immediately

## Trivia

**Trivia** = anonymous nodes + language-specific extras (e.g., `comment`).

The `*Skip` modes skip trivia automatically but fail if a non-trivia node must be skipped.

**Skip invariant**: A node is never skipped if its kind matches the target. This ensures `(comment)` explicitly in a query still matches, even though comments are typically trivia.

The trivia list is stored in the bytecode's Trivia section. See [03-symbols.md § 3](binary-format/03-symbols.md).

## Anchor Lowering

The anchor operator (`.`) compiles to `Nav` variants. Mode is determined by the **strictest operand**:

| Precedes `.`                     | Mode  |
| -------------------------------- | ----- |
| Named node `(foo)`, wildcard `_` | Skip  |
| Anonymous node `"foo"`           | Exact |
| Start of children (prefix `.`)   | Skip  |

### Compilation Examples

Using dump format from [07-dump-format.md](binary-format/07-dump-format.md):

**Simple**: `(function (identifier) @name)`

```
  01       (function)                           02
  02  ↓*   (identifier) [Node Set(M0)]          03
  03 *↑¹                                        ◼
```

**First child anchor**: `(function . (identifier))`

```
  01       (function)                           02
  02  ↓~   (identifier)                         03
  03 *↑¹                                        ◼
```

**Last child anchor**: `(function (identifier) .)`

```
  01       (function)                           02
  02  ↓*   (identifier)                         03
  03 ~↑¹                                        ◼
```

**Adjacent siblings**: `(block (a) . (b))`

```
  01       (block)                              02
  02  ↓*   (a)                                  03
  03   ~   (b)                                  04
  04 *↑¹                                        ◼
```

**Strict adjacency**: `(call (identifier) . "(")`

```
  01       (call)                               02
  02  ↓*   (identifier)                         03
  03   .   "("                                  04
  04 *↑¹                                        ◼
```

**Deep nesting**: `(a (b (c (d))))`

```
  01       (a)                                  02
  02  ↓*   (b)                                  03
  03  ↓*   (c)                                  04
  04  ↓*   (d)                                  05
  05 *↑³                                        ◼
```

Multi-level `Up(n)` coalesces ascent when no intermediate anchors exist. Not yet implemented — currently emits individual `Up(1)` steps.

**Mixed anchors**: `(a (b) . (c) .)`

```
  01       (a)                                  02
  02  ↓*   (b)                                  03
  03   ~   (c)                                  04
  04 ~↑¹                                        ◼
```

The `.` before `(c)` → `NextSkip`; the `.` after `(c)` → `UpSkipTrivia`.

**Intermediate anchors**: `(array {(object (pair) .) (number)})`

```
  02       (array)                              03
  03  ↓*   (object)                             04
  04  ↓*   (pair)                               05
  05 ~↑¹                                        06
  06  *    (number)                             07
  07 *↑¹                                        ◼
```

The `.` after `(pair)` produces `~↑¹` (exit object, pair must be last non-trivia). Then `*` finds sibling `(number)`, and `*↑¹` exits array. Cannot combine steps 05+07 into `UpSkipTrivia(2)` because the constraint applies only at the object level.

## Field Handling

Field constraints are checked during the match attempt within the search loop. The `Match` instruction stores `node_field` as an optional constraint:

```rust
pub struct Match {
    pub nav: Nav,
    pub node_type: Option<NonZeroU16>,   // None = wildcard
    pub node_field: Option<NonZeroU16>,  // None = no constraint
    pub neg_fields: Vec<u16>,            // must NOT be present
    // ...
}
```

Inside the search loop:

```rust
// Before node kind check:
if let Some(required) = pattern.node_field {
    if cursor.field_id() != Some(required) {
        // Field mismatch → apply skip policy
        continue;
    }
}
```

**Negated fields** are checked after node kind matches:

```rust
// After node kind matches:
for &fid in pattern.neg_fields {
    if node.child_by_field_id(fid).is_some() {
        // Negated field present → apply skip policy
        continue;
    }
}
```

Both constraints participate in the skip policy — a mismatch triggers retry (for `*`), fail-if-non-trivia (for `~`), or immediate fail (for `.`).

## Call Navigation

`Call` instructions handle navigation for recursive patterns. The caller provides nav and field constraint; the callee's first `Match` checks node type:

```rust
pub struct Call {
    pub nav: Nav,
    pub node_field: Option<NonZeroU16>,
    pub next: StepId,      // return address
    pub target: StepId,    // callee entry
}
```

For `field: (Ref)` patterns, this allows checking field and type on the same node without extra instructions.
