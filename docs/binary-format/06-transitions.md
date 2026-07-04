# Binary Format: Transitions

This section contains the Virtual Machine (VM) instructions. It is a heap of 8-byte aligned steps addressed by `StepId`. See [runtime-engine.md](../runtime-engine.md) for execution semantics.

## 1. Addressing

**StepId (u16)**: Zero-based index into this section. Byte offset = `transitions_offset + (StepId * 8)` where `transitions_offset` is computed (follows Entrypoints).

- **StepId 0 is reserved as the Terminal Sentinel.** Jumping to StepId 0 means the match is complete (Accept).
- Limit: 65,536 steps (512 KB section size).

Multi-step instructions (Match16–Match64) consume consecutive StepIds. A Match32 at StepId 5 occupies StepIds 5–8; the next instruction starts at StepId 9.

### Future: Segment-Based Addressing

The `type_id` byte reserves 2 bits for segment selection, enabling future expansion to 4 segments × 512 KB = 2 MB. Currently, only segment 0 is used. Compilers must emit `segment = 0`; runtimes should reject non-zero segments until implemented.

When implemented: Address = `(segment * 512KB) + (StepId * 8)`. Each instruction's successors must reside in the same segment; cross-segment jumps require trampoline steps.

## 2. Step Types

The first byte of every step encodes segment, node kind, and opcode:

```text
type_id (u8)
┌───────────┬──────────────┬────────────┐
│ segment(2)│ node_kind(2) │ opcode(4)  │
└───────────┴──────────────┴────────────┘
  bits 7-6     bits 5-4      bits 3-0
```

- **Bits 7-6 (Segment)**: Reserved for future multi-segment addressing. Must be 0.
- **Bits 5-4 (NodeKind)**: Node kind constraint category for Match instructions. Unused for Call/Return/Trampoline, where they must be zero; loaders reject a non-zero value.
- **Bits 3-0 (Opcode)**: Step type and size.

**NodeKind values** (for Match instructions):

| Value | Name      | Meaning                                    |
| :---- | :-------- | :----------------------------------------- |
| `00`  | Any       | No kind check (`_` pattern)                |
| `01`  | Named     | Named node check (`(_)` or `(identifier)`) |
| `10`  | Anonymous | Anonymous node check (`"text"` literals)   |
| `11`  | Reserved  | Reserved for future use                    |

| Opcode | Name       | Size     | Description                          |
| :----- | :--------- | :------- | :----------------------------------- |
| 0x0    | Match8     | 8 bytes  | Fast-path match (1 successor, no fx) |
| 0x1    | Match16    | 16 bytes | Extended match with inline payload   |
| 0x2    | Match24    | 24 bytes | Extended match with inline payload   |
| 0x3    | Match32    | 32 bytes | Extended match with inline payload   |
| 0x4    | Match48    | 48 bytes | Extended match with inline payload   |
| 0x5    | Match64    | 64 bytes | Extended match with inline payload   |
| 0x6    | Call       | 8 bytes  | Function call                        |
| 0x7    | Return     | 8 bytes  | Return from call                     |
| 0x8    | Trampoline | 8 bytes  | Universal entry point                |

### Terminal States

- **Match8**: Terminal if `next == 0`.
- **Match16–64**: Terminal if `succ_count == 0`.

`Call` and `Return` are never terminal.

## 3. Primitives

### 3.1. Nav (u8)

Bit-packed navigation command. **Bit 7** selects the family:

- **Set** — an Up-family ascent: **bits 6-5** select the mode, **bits 4-0** hold the level (`1..=31`).
- **Clear** — a standard command: **bits 6-0** are its enum value.

**Up modes** (bits 6-5, bit 7 set):

| Bits 6-5 | Mode           | Per-level exit constraint |
| :------- | :------------- | :------------------------ |
| `00`     | `Up`           | none                      |
| `01`     | `UpSkipTrivia` | last non-trivia child     |
| `10`     | `UpSkipExtras` | last non-extra child      |
| `11`     | `UpExact`      | last child                |

One instruction encodes up to 31 levels; a deeper ascent compiles to a chain of same-mode `Up*` (sound because `Up*` composes — the VM re-checks the constraint at every level). A level of `0` is invalid — every `Up*` ascends at least one level — so a loader must reject an Up byte (bit 7 set) whose level field is `0`.

**Standard commands** (bits 6-0, bit 7 clear):

