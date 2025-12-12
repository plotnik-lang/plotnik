# ADR-0006: Dynamic Query Execution

- **Status**: Accepted
- **Date**: 2025-12-12
- **Supersedes**: Parts of ADR-0003

## Context

Runtime interpretation of the transition graph ([ADR-0005](ADR-0005-transition-graph-format.md)). Proc-macro compilation is a future ADR.

## Decision

### Execution Order

For each transition:

1. Emit `pre_effects`
2. Match (epsilon always succeeds)
3. On success: emit `CaptureNode`, emit `post_effects`
4. Process successors with backtracking

### Effect Stream

```rust
struct EffectStream<'a> {
    effects: Vec<RuntimeEffect<'a>>,  // append-only, backtrack via truncate
}

enum RuntimeEffect<'a> {
    Op(EffectOp),
    CaptureNode(Node<'a>),  // implicit on match, never in IR
}
```

### Executor

Converts effect stream to output value.

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

### Interpreter

```rust
struct Interpreter<'a> {
    query_ir: &'a QueryIR,
    backtrack_stack: BacktrackStack,
    recursion_stack: RecursionStack,
    cursor: TreeCursor<'a>,  // created at tree root, never reset
    effects: EffectStream<'a>,
}
```

**Cursor constraint**: The cursor must be created once at the tree root and never call `reset()`. This preserves `descendant_index` validity for backtracking checkpoints.

Two stacks interact: backtracking can restore to a point inside a previously-exited call, so the recursion stack must preserve frames.

### Backtracking

```rust
struct BacktrackStack {
    points: Vec<BacktrackPoint>,
}

struct BacktrackPoint {
    cursor_checkpoint: u32,          // tree-sitter descendant_index
    effect_watermark: u32,
    recursion_frame: Option<u32>,    // saved frame index
    alternatives: Slice<TransitionId>,  // view into IR successors, not owned
}
```

`alternatives` references the IR's successor data (inline or spilled)—no runtime allocation per backtrack point.

| Operation | Action                                                 |
| --------- | ------------------------------------------------------ |
| Save      | `cursor_checkpoint = cursor.descendant_index()` — O(1) |
| Restore   | `cursor.goto_descendant(cursor_checkpoint)` — O(depth) |

Restore also truncates `effects` to `effect_watermark` and sets `recursion_stack.current` to `recursion_frame`.

### Recursion

**Problem**: A definition can be called from N sites. Naively, Exit's successors contain all N return points, requiring O(N) filtering.

**Solution**: Store returns in call frame at `Enter`, retrieve at `Exit`. O(1), no filtering.

```rust
struct RecursionStack {
    frames: Vec<CallFrame>,  // append-only
    current: Option<u32>,    // index into frames, not depth
}

struct CallFrame {
    parent: Option<u32>,          // index of caller's frame
    ref_id: RefId,                // verify Exit matches Enter
    returns: Slice<TransitionId>, // from Enter.successors()[1..]
}
```

**Append-only invariant**: Frames persist for backtracking correctness. On `Exit`, set `current` to parent index. Backtracking restores `current`; the original frame is still accessible via its index.

**Frame pruning**: After `Exit`, frames at the stack top may be reclaimed if:

1. Not the current frame (already exited)
2. Not referenced by any live backtrack point

This bounds memory by `max(recursion_depth, backtrack_depth)` rather than total call count. Without pruning, `(Rule)*` over N items allocates N frames; with pruning, it remains O(1) for non-backtracking iteration.

The `BacktrackPoint.recursion_frame` field establishes a "high-water mark"—the minimum frame index that must be preserved. Frames above this mark with no active reference can be popped.

| Operation         | Action                                                                         |
| ----------------- | ------------------------------------------------------------------------------ |
| `Enter(ref_id)`   | Push frame (parent = `current`), set `current = len-1`, follow `successors[0]` |
| `Exit(ref_id)`    | Verify ref_id, set `current = frame.parent`, continue with `frame.returns`     |
| Save backtrack    | Store `current`                                                                |
| Restore backtrack | Set `current` to saved value                                                   |

**Why index instead of depth?** Using logical depth breaks on Enter-Exit-Enter sequences:

```
Main = [(A) (B)]
A = (identifier)
B = (number)
Input: boolean

# Broken (depth-based):
1. Save BP              depth=0
2. Enter(A)             push FA, depth=1
3. Match identifier ✗
4. Exit(A)              depth=0
5. Restore BP           depth=0
6. Enter(B)             push FB, frames=[FA,FB], depth=1
7. frames[depth-1] = FA, not FB!  ← wrong frame

# Correct (index-based):
1. Save BP              current=None
2. Enter(A)             push FA{parent=None}, current=0
3. Match identifier ✗
4. Exit(A)              current=None
5. Restore BP           current=None
6. Enter(B)             push FB{parent=None}, current=1
7. frames[current] = FB ✓
```

Frames form a forest of call chains. Each backtrack point references an exact frame, not a depth.

### Atomic Groups (Future)

Cut/commit (discard backtrack points) works correctly: unreachable frames become garbage but cause no issues.

### Variant Serialization

```json
{ "$tag": "A", "$data": { ... } }
```

`$tag`/`$data` avoid capture name collisions.

### Fuel

- `transition_fuel`: decremented per transition
- `recursion_fuel`: decremented per `Enter`

Details deferred.

## Consequences

**Positive**: Append-only stacks make backtracking trivial. O(1) exit via stored returns. Two-phase separation is clean.

**Negative**: Interpretation overhead. Recursion stack memory grows monotonically (bounded by `recursion_fuel`).

## References

- [ADR-0004: Query IR Binary Format](ADR-0004-query-ir-binary-format.md)
- [ADR-0005: Transition Graph Format](ADR-0005-transition-graph-format.md)
- [ADR-0007: Type Metadata Format](ADR-0007-type-metadata-format.md)
