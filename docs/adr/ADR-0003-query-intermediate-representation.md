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

```rust
struct TransitionGraph {
    transitions: Vec<Transition>,
    data_fields: Vec<String>,   // DataFieldId → field name
    variant_tags: Vec<String>,  // VariantTagId → tag name
    entrypoints: Vec<(String, TransitionId)>,
    default_entrypoint: TransitionId,
}

type TransitionId = usize;   // position in transitions array (structural)
type DataFieldId = usize;    // index into data_fields
type VariantTagId = usize;   // index into variant_tags
type RefId = usize;          // unique per each named subquery reference (Ref node in the query AST)
```

Each named definition has an entry point. The default entry is the last definition. Multiple entry points share the same transition graph.

#### Transition

```rust
struct Transition {
    matcher: Option<Matcher>,       // None = epsilon (no node consumed)
    pre_anchored: bool,             // must match at current position, no scanning
    post_anchored: bool,            // after match, cursor must be at last sibling
    pre_effects: Vec<Effect>,       // effects before match (consume previous current)
    post_effects: Vec<Effect>,      // effects after match (consume new current)
    ref_marker: Option<RefTransition>,  // call boundary marker
    next: Vec<TransitionId>,        // successors; order = priority (first = greedy)
}

enum RefTransition {
    Enter(RefId),  // push ref_id onto return stack
    Exit(RefId),   // pop from return stack (must match ref_id)
}
```

Thompson construction creates epsilon transitions with optional `Enter`/`Exit` markers. Epsilon elimination propagates these markers to surviving transitions. At runtime, the engine uses markers to filter which `next` transitions are valid based on return stack state. Multiple transitions can share the same `RefId` after epsilon elimination.

#### Matcher

```rust
enum Matcher {
    // Matches named node like `identifier`, `function_declaration`
    Node {
        kind: NodeTypeId,
        field: Option<NodeFieldId>,        // tree-sitter field constraint
        negated_fields: Vec<NodeFieldId>,  // fields that must be absent
    },
    // literal text: "(", "function", ";", etc., resolved to NodeTypeId
    Anonymous {
        kind: NodeTypeId,
        field: Option<NodeFieldId>,        // tree-sitter field constraint
    },
    Wildcard, // matches any node
    Down,     // descend to first child
    Up,       // ascend to parent
}
```

Navigation variants `Down`/`Up` move the cursor without matching. They enable nested patterns like `(function_declaration (identifier) @name)` where we must descend into children.

#### Effects

```rust
enum Effect {
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
```

Note: Match transitions set `current` to the matched node (not an effect).

Effects capture structure only—nodes, arrays, objects. Type annotations (`:: str`, `:: Type`) are separate metadata applied during post-processing when constructing the final output.

### Data Construction

Effects emit to a linear stream during matching. After a successful match, the effect stream is executed to build the output.

#### Builder

