# ADR-0005: Transition Graph Format

- **Status**: Accepted
- **Date**: 2024-12-12
- **Supersedes**: Parts of ADR-0003

## Context

Edge-centric IR: transitions carry all semantics (matching, effects, successors). States are implicit junction points. The result is a recursive transition network—NFA with call/return for definition references.

## Decision

### Types

```rust
type TransitionId = u32;
type NodeTypeId = u16;       // from tree-sitter, do not change
type NodeFieldId = NonZeroU16;  // from tree-sitter, Option uses 0 for None
type RefId = u16;
// StringId, DataFieldId, VariantTagId: see ADR-0004
```

### Slice

Relative range within a segment:

```rust
#[repr(C)]
struct Slice<T> {
    start_index: u32,  // element index into segment array (NOT byte offset)
    len: u16,          // 65k elements per slice is sufficient
    _phantom: PhantomData<T>,
}
// 6 bytes, align 4
```

`start_index` is an **element index**, not a byte offset. This naming distinguishes it from byte offsets like `StringRef.offset` and `CompiledQuery.*_offset`. The distinction matters for typed array access.

### Transition

```rust
#[repr(C, align(64))]
struct Transition {
    // --- 32 bytes metadata ---
    matcher: Matcher,              // 16 (offset 0)
    ref_marker: RefTransition,     // 4  (offset 16)
    successor_count: u32,          // 4  (offset 20)
    effects: Slice<EffectOp>,      // 6  (offset 24, when no effects: start and len are zero)
    nav: Nav,                      // 2  (offset 30, see ADR-0008)

    // --- 32 bytes control flow ---
    successor_data: [u32; 8],      // 32 (offset 32)
}
// 64 bytes, align 64 (cache-line aligned)
```

Navigation is fully determined by `nav`—no runtime dispatch based on previous matcher. See [ADR-0008](ADR-0008-tree-navigation.md) for `Nav` definition and semantics.

Single `ref_marker` slot—sequences like `Enter(A) → Enter(B)` remain as epsilon chains.

### Inline Successors (SSO-style)

Successors use a small-size optimization to avoid indirection for the common case:

| `successor_count` | Layout                                                                              |
| ----------------- | ----------------------------------------------------------------------------------- |
| 0–8               | `successor_data[0..count]` contains `TransitionId` values directly                  |
| > 8               | `successor_data[0]` is index into `successors` segment, `successor_count` is length |

Why 8 slots: Moving `successor_count` into the metadata block frees 32 bytes for `successor_data`, giving 32 / 4 = 8 inline slots.

Coverage:

- Linear sequences: 1 successor
- Simple branches, quantifiers: 2 successors
- Most alternations: 2–8 branches

Only massive alternations (9+ branches) spill to the external buffer.

Cache benefits:

- 64 bytes = L1 cache line on x86/ARM64
- No transition straddles cache lines
- No pointer chase for 99%+ of transitions

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
}
// 16 bytes, align 4
```

`Option<NodeFieldId>` uses 0 for `None` (niche optimization).

Navigation (descend/ascend) is handled by `Nav`, not matchers. Matchers are purely for node matching.

### RefTransition

```rust
#[repr(C, u8)]
enum RefTransition {
    None,
    Enter(RefId),  // push call frame with returns
    Exit(RefId),   // pop frame, use stored returns
}
// 4 bytes, align 2
```

Layout: 1-byte discriminant + 1-byte padding + 2-byte `RefId` payload = 4 bytes. Alignment is 2 (from `RefId: u16`). Fits comfortably in the 64-byte `Transition` struct with room to spare.

Explicit `None` ensures stable binary layout (`Option<Enum>` niche is unspecified).

### Enter/Exit Semantics

**Problem**: A definition can be called from multiple sites. Naively, `Exit.next` would contain all possible return points from all call sites, requiring O(N) filtering at runtime to find which return is valid for the current call.

**Solution**: Store return transitions at `Enter` time (in the call frame), retrieve at `Exit` time. O(1) exit, no filtering.

For `Enter(ref_id)` transitions, the **logical** successor list (accessed via `TransitionView::successors()`) has special structure:

- `successors()[0]`: definition entry point (where to jump)
- `successors()[1..]`: return transitions (stored in call frame)

This structure applies to the view, not raw `successor_data` memory. The SSO optimization (inline vs spilled storage) is orthogonal—the view abstracts it away. An `Enter` with 8+ returns spills to the external segment like any other transition; the interpreter accesses the logical list uniformly.

For `Exit(ref_id)` transitions, successors are **ignored**. Return transitions come from the call frame pushed at `Enter`. See [ADR-0006](ADR-0006-dynamic-query-execution.md) for execution details.

```
Call site:
T1: ε + Enter(Func)  successors=[T10, T2, T3]
                               │    └─────┴─── return transitions (stored in frame)
                               └─────────────── definition entry

