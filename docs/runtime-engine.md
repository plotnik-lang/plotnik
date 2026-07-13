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
    effect_depths: u64, // suppression u32 | scalar u32
}

struct Frame {
    return_addr: u16,
    parent: Option<u32>,
}
```

`cursor` is restored through tree-sitter descendant indexes stored in
checkpoints. Under sustained wide backtracking, a bounded pool of cursor
snapshots takes over: the newest checkpoints restore by copying a saved cursor
(`reset_to`, O(depth)) instead of re-navigating from an index. `frames` is an
arena-backed cactus stack so backtracking can restore call stacks without
copying them.

## Execution Cycle

The VM fetches the instruction at `ip`, executes it, and either updates `ip`,
accepts, or backtracks.

### Match

A `Match` instruction first applies its `nav`, then checks node kind, field,
negated fields, and predicate. On success, its single effects list is executed
in bytecode order. `Node` effects read the current cursor node.

Accepting a candidate found by a searching nav (non-exact `Down*`/`Next*`) is
a choice point: before proceeding, the VM pushes a match-retry checkpoint so a
later failure — anywhere downstream, including deep in the candidate's subtree
— resumes the sibling search past the accepted candidate. The checkpoint is
pushed only when the skip policy admits the candidate into the pattern's gap
(always under the default `Any` policy; for anchored searches only when the
candidate is itself trivia/extra, since a named candidate under a soft anchor
is the only legal one).

If a match has multiple successors, the VM pushes checkpoints for later
successors and tries the first successor. A zero-successor match accepts.
Branch checkpoints stack above the match-retry checkpoint, so all downstream
alternatives at one candidate are exhausted before the search moves on —
ordered-choice priority is preserved.

### Epsilon

`Nav::Epsilon` is pure control flow: no cursor movement and no node check. It is
used for branches, value/default effects, and wrapper cleanup.

### Call

`Call` applies its own navigation and optional field check, pushes a frame with
the encoded return address, and jumps to the callee target. Definition bodies
are statically verified to return at the same cursor depth they entered.

### Split call

A nullable recursive call carries matched and zero-width continuations. The
call itself does not navigate or create a retry checkpoint; its specialized
callee owns the call-site navigation. This preserves the body's exact branch
order even when consuming and zero-width outcomes are interleaved. Matched
returns keep the routed navigation depth; zero-width returns restore the
caller's original depth.

A routed matched-only call uses the same callee-owned navigation rule but has
one continuation. Keeping it distinct from an ordinary call lets validation
prove the nonzero matched return depth without a flag or sentinel.

### Return

`Return` reports matched or zero-width, pops a frame, and jumps to the
corresponding return address. The bytecode also records whether the returning
body owns entry navigation; the loader consumes that contract and the hot
runtime drops it. Returning with an empty frame stack accepts the entrypoint
only for a matched, caller-owned body.

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
    effect_depths: u64, // suppression u32 | scalar u32
    ip: StepId,
}
```

Backtracking restores cursor position, truncates the effect log, restores the
frame arena pointer, restores suppression and open-scalar depth, and then
resumes per the checkpoint's kind: a branch checkpoint resumes at its recorded instruction; a
call-retry checkpoint advances to the next candidate satisfying the Call's
skip policy and field constraint, then re-enters the callee; a match-retry
checkpoint advances past the accepted-but-failed candidate and re-runs the
same Match's candidate search from there, replaying effects and branching
exactly as the original acceptance did. Every point with alternatives leaves a
checkpoint — which sibling binds a pattern, which branch of a fan-out, whether
an optional consumes — so no search ever silently commits.

Frame pruning after `Return` keeps the arena bounded by active checkpoints plus
the current call stack.

## Effects

Effects are logged only on paths that have not backtracked. Suppression (`@_`)
increments a depth counter; ordinary data effects are skipped while the counter
is non-zero. Scalar marks bypass suppression so an enclosing `:: str` or
`:: bool` value can retain provenance across a suppressed nested value. Scalar
open and close effects obey suppression, so `matches` records no scalar trace.

```rust
pub enum RuntimeEffect<'t> {
    Node(tree_sitter::Node<'t>),
    ListOpen,
    ArrayPush,
    ListClose,
    RecordOpen,
    RecordSet(u16),
    RecordClose,
    VariantOpen(u16),
    VariantClose,
    Absent,
    ScalarOpen,
    ScalarMark(tree_sitter::Node<'t>),
    StrClose,
    BoolClose(bool),
    NodeStr(tree_sitter::Node<'t>),
    NodeBool(tree_sitter::Node<'t>),
    BoolValue(bool),
    SpanStart { id: u16, node: Option<tree_sitter::Node<'t>> },
    SpanEnd(u16),
}
```

