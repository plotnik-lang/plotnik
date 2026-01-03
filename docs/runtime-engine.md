# Runtime Engine

Executes compiled query graphs against Tree-sitter syntax trees. See [06-transitions.md](binary-format/06-transitions.md) for step types.

## VM State

```rust
struct VM<'t> {
    cursor: TreeCursor<'t>,          // Never reset—preserves descendant_index for O(1) backtrack
    ip: StepId,                      // Current step index
    frames: Vec<Frame>,              // Call stack
    effects: EffectLog<'t>,          // Side-effect log
    matched_node: Option<Node<'t>>,  // Current match slot
}

struct Frame {
    return_addr: u16,   // Where to jump on Return
}
```

Lifetime `'t` denotes the parsed tree-sitter tree.

## Execution Cycle

Fetch step at `ip` → dispatch by `type_id` → execute → update `ip`.

### Match8 — Fast Path

1. Execute `nav` → check `node_type` → check `node_field`
2. Fail → backtrack
3. Success: if `next == 0` → accept; else `ip = next`

### Match16–64 — Extended Path

1. Execute `pre_effects`, clear `matched_node`
2. Execute `nav`, check `node_type`/`node_field` (see Epsilon Transitions below)
3. Success: `matched_node = cursor.node()`, verify negated fields absent
4. Execute `post_effects`
5. Continuation:
   - `succ_count == 0` → accept
   - `succ_count == 1` → `ip = successors[0]`
   - `succ_count >= 2` → branch via `successors` (backtracking)

### Epsilon Transitions

A `Match8` or `Match16–64` with `node_type: None`, `node_field: None`, and `nav: Stay` is an **epsilon transition**—it succeeds unconditionally without cursor interaction. This enables pure control-flow decisions (branching for quantifiers) even when the cursor is exhausted (EOF).

Common patterns:

- **Quantifier branches**: `(a)?` uses epsilon to decide match-or-skip
- **Trailing cleanup**: Many queries end with epsilon + `Up(n)` to restore cursor position after matching, regardless of tree depth

### Call (0x06)

Execute `nav` (with optional `node_field` check) → push `{ return_addr: next }` → `ip = target`

### Return (0x07)

Pop frame → `ip = return_addr`

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
    return_addr: u16,
    parent: Option<u32>,  // Caller's frame index
}
```

"Pop" just moves `current`—frames remain for checkpoint restoration.

### Pruning

Problem: `(a)+` accumulates frames forever. Solution: high-water mark pruning after `Return`:

```
high_water = max(current_frame_idx, checkpoint_stack.max_frame_ref)
arena.truncate(high_water + 1)
```

Bounds arena to O(max_checkpoint_depth + current_call_depth).

**O(1) Invariant**: The checkpoint stack maintains `max_frame_ref`—the highest `frame_index` referenced by any active checkpoint.

| Operation | Invariant Update                                     | Complexity     |
| --------- | ---------------------------------------------------- | -------------- |
| Push      | `max_frame_ref = max(max_frame_ref, cp.frame_index)` | O(1)           |
| Pop       | Recompute only if popping the max holder             | O(1) amortized |

Amortized analysis: each checkpoint contributes to at most one recomputation over its lifetime.

### Call/Return

Each call site stores its return address in the pushed frame.

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
pub enum RuntimeEffect<'t> {
    Node(tree_sitter::Node<'t>),
    Text(tree_sitter::Node<'t>),
    Arr,
    Push,
    EndArr,
    Obj,
    Set(u16),       // member index
    EndObj,
    Enum(u16),      // variant index
    EndEnum,
    Clear,
    Null,
}

struct EffectLog<'t>(Vec<RuntimeEffect<'t>>);
```

Lifetime `'t` denotes the parsed tree-sitter tree (per project conventions).

| Effect       | Action                                     |
| ------------ | ------------------------------------------ |
| Node(n)      | Capture node `n`                           |
| Text(n)      | Extract source text from node `n`          |
| Obj/EndObj   | Object boundaries                          |
| Set(idx)     | Assign to field at member index            |
| Arr/EndArr   | Array boundaries                           |
| Push         | Append to array                            |
| Enum/EndEnum | Tagged union boundaries (variant at index) |
| Clear        | Reset current value                        |
| Null         | Null placeholder (optional/alternation)    |

The `Node` and `Text` variants carry the actual `tree_sitter::Node` so the materializer has direct access without needing a separate node buffer. This single-stream design allows natural iteration: `for effect in log.0 { match effect { ... } }`.

### Bytecode vs Runtime Effects

**Bytecode** (`EffectOp` in `bytecode/effects.rs`): Compact 2-byte encoding with 6-bit opcode + 10-bit payload. No embedded data—the `Node` opcode signals "capture `matched_node`" but doesn't carry it.

**Runtime** (`RuntimeEffect`): The VM interprets bytecode effects and produces runtime effects with embedded data. When the VM executes a bytecode `Node` effect, it emits `RuntimeEffect::Node(matched_node)`.

### Materialization

Materializer consumes `EffectLog` to build output. Stream is purely structural; nominal types come from `Entrypoint.result_type`. See `docs/wip/materializer.md` for the materialization API.

## Fuel Limits

| Limit          | Default   | Purpose           |
| -------------- | --------- | ----------------- |
| Exec fuel      | 1,000,000 | Total transitions |
| Recursion fuel | 1,024     | Call depth        |

Exhaustion returns `RuntimeError`, not panic.

## Trivia Handling

Per-language trivia list used for `*Skip` navigation. A node is never skipped if it matches the current target—`(comment)` still matches comments.