Definition:
T10: Match(...) successors=[T11]
T11: ε + Exit(Func) successors=[] (ignored, returns from frame)
```

### EffectOp

```rust
#[repr(C, u16)]
enum EffectOp {
    CaptureNode,                   // store matched node as current value
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

**Graph construction invariant**: `CaptureNode` may only appear in the effects list of a transition where `matcher` is `Node`, `Anonymous`, or `Wildcard`. Placing `CaptureNode` on an `Epsilon` transition is illegal—graph construction must enforce this at build time.

### View Types

```rust
struct TransitionView<'a> {
    query: &'a CompiledQuery,
    raw: &'a Transition,
}

struct MatcherView<'a> {
    query: &'a CompiledQuery,
    raw: &'a Matcher,
}

enum MatcherKind { Epsilon, Node, Anonymous, Wildcard }
```

Views resolve `Slice<T>` to `&[T]`. `TransitionView::successors()` returns `&[TransitionId]`, hiding the inline/spilled distinction—callers see a uniform slice regardless of storage location. Engine code never touches offsets or `successor_data` directly.

### Quantifiers

Examples in this section show graph structure and effects. Navigation (`nav`) is omitted for brevity—see [ADR-0008](ADR-0008-tree-navigation.md) for full transition examples with navigation.

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
T0: ε [StartArray]                    → [T1]
T1: ε (branch)                        → [T2, T4]
T2: Match(identifier) [CaptureNode]   → [T3]
T3: ε [PushElement]                   → [T1]
T4: ε [EndArray]                      → [T5]
T5: ε [Field("params")]               → [...]
```

After:

```
T2': Match(identifier) [StartArray, CaptureNode, PushElement]  → [T2', T4']
T4': ε [EndArray, Field("params")]                             → [...]
```

First iteration gets `StartArray` from T0's path. Loop iterations skip it. Note T4' remains epsilon—effects cannot merge into T2' without breaking semantics.

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

Partial—full elimination impossible due to single `ref_marker` and effect ordering constraints.

**Execution order** (all transitions, including epsilon):

1. Execute `nav` and matcher
2. On success: emit `effects` in order

With explicit `CaptureNode`, effect order is unambiguous. When eliminating epsilon chains, concatenate effect lists in traversal order.

**When epsilon nodes must remain**:

1. **Ref markers**: A transition can hold at most one `Enter`/`Exit`. Sequences like `Enter(A) → Enter(B)` need epsilon.
2. **Branch points**: An epsilon with multiple successors cannot merge into predecessors without duplicating effects.
3. **Effect ordering conflicts**: When incoming and outgoing effects cannot be safely reordered.

Example of safe elimination:

```
Before:
T1: Match(A) [CaptureNode]                 → [T2]
T2: ε [PushElement]                        → [T3]
T3: Match(B) [CaptureNode, Field("b")]     → [...]

After:
T3': Match(B) [PushElement, CaptureNode, Field("b")]  → [...]
```

`PushElement` consumes T1's captured value before T3 overwrites `current`.

## Consequences

**Positive**: No state objects. Cache-line aligned 64-byte transitions eliminate cache straddling. Inline successors remove pointer chasing for common cases. Views hide offset arithmetic and inline/spilled distinction.

**Negative**: Single `ref_marker` leaves some epsilon chains. 33% size increase over minimal layout (acceptable for KB-scale query binaries).

## References

- [ADR-0004: Query IR Binary Format](ADR-0004-query-ir-binary-format.md)
- [ADR-0006: Dynamic Query Execution](ADR-0006-dynamic-query-execution.md)
- [ADR-0007: Type Metadata Format](ADR-0007-type-metadata-format.md)