- `0`: `Epsilon` (Pure control flow, no cursor movement or node check)
- `1`: `Stay` (No movement)
- `2`: `StayExact` (No movement, exact match only)
- `3`: `Next` (Sibling, skip any)
- `4`: `NextSkip` (Sibling, skip trivia)
- `5`: `NextSkipExtras` (Sibling, skip extras only)
- `6`: `NextExact` (Sibling, exact)
- `7`: `Down` (Child, skip any)
- `8`: `DownSkip` (Child, skip trivia)
- `9`: `DownSkipExtras` (Child, skip extras only)
- `10`: `DownExact` (Child, exact)
- `11..=127`: reserved

### 3.2. EffectOp (u16)

Side-effect operation code packed into 16 bits.
[]

```text
EffectOp (u16)
┌──────────────┬─────────────────────┐
│ opcode (6b)  │    payload (10b)    │
└──────────────┴─────────────────────┘
```

- **Opcode**: 6 bits (0-63).
- **Payload**: 10 bits (0-1023), member/variant index.

| Opcode | Name            | Payload                |
| :----- | :-------------- | :--------------------- |
| 0      | `Node`          | -                      |
| 1      | `ArrayOpen`     | -                      |
| 2      | `Push`          | -                      |
| 3      | `ArrayClose`    | -                      |
| 4      | `StructOpen`    | -                      |
| 5      | `Set`           | Member index (0-1023)  |
| 6      | `StructClose`   | -                      |
| 7      | `EnumOpen`      | Variant index (0-1023) |
| 8      | `EnumClose`     | -                      |
| 9      | `Null`          | -                      |
| 10     | `SuppressBegin` | -                      |
| 11     | `SuppressEnd`   | -                      |

**Opcode Ranges** (future extensibility):

| Range | Format      | Payload                        |
| :---- | :---------- | :----------------------------- |
| 0-31  | Single word | 10-bit payload in same word    |
| 32-63 | Extended    | Next u16 word is full argument |

Effect opcodes fit in the single-word range. Future predicates needing `StringId` (u16) use extended format.

**Suppression Opcodes**: `SuppressBegin` and `SuppressEnd` implement suppressive captures (`@_`). When `SuppressBegin` is executed, the VM enters suppression mode and all subsequent effects are skipped until `SuppressEnd` is executed. Suppression nesting is supported via a depth counter.

## 4. Instructions

### 4.1. Match8

Optimized fast-path transition. Used when there are no side effects, no negated fields, and exactly one destination (linear path).

```rust
#[repr(C)]
struct Match8 {
    type_id: u8,                     // segment(2) | node_kind(2) | 0x0
    nav: u8,                         // Nav
    node_kind: u16,                  // Kind ID (interpretation depends on node_kind class)
    node_field: Option<NonZeroU16>,  // None (0) means "any field"
    next: u16,                       // Next StepId. 0 = Accept.
}
```

**NodeKind class + node_kind interpretation**:

| `node_kind` class | `node_kind=0`       | `node_kind>0`              |
| :---------------- | :------------------ | :------------------------- |
| `00` (Any)        | No check (any node) | Invalid                    |
| `01` (Named)      | Check `is_named()`  | Check `kind_id() == value` |
| `10` (Anon)       | Check `!is_named()` | Check `kind_id() == value` |

**node_kind / node_field**:

Bytes 2-3 (`node_kind`) hold a tree-sitter `NodeKindId`; bytes 4-5 (`node_field`) hold a `NodeFieldId`. The runtime compares these directly against the tree-sitter node.

### 4.2. Match16–Match64

Extended transitions with inline payload. Used for side effects, negated fields, or branching (multiple successors).

**Header (8 bytes)**:

```rust
#[repr(C)]
struct MatchHeader {
    type_id: u8,                     // segment(2) | node_kind(2) | opcode(1-5)
    nav: u8,                         // Nav
    node_kind: u16,                  // Kind ID (interpretation depends on node_kind class)
    node_field: Option<NodeFieldId>, // None (0) means "any field"
    counts: u16,                     // Bit-packed element counts
}
```

**Counts Layout (u16)**:

```text
counts (u16)
┌─────────┬─────────┬──────────┬──────────┬───────┬───┐
│ pre (3) │ neg (3) │ post (3) │ succ (5) │ pred  │ 0 │
└─────────┴─────────┴──────────┴──────────┴───────┴───┘
  bits       bits      bits       bits      bit    bit
 15-13      12-10      9-7        6-2        1      0
```

