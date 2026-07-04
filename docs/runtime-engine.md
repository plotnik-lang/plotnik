# Runtime Engine

The VM executes compiled query graphs against a tree-sitter tree. It walks a
validated bytecode module, records committed effects, and materializes the final
JSON value after a match accepts.

## VM State

```rust
struct VM<'t> {
    cursor: TreeCursor<'t>,
    ip: StepId,
    frames: FrameArena,
    checkpoints: CheckpointStack,
    effects: EffectLog<'t>,
    suppress_depth: u64,
}

struct Frame {
    return_addr: u16,
    parent: Option<u32>,
}
```

`cursor` is restored through tree-sitter descendant indexes stored in
checkpoints. `frames` is an arena-backed cactus stack so backtracking can restore
call stacks without copying them.

## Execution Cycle

The VM fetches the instruction at `ip`, executes it, and either updates `ip`,
accepts, or backtracks.

### Match

A `Match` instruction first applies its `nav`, then checks node kind, field,
negated fields, and predicate. On success, its single effects list is executed
in bytecode order. `Node` effects read the current cursor node.

If a match has multiple successors, the VM pushes checkpoints for later
successors and tries the first successor. A zero-successor match accepts.

### Epsilon

`Nav::Epsilon` is pure control flow: no cursor movement and no node check. It is
used for branches, value/default effects, and wrapper cleanup.

### Call

`Call` applies its own navigation and optional field check, pushes a frame with
the encoded return address, and jumps to the callee target. Definition bodies
are statically verified to return at the same cursor depth they entered.

### Return

`Return` pops a frame and jumps to its return address. Returning with an empty
frame stack accepts the entrypoint.

## Navigation

`Nav` byte encodes cursor movement, resolved at compile time.

| Mode                          | Behavior                          |
| ----------------------------- | --------------------------------- |
| Epsilon                       | Pure control flow                 |
| Stay                          | No movement                       |
| StayExact                     | No movement, exact match only     |
| Next/Down                     | Skip any nodes until match        |
| NextSkip/DownSkip             | Skip trivia only                  |
| NextSkipExtras/DownSkipExtras | Skip extras only                  |
| NextExact/DownExact           | Immediate match required          |
| Up(n)                         | Ascend n levels                   |
| UpSkipTrivia(n)               | Ascend n, must be last non-trivia |
| UpSkipExtras(n)               | Ascend n, must be last non-extra  |
| UpExact(n)                    | Ascend n, must be last child      |

Search navigation retries candidates according to the selected skip policy.
Exact navigation fails on the first mismatch.

## Backtracking

```rust
struct Checkpoint {
    descendant_index: u32,
    effect_watermark: usize,
    frame_index: Option<u32>,
    recursion_depth: u32,
    suppress_depth: u64,
    ip: StepId,
}
```

Backtracking restores cursor position, truncates the effect log, restores the
frame arena pointer, restores suppression depth, and resumes at the checkpoint
instruction. Checkpoints also support call retry: a failed call over a searchable
navigation can advance to the next candidate and re-enter the callee.

Frame pruning after `Return` keeps the arena bounded by active checkpoints plus
the current call stack.

## Effects

Effects are logged only on paths that have not backtracked. Suppression
(`@_`) increments a depth counter; data effects are skipped while the counter is
non-zero.

```rust
pub enum RuntimeEffect<'t> {
    Node(tree_sitter::Node<'t>),
    ArrayOpen,
    Push,
    ArrayClose,
    StructOpen,
    Set(u16),
    StructClose,
    EnumOpen(u16),
    EnumClose,
    Null,
}
```

| Effect                 | Action                             |
| ---------------------- | ---------------------------------- |
| Node                   | Produce the current cursor node    |
| Null                   | Produce a null value               |
| ArrayOpen/ArrayClose   | Build an array value               |
| Push                   | Append the pending value to array  |
| StructOpen/StructClose | Build a struct value               |
| Set(idx)               | Assign pending value to member idx |
| EnumOpen/EnumClose     | Build an enum variant              |

## Entrypoint Wrappers

Every entrypoint targets a wrapper compiled for that definition's result shape.
Wrappers call the definition body and add only the effects needed to expose the
entrypoint value:

- Struct result: `StructOpen`, call body, `StructClose`, return.
- Node result: call body, `Node`, return.
- Optional/array/enum result: call body, return; the body already produces the
  pending value.
- Void result: call body, return; materialization falls back to `null`.

## Materialization

The materializer is a stack machine over the committed effect stream. Producers
(`Node`, `Null`, and close effects) place a value in a `pending` register.
Consumers (`Set`, `Push`) take that pending value and attach it to the current
builder frame. Open effects push builder frames; close effects pop them and
produce the completed value.

Void output is represented by an empty stream and materializes as `null`.
Tag-only enum variants emit no payload effects, so the rendered value has
`$tag` without `$data`.

Load-time validation proves the stream discipline before the VM runs, so these
materializer assertions are inside-zone invariants.

## Execution Limits

A run is bounded by two resources, each a `Limit` (`Auto`, `Of(n)`, or
`Unbounded`):

| Resource | `Auto` default              | Bounds                                               |
| -------- | --------------------------- | ---------------------------------------------------- |
| Steps    | `1M + 1024 * node_count`    | total work (instruction dispatches)                  |
| Memory   | `64 MiB + 256 * node_count` | live runtime heap (frame, checkpoint, effect arenas) |

Both `Auto` ceilings scale linearly with the source's node count. Exhaustion
returns `RuntimeError` (`StepLimitExceeded` or `MemoryLimitExceeded`), never a
panic.

There is no separate recursion limit. Backtracking is iterative and call depth
costs heap memory only, which the memory ceiling bounds.

## Trivia Handling

The VM reads tree-sitter's per-node `is_extra` bit at runtime. `*Skip`
navigation skips trivia (`!node.is_named() || node.is_extra()`); `*SkipExtras`
skips only extras. A node is never skipped if it matches the current target, so
`(comment)` still matches comments.
