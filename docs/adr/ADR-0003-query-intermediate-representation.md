# ADR-0003: Query Intermediate Representation

- **Status**: Accepted
- **Date**: 2025-12-10

## Context

Plotnik needs to execute queries against tree-sitter syntax trees. The query language supports:

- Named node and anonymous node matching
- Field constraints and negated fields
- Named definitions with mutual recursion
- Quantifiers (`*`, `+`, `?`) with greedy/non-greedy variants
- Alternations (tagged and untagged)
- Sequences
- Captures with type annotations
- Anchors for strict positional matching

Plotnik supports two execution modes:

1. **Proc macro (compile-time)**: Query is compiled to specialized Rust functions. Zero runtime interpretation overhead. Used when query is known at compile time.

2. **Dynamic (runtime)**: Query is parsed and executed at runtime via graph interpretation. Used when query is provided by user input or loaded from files.

Both modes share the same intermediate representation (IR). The IR must support efficient execution in both contexts.

The design evolved through several realizations:

1. **Thompson-style fragment composition**: We adapted Thompson's technique for composing pattern fragments—alternation, sequence, quantifiers. However, unlike classic NFAs (which handle only regular languages), our representation supports recursion via a return stack.

2. **Transitions do all the work**: Each transition performs navigation, matching, and effect emission. States are just junction points with no semantics.

3. **Edge-centric model**: Transitions are primary, states are implicit. The IR is a flat array of transitions, each knowing its successors. The result is a _recursive transition network_—like an NFA but with call/return semantics for definition references.

## Decision

We adopt an edge-centric intermediate representation where:

1. **Transitions are primary**: Each transition carries matching logic, effects, and successor links
2. **States are implicit**: No explicit state objects; transitions point directly to successor transitions
3. **Effects are append-only**: Data construction emits effects to a linear stream. Backtracking truncates to a saved watermark—no complex undo logic, just `Vec::truncate`
4. **Shared IR, different executors**: The same `TransitionGraph` serves both proc macro codegen and dynamic interpretation

### Core Data Structures

These structures are used by both execution modes.

#### Transition Graph Container

The graph is immutable after construction. We use a single contiguous allocation sliced into typed segments with proper alignment handling.

```rust
struct TransitionGraph {
    data: Box<[u8]>,
    // segment offsets (aligned for each type)
    successors_offset: u32,
    effects_offset: u32,
    negated_fields_offset: u32,
    data_fields_offset: u32,
    variant_tags_offset: u32,
    entrypoints_offset: u32,
    default_entrypoint: TransitionId,
}

impl TransitionGraph {
    fn new() -> Self;
    fn get(&self, id: TransitionId) -> TransitionView<'_>;
    fn entry(&self, name: &str) -> Option<TransitionView<'_>>;
    fn default_entry(&self) -> TransitionView<'_>;
    fn field_name(&self, id: DataFieldId) -> &str;
    fn tag_name(&self, id: VariantTagId) -> &str;
}
```

##### Memory Arena Design

The single `Box<[u8]>` allocation is divided into typed segments. Each segment is properly aligned for its type, ensuring safe access across all architectures (x86, ARM, WASM).

**Segment Layout**:

| Segment        | Type                | Offset                  | Alignment |
| -------------- | ------------------- | ----------------------- | --------- |
| Transitions    | `[Transition; N]`   | 0 (implicit)            | 4 bytes   |
| Successors     | `[TransitionId; M]` | `successors_offset`     | 4 bytes   |
| Effects        | `[EffectOp; P]`     | `effects_offset`        | 2 bytes   |
| Negated Fields | `[NodeFieldId; Q]`  | `negated_fields_offset` | 2 bytes   |
| Data Fields    | `[u8; R]`           | `data_fields_offset`    | 1 byte    |
| Variant Tags   | `[u8; S]`           | `variant_tags_offset`   | 1 byte    |
| Entrypoints    | `[Entrypoint; T]`   | `entrypoints_offset`    | 4 bytes   |

Transitions always start at offset 0—no explicit offset stored. The arena base address is allocated with 8-byte alignment, satisfying `Transition`'s requirement.

Note: `entry(&str)` performs linear scan — O(n) where n = definition count (typically <20).

##### Memory Layout & Alignment

Casting `&u8` to `&T` when the address is not aligned to `T` causes traps on WASM and faults on strict ARM. The `Box<[u8]>` type only guarantees 1-byte alignment. We enforce alignment explicitly:

**A. Base Allocation Alignment**

The arena must be allocated with alignment equal to the maximum of all segment types:

```rust
const ARENA_ALIGN: usize = 4; // align_of::<Transition>()

let layout = std::alloc::Layout::from_size_align(total_size, ARENA_ALIGN).unwrap();
let ptr = std::alloc::alloc(layout);
let data: Box<[u8]> = unsafe { Box::from_raw(std::slice::from_raw_parts_mut(ptr, total_size)) };
```

**B. Segment Offset Calculation**

Each segment offset is rounded up to its type's alignment:

```rust
fn align_up(offset: usize, align: usize) -> usize {
    (offset + align - 1) & !(align - 1)
}

// Example: if Transitions end at byte 103, Successors (align 4) start at 104
let successors_offset = align_up(transitions_end, align_of::<TransitionId>());
```

**C. Entrypoints Structure**

Entrypoints use fixed-size metadata with indirect string storage:

```rust
#[repr(C)]
struct Entrypoint {
    name_offset: u32,  // index into data_fields segment
    name_len: u32,
    target: TransitionId,
}
// Size: 12 bytes, Align: 4 bytes
```

The `name_offset` points into the `data_fields` segment (u8 array), where alignment is irrelevant. This avoids the alignment hazards of inline variable-length strings.

##### Slice Resolution

`Slice<T>` handles are resolved to actual slices by combining:

1. The segment's base offset (e.g., `effects_offset` for `Slice<EffectOp>`)
2. The slice's `start` field (element index within segment)
3. The slice's `len` field

The `TransitionView` methods (`pre_effects()`, `post_effects()`, `next()`) perform this resolution internally, returning standard `&[T]` slices. Engine code never performs offset arithmetic directly.

**Access Pattern**:

The `TransitionView` and `MatcherView` types provide safe access by:

- Resolving `Slice<T>` handles to actual slices within the appropriate segment
- Converting relative indices to absolute pointers
- Hiding all offset arithmetic from the query engine

This design achieves:

- **Cache efficiency**: All graph data in one contiguous allocation
- **Memory efficiency**: No per-node allocations, minimal overhead
- **Type safety**: Phantom types ensure slices point to correct segments
- **Zero-copy**: Direct references into the arena, no cloning

#### Transition View

`TransitionView` bundles a graph reference with a transition, enabling ergonomic access without explicit slice resolution:

```rust
struct TransitionView<'a> {
    graph: &'a TransitionGraph,
    raw: &'a Transition,
}

impl<'a> TransitionView<'a> {
    fn matcher(&self) -> MatcherView<'a>;
    fn next(&self) -> impl Iterator<Item = TransitionView<'a>>;
    fn pre_effects(&self) -> &[EffectOp];
    fn post_effects(&self) -> &[EffectOp];
    fn is_pre_anchored(&self) -> bool;
    fn is_post_anchored(&self) -> bool;
    fn ref_marker(&self) -> Option<&RefTransition>;
}

struct MatcherView<'a> {
    graph: &'a TransitionGraph,
    raw: &'a Matcher,
}

impl<'a> MatcherView<'a> {
    fn kind(&self) -> MatcherKind;
    fn node_kind(&self) -> Option<NodeTypeId>;
    fn field(&self) -> Option<NodeFieldId>;
    fn negated_fields(&self) -> &[NodeFieldId];  // resolved from Slice
    fn matches(&self, cursor: &TreeCursor) -> bool;
}

enum MatcherKind { Epsilon, Node, Anonymous, Wildcard, Down, Up }
```

**Execution Flow**:

The engine traverses transitions following this pattern:

1. **Pre-effects** execute unconditionally before any matching attempt
2. **Matching** determines whether to proceed:
   - With matcher: Test against current cursor position
   - Without matcher (epsilon): Always proceed
3. **On successful match**: Implicitly capture the node, execute post-effects
4. **Successors** are processed recursively, with appropriate backtracking

The `TransitionView` abstraction hides all segment access complexity. The same logical flow applies to both execution modes—dynamic interpretation emits effects while proc-macro generation produces direct construction code.

#### Slice Handle

A compact, relative reference to a contiguous range within a segment. Replaces `&[T]` to keep structs self-contained.

```rust
#[repr(C)]
struct Slice<T> {
    start: u32,  // Index within segment
    len: u32,    // Number of items
    _phantom: PhantomData<T>,
}

impl<T> Slice<T> {
    const EMPTY: Self = Self { start: 0, len: 0, _phantom: PhantomData };
}
```

Size: 8 bytes. Using `u32` for both fields fills the natural alignment with no padding waste, supporting up to 4B items per slice—well beyond any realistic query.

