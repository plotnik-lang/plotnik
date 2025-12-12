# ADR-0005: Transition Graph Format

- **Status**: Accepted
- **Date**: 2025-12-12
- **Supersedes**: Parts of [ADR-0003](ADR-0003-query-intermediate-representation.md)

## Context

Edge-centric IR: transitions carry all semantics (matching, effects, successors). States are implicit junction points. The result is a recursive transition network—NFA with call/return for definition references.

## Decision

### Types

```rust
type TransitionId = u32;
type NodeTypeId = u16;       // from tree-sitter, do not change
type NodeFieldId = NonZeroU16;  // from tree-sitter, Option uses 0 for None
type DataFieldId = u16;
type VariantTagId = u16;
type RefId = u16;
```

### Slice

Relative range within a segment:

```rust
#[repr(C)]
struct Slice<T> {
    start: u32,
    len: u32,
    _phantom: PhantomData<T>,
}
```

### Transition

```rust
#[repr(C)]
struct Transition {
    matcher: Matcher,              // 16 bytes
    pre_anchored: bool,            // 1
    post_anchored: bool,           // 1
    _pad1: [u8; 2],                // 2
    pre_effects: Slice<EffectOp>,  // 8
    post_effects: Slice<EffectOp>, // 8
    ref_marker: RefTransition,     // 4
    next: Slice<TransitionId>,     // 8
}
// 48 bytes, align 4
```

Single `ref_marker` slot—sequences like `Enter(A) → Enter(B)` remain as epsilon chains.

### Matcher

```rust
#[repr(C, u32)]
enum Matcher {
    Epsilon,
    Node {
        kind: NodeTypeId,                   // 2
        field: Option<NodeFieldId>,         // 2
        negated_fields: Slice<NodeFieldId>, // 8
    },
    Anonymous {
        kind: NodeTypeId,                   // 2
        field: Option<NodeFieldId>,         // 2
        negated_fields: Slice<NodeFieldId>, // 8
    },
    Wildcard,
    Down,  // cursor to first child
    Up,    // cursor to parent
}
// 16 bytes, align 4
```

`Option<NodeFieldId>` uses 0 for `None` (niche optimization).

### RefTransition

```rust
#[repr(C, u8)]
enum RefTransition {
    None,
    Enter(RefId),  // push return stack
    Exit(RefId),   // pop, must match
}
// 4 bytes, align 2
```

Explicit `None` ensures stable binary layout (`Option<Enum>` niche is unspecified).

### EffectOp

```rust
#[repr(C, u16)]
enum EffectOp {
    StartArray,
    PushElement,
    EndArray,
    StartObject,
    EndObject,
    Field(DataFieldId),
    StartVariant(VariantTagId),
    EndVariant,
    ToString,
}
// 4 bytes, align 2
```

No `CaptureNode`—implicit on successful match.

### Effect Placement

| Effect         | Placement | Why                        |
| -------------- | --------- | -------------------------- |
| `StartArray`   | Pre       | Container before elements  |
| `StartObject`  | Pre       | Container before fields    |
| `StartVariant` | Pre       | Tag before payload         |
| `PushElement`  | Post      | Consumes matched node      |
| `Field`        | Post      | Consumes matched node      |
| `End*`         | Post      | Finalizes after last match |
| `ToString`     | Post      | Converts matched node      |

### View Types

```rust
struct TransitionView<'a> {
    query_ir: &'a QueryIR,
    raw: &'a Transition,
}

struct MatcherView<'a> {
    query_ir: &'a QueryIR,
    raw: &'a Matcher,
}

enum MatcherKind { Epsilon, Node, Anonymous, Wildcard, Down, Up }
```

Views resolve `Slice<T>` to `&[T]`. Engine code never touches offsets directly.

### Quantifiers

**Greedy `*`**:

```
         ┌─────────────────┐
         ↓                 │
Entry ─ε→ Branch ─ε→ Match ─┘
           │
           └─ε→ Exit

Branch.next = [match, exit]
```

**Greedy `+`**:

```
         ┌─────────────────┐
         ↓                 │
Entry ─→ Match ─ε→ Branch ─┘
                     │
                     └─ε→ Exit

Branch.next = [match, exit]
```

**Non-greedy `*?`/`+?`**: Same, but `Branch.next = [exit, match]`.

### Example: Array

Query: `(parameters (identifier)* @params)`

Before elimination:

```
T0: ε + StartArray       → [T1]
T1: ε (branch)           → [T2, T4]
T2: Match(identifier)    → [T3]
T3: ε + PushElement      → [T1]
T4: ε + EndArray         → [T5]
T5: ε + Field("params")  → [...]
```

After:

```
T2': pre:[StartArray] Match(identifier) post:[PushElement]  → [T2', T4']
T4': post:[EndArray, Field("params")]                       → [...]
```

First iteration gets `StartArray` from T0's path. Loop iterations skip it.

### Example: Object

Query: `{ (identifier) @name (number) @value } @pair`

```
T0: ε + StartObject       → [T1]
T1: Match(identifier)     → [T2]
T2: ε + Field("name")     → [T3]
T3: Match(number)         → [T4]
T4: ε + Field("value")    → [T5]
T5: ε + EndObject         → [T6]
T6: ε + Field("pair")     → [...]
```

### Example: Tagged Alternation

Query: `[ A: (true) @val  B: (false) @val ]`

```
T0: ε (branch)                        → [T1, T4]
T1: ε + StartVariant("A")             → [T2]
T2: Match(true)                       → [T3]
T3: ε + Field("val") + EndVariant     → [T7]
T4: ε + StartVariant("B")             → [T5]
T5: Match(false)                      → [T6]
T6: ε + Field("val") + EndVariant     → [T7]
```

### Epsilon Elimination

Partial—full elimination impossible due to single `ref_marker`.

Why pre/post split matters:

```
Before:
T1: Match(A)        → [T2]      // current = A
T2: ε + PushElement → [T3]      // push A ✓
T3: Match(B)        → [...]     // current = B

After (correct):
T3': pre:[PushElement] Match(B)     // push A, then match B ✓

Wrong (no split):
T3': Match(B) post:[PushElement]    // match B, push B ✗
```

Incoming epsilon effects → `pre_effects`. Outgoing → `post_effects`.

## Consequences

**Positive**: No state objects. Compact 48-byte transitions. Views hide offset arithmetic.

**Negative**: Single `ref_marker` leaves some epsilon chains. Large queries may pressure cache.

## References

- [ADR-0004: Query IR Binary Format](ADR-0004-query-ir-binary-format.md)
- [ADR-0006: Dynamic Query Execution](ADR-0006-dynamic-query-execution.md)
