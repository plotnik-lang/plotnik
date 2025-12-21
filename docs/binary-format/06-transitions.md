# Binary Format: Transitions

This section contains the Virtual Machine (VM) instructions and associated data blocks. It is a heap of 8-byte aligned blocks addressed by `StepId`. See [runtime-engine.md](../runtime-engine.md) for execution semantics.

## 1. Addressing

**StepId (u16)**: Zero-based index into this section.

- Byte offset = `header.transitions_offset + (index * 8)`.
- Limit: 65,536 blocks (512 KB section size).

## 2. Block Types

The first byte of every block encodes both type and terminal status:

```text
type_id (u8)
┌──────────┬───────────────┐
│ term (1) │  type (7)     │
└──────────┴───────────────┘
```

- **Bit 7**: Terminal flag (`type_id & 0x80`). If set, this is an accept state—match complete.
- **Bits 0-6**: Block type (`type_id & 0x7F`).

| Code | Name           | Category    |
| :--- | :------------- | :---------- |
| 0x00 | `Match`        | Instruction |
| 0x01 | `MatchExt`     | Instruction |
| 0x02 | `Call`         | Instruction |
| 0x03 | `Return`       | Instruction |
| 0x10 | `MatchPayload` | Data        |

Terminal variants: `0x80` (Match), `0x81` (MatchExt). `Call`, `Return`, and `MatchPayload` are never terminal.

## 3. Primitives

### 3.1. Nav (u8)

Bit-packed navigation command.

| Bits 7-6 | Mode         | Bits 5-0 Payload       |
| :------- | :----------- | :--------------------- |
| `00`     | Standard     | Enum (see below)       |
| `01`     | Up           | Level count `n` (1-63) |
| `10`     | UpSkipTrivia | Level count `n` (1-63) |
| `11`     | UpExact      | Level count `n` (1-63) |

**Standard Modes**:

- `0`: `Stay` (Entry only)
- `1`: `Next` (Sibling, skip any)
- `2`: `NextSkip` (Sibling, skip trivia)
- `3`: `NextExact` (Sibling, exact)
- `4`: `Down` (Child, skip any)
- `5`: `DownSkip` (Child, skip trivia)
- `6`: `DownExact` (Child, exact)

### 3.2. EffectOp (u16)

Side-effect operation code packed into 16 bits.

```text
EffectOp (u16)
┌──────────────┬─────────────────────┐
│ opcode (6b)  │    payload (10b)    │
└──────────────┴─────────────────────┘
```

- **Opcode**: 6 bits (0-63), currently 12 defined
- **Payload**: 10 bits (0-1023), member/variant index. Limits struct/enum members to 1024.

| Opcode | Name           | Payload (10b)          |
| :----- | :------------- | :--------------------- |
| 0      | `CaptureNode`  | -                      |
| 1      | `StartArray`   | -                      |
| 2      | `PushElement`  | -                      |
| 3      | `EndArray`     | -                      |
| 4      | `StartObject`  | -                      |
| 5      | `EndObject`    | -                      |
| 6      | `SetField`     | Member index (0-1023)  |
| 7      | `StartVariant` | Variant index (0-1023) |
| 8      | `EndVariant`   | -                      |
| 9      | `ToString`     | -                      |
| 10     | `ClearCurrent` | -                      |
| 11     | `PushNull`     | -                      |

**Object vs Scalar List Context**:

The VM builds **Array of Structs** (AoS), not Structure of Arrays (SoA). This affects opcode usage:

- **Scalar lists** (`(x)* @items`): `StartArray` → loop(`CaptureNode`, `PushElement`) → `EndArray`, `SetField`
- **Row lists** (`{ (x) @x }* @rows`): `StartArray` → loop(`StartObject`, `CaptureNode`, `SetField`, `EndObject`, `PushElement`) → `EndArray`, `SetField`

Arrays are built on a value stack and assigned to fields via `SetField`.

`PushNull` emits explicit null values for:

- Optional fields when the optional branch is skipped
- Alternation branches missing a capture present in other branches

Member/variant indices are resolved via `type_members[struct_or_enum.members.start + index]`.

### Opcode Ranges (Future Extensibility)

Opcodes are partitioned by argument size:

| Range | Format      | Payload                        |
| :---- | :---------- | :----------------------------- |
| 0-31  | Single word | 10-bit payload in same word    |
| 32-63 | Extended    | Next u16 word is full argument |