#### Raw Transition

Internal storage. Engine code uses `TransitionView` instead of accessing this directly.

```rust
#[repr(C)]
struct Transition {
    matcher: Matcher,            // 16 bytes (Epsilon variant for epsilon-transitions)
    pre_anchored: bool,          // 1 byte
    post_anchored: bool,         // 1 byte
    _pad1: [u8; 2],              // 2 bytes padding
    pre_effects: Slice<EffectOp>,  // 8 bytes
    post_effects: Slice<EffectOp>, // 8 bytes
    ref_marker: Option<RefTransition>, // 4 bytes
    next: Slice<TransitionId>,   // 8 bytes
}
// Size: 48 bytes, Align: 4 bytes
```

The `TransitionView` resolves `Slice<T>` by combining the graph's segment offset with the slice's start/len fields.

**Design Note**: The `ref_marker` field is intentionally a single `Option<RefTransition>` rather than a `Slice`. This means a transition can carry at most one Enter or Exit marker. While this prevents full epsilon elimination for nested reference sequences (e.g., `Enter(A) → Enter(B)`), we accept this limitation for simplicity. Such sequences remain as chains of epsilon transitions in the final graph.

```rust
type TransitionId = u32;
type DataFieldId = u16;
type VariantTagId = u16;
type RefId = u16;
```

Each named definition has an entry point. The default entry is the last definition. Multiple entry points share the same transition graph.

#### Matcher

Note: `NodeTypeId` and `NodeFieldId` are defined in `plotnik-core` (tree-sitter uses `u16` and `NonZeroU16` respectively).

```rust
#[repr(C)]
enum Matcher {
    Epsilon,                           // no payload
    Node {
        kind: NodeTypeId,              // 2 bytes
        field: Option<NodeFieldId>,    // 2 bytes
        negated_fields: Slice<NodeFieldId>, // 8 bytes
    },
    Anonymous {
        kind: NodeTypeId,              // 2 bytes
        field: Option<NodeFieldId>,    // 2 bytes
    },
    Wildcard,
    Down,
    Up,
}
// Size: 16 bytes (4-byte discriminant + 12-byte largest variant). Align: 4 bytes
```

Navigation variants `Down`/`Up` move the cursor without matching. They enable nested patterns like `(function_declaration (identifier) @name)` where we must descend into children.

#### Reference Markers

```rust
#[repr(C)]
enum RefTransition {
    Enter(RefId),  // push ref_id onto return stack
    Exit(RefId),   // pop from return stack (must match ref_id)
}
// Size: 4 bytes (1-byte discriminant + 2-byte payload + 1-byte padding), Align: 2 bytes
```

Thompson construction creates epsilon transitions with optional `Enter`/`Exit` markers. Epsilon elimination propagates these markers to surviving transitions. At runtime, the engine uses markers to filter which `next` transitions are valid based on return stack state. Multiple transitions can share the same `RefId` after epsilon elimination.

#### Effect Operations

Instructions stored in the transition graph. These are static, `Copy`, and contain no runtime data.

```rust
#[derive(Clone, Copy)]
#[repr(C)]
enum EffectOp {
    StartArray,              // push new [] onto container stack
    PushElement,             // move current value into top array
    EndArray,                // pop array from stack, becomes current
    StartObject,             // push new {} onto container stack
    EndObject,               // pop object from stack, becomes current
    Field(DataFieldId),      // move current value into field on top object
    StartVariant(VariantTagId), // push variant tag onto container stack
    EndVariant,              // pop variant from stack, wrap current, becomes current
    ToString,                // convert current Node value to String (source text)
}
// Size: 4 bytes (1-byte discriminant + 2-byte payload + 1-byte padding), Align: 2 bytes
```

Note: There is no `CaptureNode` instruction. Node capture is implicit—a successful match automatically emits `RuntimeEffect::CaptureNode` to the effect stream (see below).

Effects capture structure only—arrays, objects, variants. Type annotations (`:: str`, `:: Type`) are separate metadata applied during post-processing.

##### Effect Placement Rules

After epsilon elimination, effects are classified as pre or post based on when they must execute relative to the match:

| Effect         | Placement | Reason                                     |
| -------------- | --------- | ------------------------------------------ |
| `StartArray`   | Pre       | Container must exist before elements added |
| `StartObject`  | Pre       | Container must exist before fields added   |
| `StartVariant` | Pre       | Tag must be set before payload captured    |
| `PushElement`  | Post      | Consumes the just-matched node             |
| `Field`        | Post      | Consumes the just-matched node             |
| `EndArray`     | Post      | Finalizes after last element matched       |
| `EndObject`    | Post      | Finalizes after last field matched         |
| `EndVariant`   | Post      | Wraps payload after it's captured          |
| `ToString`     | Post      | Converts the just-matched node to text     |