```rust
/// Accumulates effects during matching; supports rollback on backtrack
struct Builder {
    effects: Vec<Effect>,
}

impl Builder {
    fn emit(&mut self, effect: Effect) {
        self.effects.push(effect);
    }

    fn watermark(&self) -> usize {  // save point for backtracking
        self.effects.len()
    }

    fn rollback(&mut self, watermark: usize) {  // discard effects after watermark
        self.effects.truncate(watermark);
    }
}
```

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
    Object(HashMap<DataFieldId, Value<'a>>), // completed object
    Variant(VariantTagId, Box<Value<'a>>),   // tagged variant (tag + payload)
}

enum Container<'a> {
    Array(Vec<Value<'a>>),                   // array under construction
    Object(HashMap<DataFieldId, Value<'a>>), // object under construction
    Variant(VariantTagId),                   // variant tag; EndVariant wraps current value
}
```

#### Execution Pipeline

For any given transition, the execution order is strict to ensure data consistency during backtracking:

1. **Enter**: Push `Frame` with current `builder.watermark()`.
2. **Pre-Effects**: Emit `pre_effects` (uses previous `current` value).
3. **Match**: Validate node kind/fields. If fail, rollback to watermark and abort.
4. **Post-Effects**: Emit `post_effects` (uses new `current` value).
5. **Exit**: Pop `Frame` (validate return).

This order ensures correct behavior during epsilon elimination. Pre-effects run before the match overwrites `current`, allowing effects like `PushElement` to be safely merged from preceding epsilon transitions. Post-effects run after, for effects that need the newly matched node.

#### Example

Query:

```
Func = (function_declaration
    name: (identifier) @name
    parameters: (parameters (identifier)* @params :: string))
```

Input: `function foo(a, b) {}`

Effect stream (annotated with pre/post classification):

```
pre:  StartObject
      (match "foo")
post: Field("name")
pre:  StartArray
      (match "a")
post: ToString
post: PushElement
      (match "b")
post: ToString
post: PushElement
post: EndArray
post: Field("params")
post: EndObject
```

Note: In the raw graph, effects live on epsilon transitions between matches. The pre/post classification determines where they land after epsilon elimination. `StartObject` and `StartArray` are pre-effects (setup before matching). `Field`, `PushElement`, `ToString`, and `End*` are post-effects (consume the matched node or finalize containers).

Execution trace:

| Effect          | current     | stack                                    |
| --------------- | ----------- | ---------------------------------------- |
| StartObject     | -           | [{}]                                     |
| (match "foo")   | Node(foo)   | [{}]                                     |
| Field("name")   | -           | [{name: Node(foo)}]                      |
| StartArray      | -           | [{name:...}, []]                         |
| (match "a")     | Node(a)     | [{name:...}, []]                         |
| ToString        | String("a") | [{name:...}, []]                         |
| PushElement     | -           | [{name:...}, [String("a")]]              |
| (match "b")     | Node(b)     | [{name:...}, [String("a")]]              |
| ToString        | String("b") | [{name:...}, [String("a")]]              |
| PushElement     | -           | [{name:...}, [String("a"), String("b")]] |
| EndArray        | [...]       | [{name:...}]                             |
| Field("params") | -           | [{name:..., params:[...]}]               |
| EndObject       | {...}       | []                                       |

Final result:

```json
{
  "name": Node(foo),
  "params": ["a", "b"]
}
```

### Backtracking

Two mechanisms work together (same for both execution modes):

1. **Cursor checkpoint**: `cursor.descendant_index()` returns a `usize` position; `cursor.goto_descendant(pos)` restores it. O(1) save, O(depth) restore, no allocation.

2. **Effect watermark**: `builder.watermark()` before attempting a branch; `builder.rollback(watermark)` on failure.

```rust
// This logic appears in both modes:
// - Proc macro: generated as literal Rust code
// - Dynamic: executed by the interpreter

let cursor_checkpoint = cursor.descendant_index();
let builder_watermark = builder.watermark();

if try_first_branch(cursor, builder) {
    return true;
}

cursor.goto_descendant(cursor_checkpoint);
builder.rollback(builder_watermark);

try_second_branch(cursor, builder)
```

### Quantifiers

Quantifiers compile to epsilon transitions with specific `next` ordering:

**Greedy `*`/`+`**:

```
Entry ─ε→ [try match first, then exit]
          ↓
        Match ─ε→ loop back to Entry
```

**Non-greedy `*?`/`+?`**:

```
Entry ─ε→ [try exit first, then match]
```

Same structure, different `next` order. The first successor has priority.

### Arrays

Array construction uses epsilon transitions with effects:

```
T0: ε + StartArray             next: [T1]       // pre-effect: setup array
T1: ε (branch)                 next: [T2, T4]   // try match or exit
T2: Match(expr)                next: [T3]
T3: ε + PushElement            next: [T1]       // post-effect: consume matched node
T4: ε + EndArray               next: [T5]       // post-effect: finalize array
T5: ε + Field("items")         next: [...]      // post-effect: assign to field
```

After epsilon elimination, `PushElement` from T3 merges into T2 as a post-effect. `StartArray` from T0 merges into T2 as a pre-effect (first iteration only—loop iterations enter from T3, not T0).

Backtracking naturally handles partial arrays: truncating the effect stream removes uncommitted `PushElement` effects.

### Scopes

Nested objects from `{...} @name` use `StartObject`/`EndObject` effects:

```
T0: ε + StartObject            next: [T1]       // pre-effect: setup object
T1: ... (sequence contents)    next: [T2]
T2: ε + EndObject              next: [T3]       // post-effect: finalize object
T3: ε + Field("name")          next: [...]      // post-effect: assign to field
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
```

The resulting `Value::Variant` preserves the tag distinct from the payload, preventing name collisions.

**JSON serialization** depends on payload type:

- **Object payload**: Flatten fields into the tagged object.
  ```json
  { "$tag": "A", "x": 1, "y": 2 }
  ```
- **Array/Primitive payload**: Wrap in a `content` field.
  ```json
  { "$tag": "A", "content": [1, 2, 3] }
  { "$tag": "B", "content": "foo" }
  ```

The `$tag` key avoids collisions with user-defined `@tag` captures.

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

```rust
// Generated code
fn match_expr(cursor: &mut TreeCursor, builder: &mut Builder) -> bool {
    // ... alternation over Num, Binary, Call variants
}

fn match_binary(cursor: &mut TreeCursor, builder: &mut Builder) -> bool {
    // ...
    if !match_expr(cursor, builder) { return false; }  // RefId implicit
    // ...
}
```

**Dynamic**: The interpreter maintains an explicit return stack. On `Enter(ref_id)`:

1. Push frame with `ref_id`, cursor checkpoint, builder watermark
2. Follow `next` into the definition body

On `Exit(ref_id)`:

1. Verify top frame matches `ref_id`
2. Filter `next` to only transitions reachable from the call site (same `ref_id` on their entry path)
3. Pop frame on successful exit

```rust
/// Return stack entry for definition calls
struct Frame {
    ref_id: RefId,                  // which call site we're inside
    cursor_checkpoint: usize,       // cursor position before call
    builder_watermark: usize,       // effect count before call
}

/// Runtime query executor
struct Interpreter<'a> {
    graph: &'a TransitionGraph,
    return_stack: Vec<Frame>,       // call stack for definition references
    cursor: TreeCursor<'a>,         // current position in AST
    builder: Builder,               // effect accumulator
}
```

### Epsilon Elimination (Optimization)

After initial construction, epsilon transitions can be eliminated by computing epsilon closures. The `pre_effects`/`post_effects` split is essential for correctness here.

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

- Effects from incoming epsilon paths → accumulate into `pre_effects`
- Effects from outgoing epsilon paths → accumulate into `post_effects`

This is why both are `Vec<Effect>` rather than `Option<Effect>`.

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

This optimization benefits both modes:

- **Proc macro**: Fewer transitions → less generated code
- **Dynamic**: Fewer graph traversals → faster interpretation

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
- Effect stream with watermarks

Trade-off: More flexible (runtime query construction), but slower than generated code.

## Execution Mode Comparison

| Aspect           | Proc Macro             | Dynamic                 |
| ---------------- | ---------------------- | ----------------------- |
| Query source     | Compile-time literal   | Runtime string          |
| Graph lifetime   | Compile-time only      | Runtime                 |
| Definition calls | Rust function calls    | Explicit return stack   |
| Return stack     | Rust call stack        | `Vec<Frame>`            |
| Backtracking     | Generated `if`/`else`  | Interpreter loop        |
| Performance      | Zero dispatch overhead | Interpretation overhead |
| Type safety      | Compile-time checked   | Runtime types           |
| Use case         | Known queries          | User-provided queries   |

## Consequences

### Positive

- **Shared IR**: One representation serves both execution modes
- **Proc macro zero-overhead**: Generated code is plain Rust with no dispatch
- **Dynamic flexibility**: Queries can be constructed or modified at runtime
- **Unified backtracking**: Same watermark mechanism for cursor and effects in both modes
- **Optimizable**: Epsilon elimination benefits both modes
- **Multiple entry points**: Same graph supports querying any definition

### Negative

- **Two code paths**: Must maintain both codegen and interpreter
- **Proc macro compile cost**: Complex queries generate more code
- **Dynamic runtime cost**: Interpretation overhead vs. generated code
- **Testing burden**: Must verify both modes produce identical results

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

## References

- Thompson, K. (1968). "Programming Techniques: Regular expression search algorithm." Communications of the ACM, 11(6), pp. 419-422. — fragment composition technique adapted here
- Woods, W. A. (1970). "Transition network grammars for natural language analysis." Communications of the ACM, 13(10), pp. 591-606. — recursive transition networks
- Tree-sitter TreeCursor API: `descendant_index()`, `goto_descendant()`
- [ADR-0001: Query Parser](ADR-0001-query-parser.md)