Current opcodes (0-11) fit in the single-word range. Future predicates needing `StringId` (u16) use extended format:

```
// Single word (current)
SetField:     [opcode=6 | member_idx]

// Extended (future)
AssertEqText: [opcode=32 | reserved], [StringId]
AssertMatch:  [opcode=33 | flags],    [RegexId]
```

This maintains backwards compatibility—existing binaries use only opcodes < 32.

## 4. Instructions

All instructions are exactly 8 bytes.

**Note**: In tree-sitter, `NodeTypeId` 0 is reserved for an internal "end" sentinel and is never exposed via the Cursor API. Languages with an actual `end` keyword (Ruby, Lua, etc.) assign it a different non-zero ID. Similarly, `NodeFieldId` 0 is never valid. We use `Option<NonZeroU16>` to represent these values, where `None` (stored as `0`) indicates no check (wildcard).

**Epsilon Transitions**: A `MatchExt` with `node_type: None`, `node_field: None`, and `nav: Stay` is an **epsilon transition**—it succeeds unconditionally without cursor interaction. This is critical for:

- **Branching at EOF**: `(a)?` must succeed when no node exists to match
- **Trailing navigation**: Many queries end with epsilon + `Up(n)` to restore cursor position after matching descendants

Epsilon transitions bypass the normal "check node exists → check type → check field" logic entirely. They execute effects and select successors without touching the cursor.

### 4.1. Match

Optimized fast-path transition.

```rust
#[repr(C)]
struct Match {
    type_id: u8,                     // 0x00 or 0x80 (terminal)
    nav: u8,                         // Nav
    node_type: Option<NodeTypeId>,   // None means "any"
    node_field: Option<NodeFieldId>, // None means "any"
    next: u16,                       // Next StepId (ignored if terminal)
}
```

When `type_id & 0x80` is set, the match succeeds and accepts—`next` is ignored.

### 4.2. MatchExt

Extended transition pointing to a payload block.

```rust
#[repr(C)]
struct MatchExt {
    type_id: u8,                     // 0x01
    nav: u8,                         // Nav
    node_type: Option<NodeTypeId>,   // None means "any"
    node_field: Option<NodeFieldId>, // None means "any"
    payload: u16,                    // StepId to MatchPayload
}
```

### 4.3. Call

Invokes another definition (recursion). Pushes `next` to the call stack and jumps to `target`.

```rust
#[repr(C)]
struct Call {
    type_id: u8,        // 0x02
    reserved: u8,
    next: u16,          // Return address (StepId)
    target: u16,        // Callee StepId
    ref_id: u16,        // Must match Return.ref_id
}
```

### 4.4. Return

Returns from a definition. Pops the return address from the call stack.

```rust
#[repr(C)]
struct Return {
    type_id: u8,        // 0x03
    reserved: u8,
    ref_id: u16,        // Must match Call.ref_id (invariant check)
    _pad: u32,
}
```

### 4.5. The `ref_id` Invariant

The `ref_id` field enforces stack discipline between `Call` and `Return` instructions. Each definition gets a unique `ref_id` at compile time. At runtime:

1. `Call` pushes a frame with its `ref_id` onto the call stack.
2. `Return` verifies its `ref_id` matches the current frame's `ref_id`.
3. Mismatch indicates a malformed query or VM bug—panic in debug builds.

This catches errors like mismatched call/return pairs or corrupted stack state during backtracking. The check is O(1) and provides strong guarantees about control flow integrity.

## 5. Data Blocks

Variable-length blocks. The total size must be padded to a multiple of 8 bytes.

> **Note**: These blocks are included in the Transitions segment to allow co-location with related instructions (e.g., placing `MatchPayload` immediately after `MatchExt`) to optimize for CPU cache locality.

### 5.1. MatchPayload

Contains extended logic for `MatchExt`.

```rust
#[repr(C)]
struct MatchPayloadHeader {
    type_id: u8,       // 0x10
    reserved: u8,
    pre_count: u8,     // Count of Pre-Effects
    neg_count: u8,     // Count of Negated Fields
    post_count: u8,    // Count of Post-Effects
    succ_count: u8,    // Count of Successors
    _pad: u16,
}
```

**Body Layout** (contiguous, u16 aligned, matches header order):

1. `pre_effects`: `[EffectOp; pre_count]`
2. `negated_fields`: `[u16; neg_count]`
3. `post_effects`: `[EffectOp; post_count]`
4. `successors`: `[u16; succ_count]` (StepIds)