| Effect                 | Action                                                   |
| ---------------------- | -------------------------------------------------------- |
| Node                   | Produce the current cursor node                          |
| Absent                 | Produce an absent value                                  |
| ListOpen/ListClose     | Build a list value                                       |
| ArrayPush              | Append the pending value to the list's backing array     |
| RecordOpen/RecordClose | Build a record value                                     |
| RecordSet(idx)         | Assign the pending value to a record member              |
| VariantOpen/Close      | Build a variant value                                    |
| ScalarOpen             | Begin one value-local source-provenance frame            |
| ScalarMark             | Add the current explicit node match to every open scalar |
| StrClose               | Close a scalar and produce its source slice or `null`    |
| BoolClose(value)       | Close a scalar and produce the encoded boolean           |
| NodeStr                | Produce one matched node's source text directly          |
| NodeBool               | Produce `true` for one matched node directly             |
| BoolValue(value)       | Produce a boolean without source provenance              |
| SpanStart/SpanEnd      | Bracket query-inspection provenance                      |

`ScalarMark` stores the matched node, not a byte sentinel. Each open scalar
frame unions its marks into an optional byte-range hull. No marks means no
matched node; a real zero-byte node contributes `Some(n..n)`. Consequently
`StrClose` distinguishes an absent value (`null`) from a zero-byte node (`""`).
`BoolClose` takes its value only from its encoded boolean; marks provide
inspection provenance and never implement truthiness.
Direct node scalars use `NodeStr` or `NodeBool` instead of allocating a scalar
frame; framed effects remain the general source-hull representation.
Non-inspection lowering has no consumer for boolean source provenance, so it
emits `BoolValue(true)` for a present value instead of `NodeBool` or a balanced
boolean frame. Inspection lowering retains the provenance-carrying forms.

## Entrypoint Wrappers

Every entrypoint targets a wrapper compiled for that definition's result shape.
Wrappers call the definition body and add only the effects needed to expose the
entrypoint value:

- Record result: `RecordOpen`, call body, `RecordClose`, return.
- Node result: call body, `Node`, return.
- Option/list/variant/scalar result: call body, return; the body already
  produces the pending value.
- Void result: call body, return; materialization falls back to `null`.

## Materialization

The materializer is a stack machine over the committed effect stream. Producers
(`Node`, `Absent`, and close effects) place a value in a `pending` register.
Consumers (`RecordSet`, `ArrayPush`) take that pending value and attach it to the current
builder frame. Open effects push builder frames; close effects pop them and
produce the completed value.

Scalar frames are part of the same balanced frame algebra as lists, records,
and variants. `ScalarOpen` pushes a frame, `ScalarMark` expands its hull, and
exactly one of `StrClose` or `BoolClose` closes it. Source text is sliced once
from the validated source and remains borrowed; booleans are stored directly.
The bytecode loader rejects mis-nested scalar effects before execution.

Void output is represented by an empty stream and materializes as `null`.
Tag-only enum variants emit no payload effects, so the rendered value has
`$tag` without `$data`.

Materialized values borrow captured node text from the source and member/tag
names from the bytecode string table. Rendering is unchanged; the borrows
only avoid repeated string allocation and UTF-8 validation.

Construction-time validation proves the stream discipline before the VM runs,
so these materializer assertions are inside-zone invariants.

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

There is no separate recursion limit _for the VM_. Backtracking is iterative
and call depth costs heap memory only, which the memory ceiling bounds; the
materializer renders output iteratively too.

Generated Rust matchers meter one more resource: **replay depth**, the
recursive typed reader's native-stack use. `Auto` is not input-scaled; the
emitter estimates the module's widest reader frame and the runtime resolves a
ceiling from that estimate. Safe `parse` refuses recursive output nesting past
the bound (`LimitExceeded::Depth`) while `matches` suppresses output and never
runs typed replay. The VM does not track or enforce replay depth: its
backtracking and materialization paths are iterative.

## Trivia Handling

The VM reads tree-sitter's per-node `is_extra` bit at runtime. `*Skip`
navigation skips trivia (`!node.is_named() || node.is_extra()`); `*SkipExtras`
skips only extras. A node is never skipped if it matches the current target, so
`(comment)` still matches comments.