- **Bits 15-13**: `pre_count` (0-7)
- **Bits 12-10**: `neg_count` (0-7)
- **Bits 9-7**: `post_count` (0-7)
- **Bits 6-2**: `succ_count` (0-31)
- **Bit 1**: `has_predicate` (if set, payload includes 4-byte predicate before successors)
- **Bit 0**: Reserved; must be 0, loaders reject a set bit

**Payload** (immediately follows header):

| Order | Content          | Type                     | Condition          |
| :---- | :--------------- | :----------------------- | :----------------- |
| 1     | `pre_effects`    | `[EffectOp; pre_count]`  | always             |
| 2     | `negated_fields` | `[u16; neg_count]`       | always             |
| 3     | `post_effects`   | `[EffectOp; post_count]` | always             |
| 4     | `predicate`      | `Predicate` (4 bytes)    | if `has_predicate` |
| 5     | `successors`     | `[u16; succ_count]`      | always             |
| 6     | Padding          | Zero bytes to step size  | always             |

**Predicate** (4 bytes, when `has_predicate` is set):

```rust
#[repr(C)]
struct Predicate {
    op: u16,        // low byte: operator; bit 8: regex flag; bits 9-15 reserved (zero, rejected at load)
    value_ref: u16, // StringId (string ops) or RegexId (regex ops)
}
```

| Op  | Name | Meaning                           |
| --- | ---- | --------------------------------- |
| 1   | `==` | Exact string match                |
| 2   | `!=` | Not equal                         |
| 3   | `^=` | Starts with                       |
| 4   | `$=` | Ends with                         |
| 5   | `*=` | Contains                          |
| 6   | `=~` | Regex match (value_ref = RegexId) |
| 7   | `!~` | Regex non-match                   |

**Payload Capacity**:

| Step    | Total Size | Payload Bytes | Max u16 Slots |
| :------ | :--------- | :------------ | :------------ |
| Match16 | 16         | 8             | 4             |
| Match24 | 24         | 16            | 8             |
| Match32 | 32         | 24            | 12            |
| Match48 | 48         | 40            | 20            |
| Match64 | 64         | 56            | 28            |

The compiler selects the smallest step size that fits the payload. If the total exceeds 28 slots, the transition must be split into a chain. With predicates (4 bytes = 2 slots), available slots for other payload items are reduced.

**Continuation Logic**:

| `succ_count` | Behavior                      | Use case                   |
| :----------- | :---------------------------- | :------------------------- |
| 0            | Accept (terminal state)       | Final state with effects   |
| 1            | `ip = successors[0]`          | Linear continuation        |
| 2+           | Branch via `successors[0..n]` | Alternation (backtracking) |

**Pre vs Post Effects**:

- `pre_effects`: Execute before match attempt (before nav, before node checks). Any effect can appear here.
- `post_effects`: Execute after successful match (after `matched_node` is set). Any effect can appear here.

The compiler places effects based on semantic requirements: scope openers often go in pre (to run regardless of which branch succeeds), captures often go in post (to access `matched_node`). But this is a compiler decision, not a bytecode-level restriction.

### 4.3. Epsilon Transitions

A Match instruction with `nav == Epsilon` is an **epsilon transition** — it succeeds unconditionally without cursor movement or node checking. The VM skips navigation and node matching entirely, only executing effects and proceeding to successors. This enables:

- **Branching at EOF**: `(a)?` must succeed when no node exists to match.
- **Pure control flow**: Decision points for quantifiers.
- **Effect-only steps**: Scope openers/closers (`StructOpen`, `StructClose`) without node interaction.

When `nav == Epsilon`:

- No cursor movement occurs
- No node kind or field checks are performed
- `node_kind` and `node_field` are ignored
- Only `pre_effects`, `post_effects`, and successors are meaningful

### 4.4. Call

Invokes another definition (recursion). Executes navigation (with optional field constraint), pushes return address to the call stack, and jumps to target.

```rust
#[repr(C)]
struct Call {
    type_id: u8,                    // segment(2) | 0 | 0x6
    nav: u8,                        // Nav
    node_field: Option<NonZeroU16>, // None (0) means "any"
    next: u16,                      // Return address (StepId, current segment)
    target: u16,                    // Callee StepId (segment from type_id)
}
```

- **Nav + Field**: Call handles navigation and field constraint. The callee's first Match checks node kind. This allows `field: (Ref)` patterns to check field and kind on the same node.
- **Target Segment**: Defined by `(type_id >> 6) & 0x3`.
- **Return Segment**: Implicitly the current segment.

