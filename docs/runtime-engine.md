# Runtime Engine

Executes compiled query graphs against Tree-sitter syntax trees. See [06-transitions.md](binary-format/06-transitions.md) for block types.

## VM State

```rust
struct VM<'a> {
    cursor: TreeCursor<'a>,          // Never reset—preserves descendant_index for O(1) backtrack
    ip: StepId,                      // Current block index
    frames: Vec<Frame>,              // Call stack
    effects: EffectStream<'a>,       // Side-effect log
    matched_node: Option<Node<'a>>,  // Current match slot
}

struct Frame {
    ref_id: u16,        // For Return verification
    return_addr: u16,   // Where to jump on Return
}
```

## Execution Cycle

Fetch block at `ip` → dispatch by `type_id` → execute → update `ip`.

### Match — Fast Path

1. Execute `nav` → check `node_type` → check `node_field`
2. Fail → backtrack
3. Success: if terminal (`type_id & 0x80`) → accept; else `ip = next`

### MatchExt — Extended Path

1. Fetch `MatchPayload`, execute `pre_effects`, clear `matched_node`
2. Execute `nav`, check `node_type`/`node_field` (see Epsilon Transitions below)
3. Success: `matched_node = cursor.node()`, verify negated fields absent
4. Execute `post_effects`
5. Continuation:
   - Terminal (`type_id & 0x80`) → accept (requires `succ_count == 0`)
   - `succ_count == 1` → `ip = successors[0]`
   - `succ_count >= 2` → branch via `successors` (backtracking)

### Epsilon Transitions

A `MatchExt` with `node_type: None` and `nav: Stay` is an **epsilon transition**—it succeeds unconditionally without cursor interaction. This enables pure control-flow decisions (branching for quantifiers) even when the cursor is exhausted (EOF).

Common patterns:

- **Quantifier branches**: `(A)?` uses epsilon to decide match-or-skip
- **Trailing cleanup**: Many queries end with epsilon + `Up(n)` to restore cursor position after matching, regardless of tree depth

### Call (0x02)

Push `{ ref_id, return_addr: next }` → `ip = target`

### Return (0x03)

Pop frame → verify `ref_id` match (panic on mismatch) → `ip = return_addr`

## Navigation

`Nav` byte encodes cursor movement, resolved at compile time.

| Mode                | Behavior                          |
| ------------------- | --------------------------------- |
| Stay                | No movement                       |
| Next/Down           | Skip any nodes until match        |
| NextSkip/DownSkip   | Skip trivia only                  |
| NextExact/DownExact | Immediate match required          |
| Up(n)               | Ascend n levels                   |
| UpSkipTrivia(n)     | Ascend n, must be last non-trivia |
| UpExact(n)          | Ascend n, must be last child      |

### Search Loop

1. Move cursor → try match
2. On fail: Exact → fail; Skip → fail if non-trivia, else retry; Any → retry
3. On exhaustion: fail

Example: `(foo (bar))` vs `(foo (foo) (foo) (bar))` with `Down` mode skips two `foo` children to find `bar`. With `DownExact`, first mismatch fails immediately.

## Recursion

### Cactus Stack

Backtracking needs to restore frames destroyed by failed branches. Solution: arena + parent pointer.

```rust
struct FrameArena {
    frames: Vec<Frame>,   // Append-only
    current: Option<u32>, // "Stack pointer"
}
struct Frame {
    ref_id: u16,
    return_addr: u16,
    parent: Option<u32>,  // Caller's frame index
}
```

"Pop" just moves `current`—frames remain for checkpoint restoration.

### Pruning

Problem: `(A)+` accumulates frames forever. Solution: high-water mark pruning after `Return`:

```
high_water = max(current_frame_idx, max_checkpoint_watermark)
arena.truncate(high_water + 1)
```

Bounds arena to O(max_checkpoint_depth + current_call_depth).

### Call/Return

Each call site stores its return address in the pushed frame. The `ref_id` check catches stack corruption (malformed IR or VM bug).

## Backtracking

```rust
struct Checkpoint {
    descendant_index: u32,    // Cursor position
    effect_watermark: usize,  // Effect stream length
    frame_index: Option<u32>, // Frame arena state
    ip: StepId,               // Resume point
}
```

### Process

1. **Save**: Push checkpoint, track `max_frame_watermark` for pruning
2. **Restore**: `goto_descendant()`, truncate effects, set `frames.current`
3. **Resume**: `ip = checkpoint.ip`

### Branching (`succ_count > 1`)

Save checkpoint for `successors[1..]` → try `successors[0]` → on fail, restore and try next.

## Effects

Operations logged instead of inline output. Backtracking: `truncate(watermark)`.

```rust
struct EffectStream<'a> {
    ops: Vec<EffectOp>,
    nodes: Vec<Node<'a>>,
}
```

| Effect              | Action                             |
| ------------------- | ---------------------------------- |
| CaptureNode         | Push `matched_node`                |
| Start/EndObject     | Object boundaries                  |
| SetField(id)        | Assign to field                    |
| PushField(id)       | Append to array field (columnar)   |
| Start/EndArray      | Array boundaries                   |
| PushElement         | Append to array                    |
| Start/EndVariant(t) | Tagged union boundaries            |
| ToString            | Node → source text                 |
| ClearCurrent        | Reset current value                |
| PushNull            | Null placeholder (`?` in columnar) |

### Materialization

Materializer replays effects to build output. Stream is purely structural; nominal types come from `Entrypoint.result_type`.

## Fuel Limits

| Limit          | Default   | Purpose           |
| -------------- | --------- | ----------------- |
| Exec fuel      | 1,000,000 | Total transitions |
| Recursion fuel | 1,024     | Call depth        |

Exhaustion returns `RuntimeError`, not panic.

## Trivia Handling

Per-language trivia list used for `*Skip` navigation. A node is never skipped if it matches the current target—`(comment)` still matches comments.