Pre-effects from incoming epsilon paths accumulate in order. Post-effects from outgoing epsilon paths accumulate in order. This ordering is deterministic and essential for correct data construction.

### Data Construction (Dynamic Interpreter)

This section describes data construction for the dynamic interpreter. Proc-macro codegen uses direct construction instead (see [Direct Construction](#direct-construction-no-effect-stream)).

The interpreter emits events to a linear stream during matching. After a successful match, the stream is executed to build the output.

#### Runtime Effects

Events emitted to the effect stream during interpretation. Unlike `EffectOp`, these carry runtime data.

```rust
enum RuntimeEffect<'a> {
    Op(EffectOp),            // forwarded instruction from graph
    CaptureNode(Node<'a>),   // emitted implicitly on successful match
}
```

The `CaptureNode` variant is never stored in the graph—it's generated by the interpreter when a match succeeds. This separation keeps the graph static (no lifetimes) while allowing the runtime stream to carry actual node references.

#### Effect Stream

```rust
/// Accumulates runtime effects during matching; supports rollback on backtrack
struct EffectStream<'a> {
    effects: Vec<RuntimeEffect<'a>>,
}
```

The effect stream accumulates effects linearly during matching. It provides:

- **Effect emission**: Appends `EffectOp` instructions and `CaptureNode` events
- **Watermarking**: Records position before attempting branches
- **Rollback**: Truncates to saved position on backtrack

This append-only design makes backtracking trivial—just truncate the vector. No complex undo logic needed.

#### Execution Model

Two separate concepts during effect execution:

1. **Current value** — the last matched node or just-completed container
2. **Container stack** — objects and arrays being built

```rust
struct Executor<'a> {
    current: Option<Value<'a>>,  // last matched node or completed container
    stack: Vec<Container<'a>>,   // objects/arrays being built
}

// Result is the final `current` value after execution completes.
// This allows returning any value type: Object, Array, Node, String, or Variant.

enum Value<'a> {
    Node(Node<'a>),                          // AST node reference
    String(String),                          // Text values (from @capture :: string)
    Array(Vec<Value<'a>>),                   // completed array
    Object(BTreeMap<DataFieldId, Value<'a>>), // completed object (BTreeMap for deterministic iteration)
    Variant(VariantTagId, Box<Value<'a>>),   // tagged variant (tag + payload)
}

enum Container<'a> {
    Array(Vec<Value<'a>>),                   // array under construction
    Object(BTreeMap<DataFieldId, Value<'a>>), // object under construction
    Variant(VariantTagId),                   // variant tag; EndVariant wraps current value
}
```

Effect semantics on `current`:

- `CaptureNode(node)` → sets `current` to `Value::Node(node)`
- `Field(id)` → moves `current` into top object, clears to `None`
- `PushElement` → moves `current` into top array, clears to `None`
- `End*` → pops container from stack into `current`
- `ToString` → replaces `current` Node with its source text as String

#### Execution Pipeline

For any given transition, the execution order is strict to ensure data consistency during backtracking:

1. **Enter**: Push `Frame` with current `effect_stream.watermark()`.
2. **Pre-Effects**: Emit `pre_effects` as `RuntimeEffect::Op(...)`.
3. **Match**: Validate node kind/fields. If fail, rollback to watermark and abort.
4. **Capture**: Emit `RuntimeEffect::CaptureNode(matched_node)` — implicit, not from graph.
5. **Post-Effects**: Emit `post_effects` as `RuntimeEffect::Op(...)`.
6. **Exit**: Pop `Frame` (validate return).

This order ensures correct behavior during epsilon elimination. Pre-effects run before the match overwrites `current`, allowing effects like `PushElement` to be safely merged from preceding epsilon transitions. Post-effects run after, for effects that need the newly matched node.

The key insight: `CaptureNode` is generated by the interpreter on successful match, not stored as an instruction. The graph only contains structural operations (`EffectOp`); the runtime stream (`RuntimeEffect`) adds the actual node data.

#### Example

Query:

```
Func = (function_declaration
    name: (identifier) @name
    parameters: (parameters (identifier)* @params :: string))
```

Input: `function foo(a, b) {}`

Runtime effect stream (showing `EffectOp` from graph vs implicit `CaptureNode`):

```
graph pre:  Op(StartObject)
implicit:   CaptureNode(foo)        ← from successful match
graph post: Op(Field("name"))
graph pre:  Op(StartArray)
implicit:   CaptureNode(a)          ← from successful match
graph post: Op(ToString)
graph post: Op(PushElement)
implicit:   CaptureNode(b)          ← from successful match
graph post: Op(ToString)
graph post: Op(PushElement)
graph post: Op(EndArray)
graph post: Op(Field("params"))
graph post: Op(EndObject)
```

Note: The graph stores only `EffectOp` instructions. `CaptureNode` events are generated by the interpreter on each successful match—they never appear in `Transition.pre_effects` or `Transition.post_effects`.

In the raw graph, `EffectOp`s live on epsilon transitions between matches. The pre/post classification determines where they land after epsilon elimination. `StartObject` and `StartArray` are pre-effects (setup before matching). `Field`, `PushElement`, `ToString`, and `End*` are post-effects (consume the matched node or finalize containers).

Execution trace (key steps, second array element omitted):

| RuntimeEffect       | current    | stack           |
| ------------------- | ---------- | --------------- |
| Op(StartObject)     | -          | [{}]            |
| CaptureNode(foo)    | Node(foo)  | [{}]            |
| Op(Field("name"))   | -          | [{name: Node}]  |
| Op(StartArray)      | -          | [{...}, []]     |
| CaptureNode(a)      | Node(a)    | [{...}, []]     |
| Op(ToString)        | "a"        | [{...}, []]     |
| Op(PushElement)     | -          | [{...}, ["a"]]  |
| _(repeat for "b")_  | ...        | ...             |
| Op(EndArray)        | ["a", "b"] | [{...}]         |
| Op(Field("params")) | -          | [{..., params}] |
| Op(EndObject)       | {...}      | []              |

Final result:

```json
{
  "name": "<Node: foo>",
  "params": ["a", "b"]
}
```

### Backtracking

Two mechanisms work together (same for both execution modes):

1. **Cursor checkpoint**: `cursor.descendant_index()` returns a `usize` position; `cursor.goto_descendant(pos)` restores it. O(1) save, O(depth) restore, no allocation.

2. **Effect watermark**: `effect_stream.watermark()` before attempting a branch; `effect_stream.rollback(watermark)` on failure.

Both execution modes follow the same pattern: save state before attempting a branch; on failure, restore both cursor and effects before trying the next branch. This ensures each alternative starts from the same clean state.

```

### Quantifiers

Quantifiers compile to epsilon transitions with specific `next` ordering:

**Greedy `*`** (zero or more):

```

Entry ─ε→ [try match first, then exit]
↓
Match ─ε→ loop back to Entry

```

**Greedy `+`** (one or more):

```

         ┌──────────────────────────┐
         ↓                          │

Entry ─→ Match ─ε→ Loop ─ε→ [try match first, then exit]

```

The `+` quantifier differs from `*`: it enters directly at `Match`, requiring at least one successful match before the exit path becomes available. After the first match, the `Loop` node behaves like `*` (match-first, exit-second).

**Non-greedy `*?`/`+?`**:

Same structures as above, but with reversed `next` ordering: exit path has priority over match path. For `+?`, after the mandatory first match, the loop prefers exiting over matching more.

### Arrays

Array construction uses epsilon transitions with effects:

```

T0: ε + StartArray next: [T1] // pre-effect: setup array
T1: ε (branch) next: [T2, T4] // try match or exit
T2: Match(expr) next: [T3]
T3: ε + PushElement next: [T1] // post-effect: consume matched node
T4: ε + EndArray next: [T5] // post-effect: finalize array
T5: ε + Field("items") next: [...] // post-effect: assign to field

```

After epsilon elimination, `PushElement` from T3 merges into T2 as a post-effect. `StartArray` from T0 merges into T2 as a pre-effect (first iteration only—loop iterations enter from T3, not T0).

Backtracking naturally handles partial arrays: truncating the effect stream removes uncommitted `PushElement` effects.

### Scopes

Nested objects from `{...} @name` use `StartObject`/`EndObject` effects:

```

T0: ε + StartObject next: [T1] // pre-effect: setup object
T1: ... (sequence contents) next: [T2]
T2: ε + EndObject next: [T3] // post-effect: finalize object
T3: ε + Field("name") next: [...] // post-effect: assign to field

```

`StartObject` is a pre-effect (merges forward). `EndObject` and `Field` are post-effects (merge backward onto preceding match).

### Tagged Alternations

Tagged branches use `StartVariant` to create explicit tagged structures.

```

[ A: (true) ]

```

Effect stream:

```

StartVariant("A")
StartObject
...
EndObject
EndVariant

````

The resulting `Value::Variant` preserves the tag distinct from the payload, preventing name collisions.

**JSON serialization** always uses `$data` wrapper for uniformity:

```json
{ "$tag": "A", "$data": { "x": 1, "y": 2 } }
{ "$tag": "B", "$data": [1, 2, 3] }
{ "$tag": "C", "$data": "foo" }
````

The `$tag` and `$data` keys avoid collisions with user-defined captures. Uniform structure simplifies parsing (always access `.$data`) and eliminates conditional flatten-vs-wrap logic.

**Nested variants** (variant containing variant) serialize naturally:

```json
{ "$tag": "Outer", "$data": { "$tag": "Inner", "$data": 42 } }
```

This mirrors Rust's serde adjacently-tagged representation and remains fully readable for LLMs. No query validation restriction—all payload types are valid.

### Definition References and Recursion

When a pattern references another definition (e.g., `(Expr)` inside `Binary`), the IR uses `RefId` to identify the call site. Each `Ref` node in the query AST gets a unique `RefId`, which is preserved through epsilon elimination.

```
Expr = [ (Num) (Binary) ]
Binary = (binary_expression
    left: (Expr)    // RefId = 0
    right: (Expr))  // RefId = 1
```

The `RefId` is semantic identity—"which reference in the query pattern"—distinct from `TransitionId` which is structural identity—"which slot in the transition array."

**Why RefId matters**: Epsilon elimination creates multiple transitions from a single reference. If a reference has 2 input epsilon paths and 3 output epsilon paths, elimination produces 2×3 = 6 transitions. All share the same `RefId` because they represent the same call site. The return stack uses `RefId` so that:

- Entry can occur via any input path
- Exit can continue via any output path

**Proc macro**: Each definition becomes a Rust function. References become function calls. Rust's call stack serves as the return stack—`RefId` is implicit in the call site.

In proc-macro mode, each definition becomes a Rust function. References become direct function calls, with the Rust call stack serving as the implicit return stack. The `RefId` exists only in the IR—the generated code relies on Rust's natural call/return mechanism.

**Dynamic**: The interpreter maintains an explicit return stack. On `Enter(ref_id)`:

1. Push frame with `ref_id`, cursor checkpoint, effect stream watermark
2. Follow `next` into the definition body

On `Exit(ref_id)`:

1. Verify top frame matches `ref_id` (invariant: mismatched ref_id indicates IR bug)
2. Pop frame
3. Continue to `next` successors unconditionally

**Entry filtering mechanism**: After epsilon elimination, multiple `Exit` transitions with different `RefId`s may be reachable from the same point (merged from different call sites). The interpreter only takes an `Exit(ref_id)` transition if `ref_id` matches the current stack top. This ensures returns go to the correct call site.

After taking an `Exit` and popping the frame, successors are followed unconditionally—they represent the continuation after the call. If a successor has an `Enter` marker, that's a _new_ call (e.g., `(A) (B)` where returning from A continues to calling B), not a return path.

```rust
/// Return stack entry for definition calls
struct Frame {
    ref_id: RefId,                  // which call site we're inside
    cursor_checkpoint: usize,       // cursor position before call
    effect_stream_watermark: usize, // effect count before call
}

/// Runtime query executor
struct Interpreter<'a> {
    graph: &'a TransitionGraph,
    return_stack: Vec<Frame>,       // call stack for definition references
    cursor: TreeCursor<'a>,         // current position in AST
    effect_stream: EffectStream<'a>, // effect accumulator
}
```

### Epsilon Elimination (Optimization)

After initial construction, epsilon transitions can be **partially** eliminated by computing epsilon closures. Full elimination is not always possible due to the single `ref_marker` limitation—sequences like `Enter(A) → Enter(B)` cannot be merged into one transition. The `pre_effects`/`post_effects` split is essential for correctness here.

**Why the split matters**: A match transition overwrites `current` with the matched node. Effects from _preceding_ epsilon transitions (like `PushElement`) need the _previous_ `current` value. Without the split, merging them into a single post-match list would use the wrong value.

```
Before (raw graph):
T1: Match(A)                    next: [T2]      // current = A
T2: ε + PushElement             next: [T3]      // pushes A (correct)
T3: Match(B)                    next: [...]     // current = B

After elimination (with split):
T3': pre: [PushElement], Match(B), post: []     // PushElement runs before Match(B), pushes A ✓

Wrong (without split, effects merged as post):
T3': Match(B) + [PushElement]                   // PushElement runs after Match(B), pushes B ✗
```

**Accumulation rules**:

- `EffectOp`s from incoming epsilon paths → accumulate into `pre_effects`
- `EffectOp`s from outgoing epsilon paths → accumulate into `post_effects`

This is why both are `Slice<EffectOp>` rather than `Option<EffectOp>`.

**Reference expansion**: For definition references, epsilon elimination propagates `Enter`/`Exit` markers to surviving transitions:

```
Before:
T0: ε                       next: [T1]
T1: ε + Enter(0)            next: [T2]   // enter Expr
T2: ... (Expr body) ...     next: [T3]
T3: ε + Exit(0)             next: [T4]   // exit Expr
T4: ε                       next: [T5]

After:
T0': Match(...) + Enter(0)  next: [T2']  // marker propagated
T3': Match(...) + Exit(0)   next: [T5']  // marker propagated
```

All expanded entry transitions share the same `RefId`. All expanded exit transitions share the same `RefId`. The engine filters valid continuations at runtime based on stack state—no explicit continuation storage needed.

**Limitation**: Complete epsilon elimination is impossible when reference markers chain (e.g., nested calls). The single `ref_marker` slot prevents merging `Enter(A) → Enter(B)` sequences. These remain as epsilon transition chains in the final graph.

This optimization benefits both modes:

- **Proc macro**: Fewer transitions → less generated code (where elimination is possible)
- **Dynamic**: Fewer graph traversals → faster interpretation (but must handle remaining epsilons)

### Proc Macro Code Generation

When used as a proc macro, the transition graph is a compile-time artifact:

1. Parses query source at compile time
2. Builds transition graph (Thompson-style construction)
3. Optionally eliminates epsilons
4. Generates Rust functions for each definition

Generated code uses:

- `if`/`else` chains for alternations
- `while` loops for quantifiers
- Direct function calls for definition references
- `TreeCursor` navigation methods
- `descendant_index()`/`goto_descendant()` for backtracking

At runtime, there is no graph—just plain Rust code.

#### Direct Construction (No Effect Stream)

Unlike the dynamic interpreter, proc-macro generated code constructs output values directly—no intermediate effect stream. Output structs are built in a single pass as matching proceeds.

Backtracking in direct construction means dropping partially-built values and re-allocating. This is acceptable because modern allocators maintain thread-local free lists, making the alloc→drop→alloc pattern for small objects essentially O(1).

### Dynamic Execution

When used dynamically, the transition graph is interpreted at runtime:

1. Parses query source at runtime
2. Builds transition graph
3. Optionally eliminates epsilons (can be skipped for faster startup)
4. Interpreter walks the graph, executing transitions

The interpreter maintains:

- Current transition pointer
- Explicit return stack for definition calls
- Cursor position
- `RuntimeEffect` stream with watermarks

Unlike proc-macro codegen, the dynamic interpreter uses the `RuntimeEffect` stream approach. This is necessary because:

- We don't know the output structure at compile time
- `RuntimeEffect` stream provides a uniform way to build any output shape
- Backtracking via `truncate()` is simple and correct

Trade-off: More flexible (runtime query construction), but slower than generated code due to interpretation overhead and the extra effect execution pass.

## Execution Mode Comparison

| Aspect            | Proc Macro                 | Dynamic                       |
| ----------------- | -------------------------- | ----------------------------- |
| Query source      | Compile-time literal       | Runtime string                |
| Graph lifetime    | Compile-time only          | Runtime                       |
| Data construction | Direct (no effect stream)  | `RuntimeEffect` stream + exec |
| Definition calls  | Rust function calls        | Explicit return stack         |
| Return stack      | Rust call stack            | `Vec<Frame>`                  |
| Backtracking      | Drop + re-alloc            | `truncate()` effects          |
| Performance       | Zero dispatch, single pass | Interpretation + 2 pass       |
| Type safety       | Compile-time checked       | Runtime types                 |
| Use case          | Known queries              | User-provided queries         |

## Consequences

### Positive

- **Shared IR**: One representation serves both execution modes
- **Proc macro zero-overhead**: Generated code is plain Rust with no dispatch
- **Pre-allocated graph**: Single contiguous allocation
- **Dynamic flexibility**: Queries can be constructed or modified at runtime
- **Optimizable**: Epsilon elimination benefits both modes
- **Multiple entry points**: Same graph supports querying any definition
- **Clean separation**: `EffectOp` (static instructions) vs `RuntimeEffect` (dynamic events) eliminates lifetime issues

### Negative

- **Two code paths**: Must maintain both codegen and interpreter
- **Different data construction**: Proc macro uses direct construction, dynamic uses `RuntimeEffect` stream
- **Proc macro compile cost**: Complex queries generate more code
- **Dynamic runtime cost**: Interpretation overhead + effect execution pass
- **Testing burden**: Must verify both modes produce identical results

### Runtime Safety

Both execution modes require fuel mechanisms to prevent runaway execution:

- **runtime_fuel**: Decremented on each transition, prevents infinite loops
- **recursion_fuel**: Decremented on each `Enter` marker, prevents stack overflow

These mechanisms deserve their own ADR (fuel budget design, configurable limits, error reporting on exhaustion). The IR itself carries no fuel-related data—fuel checking is purely an interpreter/codegen concern.

**Note**: Static loop detection (e.g., direct recursion like `A = (A)` or mutual recursion like `A = (B)`, `B = (A)`) is handled at the query parser level before IR construction. The IR assumes well-formed input without infinite loops in the pattern structure itself.

### WASM Compatibility

The IR design is WASM-compatible:

- **Single arena allocation**: No fragmentation concerns in linear memory. Note: WASM linear memory grows in 64KB pages; the arena coexists with other allocations (e.g., tree-sitter's memory) but this is standard for any WASM allocation.
- **Explicit alignment**: Arena allocated with `std::alloc::Layout`, segment offsets computed with `align_up()`. Prevents misaligned access traps on WASM and strict ARM.
- **`u32` offsets**: All segment offsets are `u32`, matching WASM32's pointer size. 4GB arena limit is sufficient for any query.
- **`BTreeMap` for objects**: Deterministic iteration order ensures reproducible output across platforms.
- **Fixed-size Entrypoints**: The `Entrypoint` struct (12 bytes, align 4) avoids variable-length inline strings that would cause alignment hazards.
- **No platform-specific primitives**: All types are portable (`u16`, `u32`, `Box<[u8]>`).

#### Serialization Format

For wire transfer between machines with different architectures, the arena uses a portable binary format:

- **Byte order**: Little-endian for all multi-byte integers (`u16`, `u32`). This matches WASM's native byte order and x86/ARM in little-endian mode.
- **String encoding**: UTF-8 for all string data (definition names, field names, variant tags).
- **Format**: The serialized form is a header followed by the raw arena bytes:

```
Header (16 bytes):
  magic: [u8; 4]           // "PLNK"
  version: u32             // format version (little-endian)
  arena_len: u32           // byte length of arena data
  segment_count: u32       // number of segment offset entries

