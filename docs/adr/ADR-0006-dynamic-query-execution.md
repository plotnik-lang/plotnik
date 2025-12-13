# ADR-0006: Query Execution

- **Status**: Accepted
- **Date**: 2024-12-12
- **Supersedes**: Parts of ADR-0003

## Context

Runtime execution of the transition graph ([ADR-0005](ADR-0005-transition-graph-format.md)). Proc-macro compilation is a future ADR.

## Decision

### Execution Order

For each transition:

1. Execute `nav` initial movement (e.g., goto_first_child, goto_next_sibling)
2. Search loop: try matcher, on fail apply skip policy (advance or fail)
3. On match success: store matched node, execute `effects` sequentially
4. Process `ref_marker` (see below)
5. Process successors with backtracking

For `Up*` variants, step 2 becomes: validate exit constraint, ascend N levels (no search loop).

**RefTransition handling** (step 4):

- `None`: no action, proceed to step 5
- `Enter(ref_id)`: push frame onto `FrameArena`, store `successors()[1..]` as returns, then jump to `successors()[0]` (definition entry)—step 5 is skipped
- `Exit(ref_id)`: verify `ref_id` matches current frame, pop frame, use stored returns as successors—step 5 uses these instead of the transition's own successors

Navigation is fully determined by `nav`—no runtime dispatch based on previous matcher. See [ADR-0008](ADR-0008-tree-navigation.md) for detailed semantics.

The matched node is stored in a temporary slot (`matched_node`) accessible to `CaptureNode` effect. Effects execute in order—`CaptureNode` reads from this slot and sets `executor.current`.

**Slot invariant**: The `matched_node` slot is cleared (set to `None`) at the start of each transition execution, before `nav`. This prevents stale captures if a transition path has `Epsilon → CaptureNode` without a preceding match—such a path indicates a graph construction bug, and the clear-on-entry invariant ensures it manifests as a predictable panic rather than silently capturing a wrong node.

### Effect Stream

```rust
struct EffectStream<'a> {
    ops: Vec<EffectOp>,           // effect log, backtrack via truncate
    nodes: Vec<Node<'a>>,         // captured nodes, one per CaptureNode op
}
```

Effects are **recorded**, not eagerly executed. On match success, the transition's `effects` list is appended to `ops`. For each `CaptureNode`, the `matched_node` is also appended to `nodes`.

On backtrack, both vectors truncate to their watermarks. On full match success, the executor replays `ops` sequentially, consuming from `nodes` for each `CaptureNode`.

### Materializer

Materializes effect stream into output value.

```rust
struct Materializer<'a> {
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

| Effect              | Action                                    |
| ------------------- | ----------------------------------------- |
| `CaptureNode`       | `current = Node(nodes.next())` (consumes) |
| `StartArray`        | push `Array([])` onto stack               |
| `PushElement`       | move `current` into top array             |
| `EndArray`          | pop array into `current`                  |
| `StartObject`       | push `Object({})` onto stack              |
| `Field(id)`         | move `current` into top object field      |
| `EndObject`         | pop object into `current`                 |
| `StartVariant(tag)` | push `Variant(tag)` onto stack            |
| `EndVariant`        | pop, wrap `current`, set as current       |
| `ToString`          | replace `current` Node with text          |

Invalid state = IR bug → panic.

### QueryInterpreter

```rust
struct QueryInterpreter<'a> {
    query: &'a CompiledQuery,
    checkpoints: CheckpointStack,
    frames: FrameArena,
    cursor: TreeCursor<'a>,  // created at tree root, never reset
    effects: EffectStream<'a>,
}
```

**Cursor constraint**: The cursor must be created once at the tree root and never call `reset()`. This preserves `descendant_index` validity for backtracking checkpoints.

No `prev_matcher` tracking needed—each transition's `nav` encodes the exact navigation to perform.

Two structures interact: backtracking can restore to a point inside a previously-exited call, so the frame arena must preserve frames.

### Checkpoints

```rust
struct CheckpointStack {
    points: Vec<Checkpoint>,
    max_frame_watermark: Option<u32>,  // highest frame index referenced by any point
}

