# ADR-0006: Dynamic Query Execution

- **Status**: Accepted
- **Date**: 2025-12-12
- **Supersedes**: Parts of [ADR-0003](ADR-0003-query-intermediate-representation.md)

## Context

Runtime interpretation of the transition graph ([ADR-0005](ADR-0005-transition-graph-format.md)). Proc-macro compilation is a future ADR.

## Decision

### Execution Order

For each transition:

1. Emit `pre_effects`
2. Match (epsilon always succeeds)
3. On success: emit `CaptureNode`, emit `post_effects`
4. Process `next` with backtracking

### Effect Stream

```rust
enum RuntimeEffect<'a> {
    Op(EffectOp),
    CaptureNode(Node<'a>),  // implicit on match, never in graph
}

struct EffectStream<'a> {
    effects: Vec<RuntimeEffect<'a>>,
}
```

Append-only. Backtrack via `truncate(watermark)`.

### Executor

```rust
struct Executor<'a> {
    current: Option<Value<'a>>,
    stack: Vec<Container<'a>>,
}

enum Value<'a> {
    Node(Node<'a>),
    String(String),
    Array(Vec<Value<'a>>),
    Object(BTreeMap<DataFieldId, Value<'a>>),
    Variant(VariantTagId, Box<Value<'a>>),
}

enum Container<'a> {
    Array(Vec<Value<'a>>),
    Object(BTreeMap<DataFieldId, Value<'a>>),
    Variant(VariantTagId),
}
```

| Effect              | Action                               |
| ------------------- | ------------------------------------ |
| `CaptureNode(n)`    | `current = Node(n)`                  |
| `StartArray`        | push `Array([])` onto stack          |
| `PushElement`       | move `current` into top array        |
| `EndArray`          | pop array into `current`             |
| `StartObject`       | push `Object({})` onto stack         |
| `Field(id)`         | move `current` into top object field |
| `EndObject`         | pop object into `current`            |
| `StartVariant(tag)` | push `Variant(tag)` onto stack       |
| `EndVariant`        | pop, wrap `current`, set as current  |
| `ToString`          | replace `current` Node with text     |

Invalid state = IR bug → panic.

### Backtracking

Two checkpoints, saved together:

- `cursor.descendant_index()` → restore via `goto_descendant(pos)`
- `effect_stream.len()` → restore via `truncate(watermark)`

### Recursion

```rust
struct Frame {
    ref_id: RefId,
    cursor_checkpoint: usize,
    effect_watermark: usize,
}

struct Interpreter<'a> {
    query_ir: &'a QueryIR,
    stack: Vec<Frame>,
    cursor: TreeCursor<'a>,
    effects: EffectStream<'a>,
}
```

`Enter(ref_id)`: push frame, follow `next` into definition.

`Exit(ref_id)`: verify match, pop frame, continue unconditionally.

Entry filtering: only take `Exit(ref_id)` if it matches stack top.

### Example

Query:

```
Func = (function_declaration
    name: (identifier) @name
    parameters: (parameters (identifier)* @params :: string))
```

Input: `function foo(a, b) {}`

**Phase 1: Match → Effect Stream**

```
pre:  StartObject
match function_declaration  → CaptureNode(func)
match identifier "foo"      → CaptureNode(foo)
post: Field("name")
pre:  StartArray
match identifier "a"        → CaptureNode(a), ToString, PushElement
match identifier "b"        → CaptureNode(b), ToString, PushElement
post: EndArray, Field("params"), EndObject
```

**Phase 2: Execute → Value**

| Effect           | current   | stack            |
| ---------------- | --------- | ---------------- |
| StartObject      | —         | [{}]             |
| CaptureNode(foo) | Node(foo) | [{}]             |
| Field("name")    | —         | [{name:Node}]    |
| StartArray       | —         | [{…}, []]        |
| CaptureNode(a)   | Node(a)   | [{…}, []]        |
| ToString         | "a"       | [{…}, []]        |
| PushElement      | —         | [{…}, ["a"]]     |
| CaptureNode(b)   | Node(b)   | [{…}, ["a"]]     |
| ToString         | "b"       | [{…}, ["a"]]     |
| PushElement      | —         | [{…}, ["a","b"]] |
| EndArray         | ["a","b"] | [{…}]            |
| Field("params")  | —         | [{…,params}]     |
| EndObject        | {…}       | []               |

Result: `{ name: <Node>, params: ["a", "b"] }`

### Variant Serialization

```json
{ "$tag": "A", "$data": { "x": 1 } }
{ "$tag": "B", "$data": [1, 2, 3] }
```

Uniform structure. `$tag`/`$data` avoid capture collisions.

### Fuel

- `transition_fuel`: decremented per transition
- `recursion_fuel`: decremented per `Enter`

Details deferred.

## Consequences

**Positive**: Append-only stream makes backtracking trivial. Two-phase separation is clean.

**Negative**: Interpretation overhead. Extra pass for effect execution.

## References

- [ADR-0004: Query IR Binary Format](ADR-0004-query-ir-binary-format.md)
- [ADR-0005: Transition Graph Format](ADR-0005-transition-graph-format.md)