**Pre vs Post Effects**:

- `pre_effects`: Execute before match attempt. Used for scope openers (`StartObject`, `StartArray`, `StartVariant`) that must run regardless of which branch succeeds.
- `post_effects`: Execute after successful match. Used for capture/assignment ops (`CaptureNode`, `SetField`, `EndObject`, etc.) that depend on `matched_node`.

**Continuation Logic**:

| `succ_count` | Behavior                      | Use case                   |
| :----------- | :---------------------------- | :------------------------- |
| 0            | Check terminal bit            | Accept or invalid          |
| 1            | `ip = successors[0]`          | Linear continuation        |
| 2+           | Branch via `successors[0..n]` | Alternation (backtracking) |

When `succ_count == 0`, the owning `MatchExt` must have the terminal bit set (`type_id == 0x81`). This executes effects and accepts. A non-terminal `MatchExt` with `succ_count == 0` is invalid (no continuation path).

**Contrast with `Match`**: The simpler `Match` block has inline `next` and uses the terminal bit directly. `MatchExt` uses `succ_count` for branching, with `succ_count == 0` + terminal bit for accept states that need effects.

## 6. Quantifier Compilation

Quantifiers compile to branching patterns in the transition graph.

**Note on "Branch" blocks**: The diagrams below use "Branch" as a logical construct. In the actual bytecode, a Branch is implemented as a `MatchExt` with:

- `node_type: None` (no type check)
- `nav: Stay` (no cursor movement)
- `succ_count >= 2` (multiple successors)

This combination creates an **epsilon transition**—a decision point that doesn't consume input, only selects which path to follow.

### Greedy `*` (Zero or More)

```
         ┌─────────────────┐
         ↓                 │
Entry ─ε→ Branch ─ε→ Match ─┘
           │
           └─ε→ Exit

Branch.successors = [match, exit]  // try match first
```

### Greedy `+` (One or More)

```
         ┌─────────────────┐
         ↓                 │
Entry ─→ Match ─ε→ Branch ─┘
                     │
                     └─ε→ Exit

Branch.successors = [match, exit]
```

### Non-Greedy `*?` / `+?`

Same structure as greedy, but successor order is reversed:

```
Branch.successors = [exit, match]  // try exit first
```

### Greedy `?` (Optional)

```
Entry ─ε→ Branch ─ε→ Match ─ε→ Exit
           │
           └─ε→ [PushNull] ─ε→ Exit

Branch.successors = [match, skip]  // try match first
```

The `PushNull` effect on the skip path emits an explicit null value when the optional pattern doesn't match. This distinguishes "not present" (`null`) from "not attempted." In alternations and optional captures, downstream consumers can differentiate between a missing match and a match that produced no value.

## 7. Alternation Compilation

Untagged alternations `[ A  B ]` compile to branching with **symmetric null injection** for type consistency.

### Null Injection in Alternations

When a capture appears in some branches but not others, the type system produces an optional field (`x?: T`). The compiler injects `PushNull` into branches missing that capture:

```
Query: [ (a) @x  (b) ]
Type:  { x?: Node }

Branch 1 (a): [CaptureNode, SetField(x)] → Exit
Branch 2 (b): [PushNull, SetField(x)]    → Exit
                 ↑ injected
```

The output object always has the `x` field set—either to a node or to null. This matches the type system's merged struct model.

### Multiple Captures

Each missing capture gets its own `PushNull`:

```
Query: [
  { (a) @x (b) @y }
  { (c) @x }
  (d)
]
Type: { x?: Node, y?: Node }

Branch 1: [CaptureNode, SetField(x), CaptureNode, SetField(y)]
Branch 2: [CaptureNode, SetField(x), PushNull, SetField(y)]
Branch 3: [PushNull, SetField(x), PushNull, SetField(y)]
```

This ensures the output object has all fields defined, with nulls for unmatched captures.

### Non-Greedy `??`

Same structure as `?`, but successor order is reversed:

```
Branch.successors = [skip, match]  // try skip first
```

### Example: Array Capture

Query: `(parameters (identifier)* @params)`

Compiled graph (after epsilon elimination):

```
T0: MatchExt(identifier) [StartArray, CaptureNode, PushElement]  → [T0, T1]
T1: Match [EndArray, SetField("params")]                         → next
```

The first iteration gets `StartArray` from the entry path. Loop iterations execute only `CaptureNode, PushElement`. On exit, `EndArray` finalizes the array.