struct Checkpoint {
    cursor_checkpoint: u32,          // tree-sitter descendant_index
    effect_watermark: u32,
    recursion_frame: Option<u32>,    // saved frame index
    prev_max_watermark: Option<u32>, // restore on pop for O(1) maintenance
    transition_id: TransitionId,     // source transition for alternatives
    next_alt: u32,                   // index of next alternative to try
}
```

Alternatives are retrieved via `TransitionView::successors()[next_alt..]`. This avoids the `Slice` incompatibility with inline successors (SSO stores successors inside the `Transition` struct, not in the `Successors` segment).

| Operation | Action                                                 |
| --------- | ------------------------------------------------------ |
| Save      | `cursor_checkpoint = cursor.descendant_index()` — O(1) |
| Restore   | `cursor.goto_descendant(cursor_checkpoint)` — O(depth) |

Restore also truncates `effects` to `effect_watermark` and sets `frame_arena.current` to `recursion_frame`.

### Recursion

**Problem**: A definition can be called from N sites. Naively, Exit's successors contain all N return points, requiring O(N) filtering.

**Solution**: Store returns in call frame at `Enter`, retrieve at `Exit`. O(1), no filtering.

```rust
struct FrameArena {
    frames: Vec<Frame>,      // append-only, pruned by watermark
    current: Option<u32>,    // index into frames (the "stack pointer")
}

struct Frame {
    parent: Option<u32>,          // index of caller's frame
    ref_id: RefId,                // verify Exit matches Enter
    enter_transition: TransitionId,  // to retrieve returns via successors()[1..]
}
```

Returns are retrieved via `TransitionView::successors()[1..]` on the `enter_transition`. Same rationale as `BacktrackPoint`—avoids `Slice` incompatibility with inline successors.

**Append-only invariant**: Frames persist for backtracking correctness. On `Exit`, set `current` to parent index. Backtracking restores `current`; the original frame is still accessible via its index.

**Frame pruning**: After `Exit`, frames at the arena top may be reclaimed if:

1. Not the current frame (already exited)
2. Not referenced by any live backtrack point

This bounds memory by `max(recursion_depth, backtrack_depth)` rather than total call count. Without pruning, `(Rule)*` over N items allocates N frames; with pruning, it remains O(1) for non-backtracking iteration.

**O(1) watermark tracking**: Each checkpoint stores the previous `max_frame_watermark`, enabling O(1) restore on pop:

```rust
impl CheckpointStack {
    fn push(&mut self, mut point: Checkpoint) {
        point.prev_max_watermark = self.max_frame_watermark;
        if let Some(frame) = point.recursion_frame {
            self.max_frame_watermark = Some(match self.max_frame_watermark {
                Some(max) => max.max(frame),
                None => frame,
            });
        }
        self.points.push(point);
    }

    fn pop(&mut self) -> Option<Checkpoint> {
        let point = self.points.pop()?;
        self.max_frame_watermark = point.prev_max_watermark;
        Some(point)
    }
}

fn prune_high_water_mark(
    current: Option<u32>,
    checkpoints: &CheckpointStack,
) -> Option<u32> {
    match (current, checkpoints.max_frame_watermark) {
        (None, None) => None,
        (Some(c), None) => Some(c),
        (None, Some(m)) => Some(m),
        (Some(c), Some(m)) => Some(c.max(m)),
    }
}
```

Frames with index > high-water mark can be truncated.

**Why not just check the last backtrack point?** Backtrack points are _not_ chronologically ordered by frame depth. After an Enter-Exit sequence, a new backtrack point may reference a shallower frame than earlier points:

```
1. Enter(A)     → frames=[F0], current=0
2. Save BP1     → BP1.recursion_frame = Some(0)
3. Exit(A)      → current = None
4. Save BP2     → BP2.recursion_frame = None

# BP2 is last, but BP1 still references F0
# Checking only last point would incorrectly allow pruning F0
```

The `max_frame_watermark` tracks the true maximum across all live points. Both push and pop are O(1)—each checkpoint stores the previous max, so pop simply restores it without scanning.

| Operation          | Action                                                                         |
| ------------------ | ------------------------------------------------------------------------------ |
| `Enter(ref_id)`    | Push frame (parent = `current`), set `current = len-1`, follow `successors[0]` |
| `Exit(ref_id)`     | Verify ref_id, set `current = frame.parent`, continue with `frame.returns`     |
| Save checkpoint    | Store `current`                                                                |
| Restore checkpoint | Set `current` to saved value                                                   |

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

Frames form a forest of call chains. Each checkpoint references an exact frame, not a depth.

### Atomic Groups (Future)

Cut/commit (discard checkpoints) works correctly: unreachable frames become garbage but cause no issues.

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

**Positive**: Append-only stacks make backtracking trivial. O(1) exit via stored returns. Navigation fully determined by `nav`—no state tracking between transitions.

**Negative**: Interpretation overhead. Recursion stack memory grows monotonically (bounded by `recursion_fuel`).

## References

- [ADR-0004: Query IR Binary Format](ADR-0004-query-ir-binary-format.md)
- [ADR-0005: Transition Graph Format](ADR-0005-transition-graph-format.md)
- [ADR-0007: Type Metadata Format](ADR-0007-type-metadata-format.md)
- [ADR-0008: Tree Navigation](ADR-0008-tree-navigation.md)