### 4.5. Return

Returns from a definition. Pops the return address from the call stack.

```rust
#[repr(C)]
struct Return {
    type_id: u8,        // segment(2) | 0 | 0x7
    _pad: [u8; 7],      // reserved; must be zero, loaders reject a non-zero pad
}
```

### 4.6. Trampoline

Universal entry point instruction. Like Call, but the target comes from VM context (external parameter) rather than being encoded in the instruction. Used at address 0 for the entry preamble.

```rust
#[repr(C)]
struct Trampoline {
    type_id: u8,        // segment(2) | 0 | 0x8
    _pad1: u8,          // reserved; must be zero, loaders reject a non-zero pad
    next: u16,          // Return address (StepId)
    _pad2: [u8; 4],     // reserved; must be zero, loaders reject a non-zero pad
}
```

The preamble at step 0 typically looks like: `StructOpen → Trampoline → StructClose → Accept`. When executed:

1. VM pushes `next` (return address) onto call stack
2. VM jumps to `entrypoint_target` (set from entrypoint before execution)
3. When the entrypoint returns, execution continues at `next`

This allows a single compiled preamble to dispatch to any entrypoint without recompilation.

## 5. Execution Semantics

### 5.1. Match8 Execution

1. If `nav == Epsilon`: skip steps 2-4, go directly to step 5.
2. Execute `nav` movement.
3. Check `node_kind` according to the node_kind class:
   - `Any`: no check
   - `Named(0)`: check `is_named()`
   - `Named(id)`: check `kind_id() == id`
   - `Anonymous(0)`: check `!is_named()`
   - `Anonymous(id)`: check `kind_id() == id`
4. Check `node_field` (if not 0).
5. On failure: backtrack.
6. On success: if `next == 0` → accept; else `ip = next`.

### 5.2. Match16–64 Execution

1. If `nav != Epsilon` (an epsilon transition skips this step entirely,
   preserving `matched_node` from the previous match or return):
   1. Clear `matched_node`.
   2. Execute `nav` movement.
   3. Check `node_kind` according to the node_kind class (see Match8 Execution).
   4. Check `node_field` (if not 0).
   5. Verify all `negated_fields` are absent on the current node.
   6. Evaluate the predicate (if present).
   7. Any failed step backtracks; on success `matched_node = cursor.node()`.
2. Execute `pre_effects`.
3. Execute `post_effects`.
4. Continuation:
    - `succ_count == 0` → accept.
    - `succ_count == 1` → `ip = successors[0]`.
    - `succ_count >= 2` → push checkpoints for `successors[1..n]`, execute `successors[0]`.

### 5.3. Backtracking

On failure, pop checkpoint and resume at saved `ip`. Checkpoints store cursor position (`descendant_index`), effect watermark, and call stack state. See [runtime-engine.md](../runtime-engine.md) for details.

## 6. Quantifier Compilation

Quantifiers compile to branching patterns using epsilon transitions.

### Greedy `*` (Zero or More)

```
         ┌─────────────────┐
         ↓                 │
Entry ─ε→ Branch ─ε→ Match ─┘
           │
           └─ε→ Exit

Branch.successors = [match_path, exit_path]  // try match first
```

### Greedy `+` (One or More)

```
         ┌─────────────────┐
         ↓                 │
Entry ─→ Match ─ε→ Branch ─┘
                     │
                     └─ε→ Exit

Branch.successors = [match_path, exit_path]
```

### Non-Greedy `*?` / `+?`

Same structure, reversed successor order:

```
Branch.successors = [exit_path, match_path]  // try exit first
```

### Greedy `?` (Optional)

```
Entry ─ε→ Branch ─ε→ Match ─ε→ Exit
           │
           └─ε→ [Null] ─ε→ Exit

Branch.successors = [match_path, skip_path]
```

`Null` emits explicit null when the optional pattern doesn't match.

### Non-Greedy `??`

```
Branch.successors = [skip_path, match_path]  // try skip first
```

## 7. Alternation Compilation

Union alternations `[ A  B ]` compile to branching with null injection for type consistency.

When a capture appears in some branches but not others, the compiler injects `Null` into branches missing that capture:

```
Query: [ (a) @x  (b) ]
Type:  { x: Node | null }

Branch 1 (a): [Node, Set(x)] → Exit
Branch 2 (b): [Null, Set(x)] → Exit
```

This ensures the output object always has all fields defined, matching the type system's merged struct model.