Segment Offsets (segment_count × 4 bytes):
  [u32; segment_count]     // successors_offset, effects_offset, ... (little-endian)

Arena Data (arena_len bytes):
  [u8; arena_len]          // raw arena, requires fixup on big-endian hosts
```

**Loading**: On little-endian hosts (WASM, x86, ARM LE), the arena can be used directly after verifying alignment. On big-endian hosts, multi-byte values within each segment must be byte-swapped according to their type's size.

This format prioritizes simplicity over zero-copy loading on all platforms. The typical query graph is small (<100KB), so the byte-swap cost on big-endian hosts is negligible.

### Considered Alternatives

1. **Proc macro only**
   - Rejected: Need runtime query support for tooling and user-defined queries

2. **Dynamic only**
   - Rejected: Unacceptable performance overhead for known queries

3. **Separate IRs for each mode**
   - Rejected: Duplication; harder to ensure semantic equivalence

4. **State-centric graph representation**
   - Rejected: States carry no semantic weight; edge-centric is simpler

5. **Vectorized Reference Markers (`Vec<RefTransition>`)**
   - Rejected: Optimized for alias chains (e.g. `A = B`, `B = C`) to allow full epsilon elimination. However, this bloats the `Transition` struct for all other cases. Standard epsilon elimination is sufficient; traversing a few remaining epsilon transitions for aliases is cheaper than increasing memory pressure on the whole graph.

6. **Platform-native byte order**
   - Rejected: Would require architecture detection and conditional byte-swapping on both ends. Little-endian-only is simpler and covers >99% of deployment targets.

## References

- Bazaco, D. (2022). "Building a Regex Engine" blog series. https://www.abstractsyntaxseed.com/blog/regex-engine/introduction — NFA construction and modern regex features
- Tree-sitter TreeCursor API: `descendant_index()`, `goto_descendant()`
- [ADR-0001: Query Parser](ADR-0001-query-parser.md)
