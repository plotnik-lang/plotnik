# Binary Format: Instructions

The Instructions section stores VM instructions in 8-byte bytecode words. A
`CodeAddr` identifies a word; its byte offset is:

```text
instructions_offset + CodeAddr * 8
```

`CodeAddr(0)` is a valid instruction address, including for entry points and
call targets. Match successors and call continuations use `SuccessorAddr`
operands, where raw `0` means terminal or unused; those targets must therefore
be nonzero. Layout leaves word zero as padding only when its first entry is also
referenced through a `SuccessorAddr`.

Multi-word `Match` and `CallN` instructions occupy consecutive bytecode words. For example,
`Match32` at address 5 occupies words 5 through 8, and the next instruction
starts at address 9.

## Header Byte

```text
header (u8)
┌───────────┬──────────────┬────────────┐
│ segment(2)│ flags(2)     │ opcode(4)  │
└───────────┴──────────────┴────────────┘
  bits 7-6     bits 5-4      bits 3-0
```

- `segment`: reserved, must be `0`.
- `flags`: node-kind class for `Match`; ownership/port flags for calls; entry
  ownership for `Return`.
- `opcode`: instruction kind.

For `Match`, node-kind class `00` requires `node_kind == 0`. Classes `01`
(named) and `10` (anonymous) use zero for a class-only wildcard or a specific
`NodeKindId`; `0xfffe`, Tree-sitter's internal `_ERROR` symbol, is invalid.
Class `11` is reserved. The public `ERROR` kind remains representable as
`0xffff`.

| Opcode | Name    | Size     | Description                                     |
| :----- | :------ | :------- | :---------------------------------------------- |
| 0x0    | Match8  | 8 bytes  | Fast-path match                                 |
| 0x1    | Match16 | 16 bytes | Extended match with inline payload              |
| 0x2    | Match24 | 24 bytes | Extended match with inline payload              |
| 0x3    | Match32 | 32 bytes | Extended match with inline payload              |
| 0x4    | Match48 | 48 bytes | Extended match with inline payload              |
| 0x5    | Match64 | 64 bytes | Extended match with inline payload              |
| 0x6    | Call1   | 8 bytes  | One-port definition call                        |
| 0x7    | CallN   | 24 bytes | Definition call with two to eight continuations |
| 0x8    | Return  | 8 bytes  | Return from definition or entry point           |

## Navigation

`Nav` is one byte. Bit 7 selects the `Up*` family; otherwise bits 6-0 are a
standard command.

| Byte         | Command                         |
| ------------ | ------------------------------- |
| `0`          | `Epsilon`                       |
| `1`          | `Stay`                          |
| `2`          | `StayExact`                     |
| `3`          | `Next`                          |
| `4`          | `NextSkip`                      |
| `5`          | `NextSkipExtras`                |
| `6`          | `NextExact`                     |
| `7`          | `Down`                          |
| `8`          | `DownSkip`                      |
| `9`          | `DownSkipExtras`                |
| `10`         | `DownExact`                     |
| `11`         | `ChildlessSkipTrivia`           |
| `12`         | `ChildlessSkipExtras`           |
| `13`         | `ChildlessExact`                |
| `0b1mmnnnnn` | `Up*`, mode `mm`, level `nnnnn` |

For `Up*`, level must be `1..=31`; level `0` is invalid. Modes are:

| Bits 6-5 | Command        |
| -------- | -------------- |
| `00`     | `Up`           |
| `01`     | `UpSkipTrivia` |
| `10`     | `UpSkipExtras` |
| `11`     | `UpExact`      |

## Effects

`EffectOp` is a compact `u16`.

```text
EffectOp (u16)
┌──────────────┬─────────────────────┐
│ opcode (6b)  │    payload (10b)    │
└──────────────┴─────────────────────┘
```

| Opcode | Name            | Payload      |
| :----- | :-------------- | :----------- |
| 0      | `Node`          | -            |
| 1      | `ListOpen`      | -            |
| 2      | `ArrayPush`     | -            |
| 3      | `ListClose`     | -            |
| 4      | `RecordOpen`    | -            |
| 5      | `RecordSet`     | Member index |
| 6      | `RecordClose`   | -            |
| 7      | `VariantOpen`   | Member index |
| 8      | `VariantClose`  | -            |
| 9      | `Absent`        | -            |
| 10     | `SuppressBegin` | -            |
| 11     | `SuppressEnd`   | -            |
| 12     | `SpanStartAt`   | Span index   |
| 13     | `SpanStart`     | Span index   |
| 14     | `SpanEnd`       | Span index   |
| 15     | `ScalarOpen`    | -            |
| 16     | `ScalarMark`    | -            |
| 17     | `TextClose`     | -            |
| 18     | `BoolClose`     | Boolean 0/1  |
| 19     | `NodeText`      | -            |
| 20     | `NodeBool`      | -            |
| 21     | `BoolValue`     | Boolean 0/1  |

Match effects execute only after navigation and all match checks succeed, in
the list order encoded on that instruction. Span payloads index the Spans
section and must be `< spans_count`.

`ScalarOpen` and either close form one balanced scalar value frame.
`ScalarMark` snapshots the current explicit pattern match into every open
scalar frame; it is cursor-reading like `Node` and `SpanStartAt`. Scalar open
and close effects are motion barriers and must not be moved across a consuming
match. `NodeText` and `NodeBool` are direct scalar values for one matched node;
they avoid a scalar frame when no document bounding range needs to be accumulated.
`BoolValue` is the equivalent no-provenance path, used notably for a presence
boolean's absent fallback. `BoolClose` and `BoolValue` accept only `0` or `1`. Every effect shown with `-`,
including all other scalar effects, requires a zero payload.

## Match8

Used when a match has no effects, no negated fields, no predicate, and at most
one successor.

```rust
#[repr(C)]
struct Match8 {
    type_id: u8,
    nav: u8,
    node_kind: u16,
    node_field: u16, // 0 = any field
    next: u16,       // 0 = terminal
}
```

## Match16-64

Extended matches add an inline payload after the fixed 8-byte header.

```rust
#[repr(C)]
struct MatchHeader {
    type_id: u8,
    nav: u8,
    node_kind: u16,
    node_field: u16, // 0 = any field
    counts: u16,
}
```

### Counts Word

```text
counts (u16)
┌────────────┬────────┬─────────┬───────────┬─────────┬─────────────┐
│ effects(4) │ neg(3) │ succ(5) │ predicate │ missing │ reserved(2) │
└────────────┴────────┴─────────┴───────────┴─────────┴─────────────┘
 bits 15-12   11-9     8-4       bit 3       bit 2     bits 1-0
```

- `effects`: number of inline `EffectOp` payload slots, max 15.
- `neg`: number of negated field IDs, max 7.
- `succ`: number of successors, max 31.
- `predicate`: one bit; when set, two payload slots hold the predicate.
- `missing`: one bit; when set, the node must be a Tree-sitter MISSING node (a
  zero-byte node inserted by error recovery) — the `(MISSING …)` constraint.
  Independent of `predicate`; it forces at least the `Match16` form since the
  `Match8` fast path has no counts word.
- `reserved`: must be zero.

### Payload Order

Operand slots are 16-bit little-endian values placed immediately after the
header:

1. `effects`
2. `negated_fields`
3. `predicate` (`op_flags`, `value_ref`) when present
4. `successors`
5. zero padding to the selected instruction size

## Predicate Payload

```rust
#[repr(C)]
struct Predicate {
    op_flags: u16,
    value_ref: u16,
}
```

`op_flags` low byte stores the string/regex operator. Bit 8 marks regex mode.
Higher bits are reserved and must be zero. `value_ref` indexes either the string
table or regex table, depending on regex mode.

## Call1

```rust
#[repr(C)]
struct Call1 {
    header: u8,
    nav: u8,
    node_field: u16, // 0 = no field constraint
    next: u16,       // SuccessorAddr: return address
    target: u16,     // CodeAddr: callee entry
}
```

The header flag bits are:

- bit 0: entry ownership (`0` caller, `1` callee);
- bit 1: whether port 0 consumed a node.

Caller-owned calls apply `nav` and `node_field` before entering the callee and
must expose a consuming port. Callee-owned calls retain the authored entry
obligation for validation; their specialized target performs the actual entry
work.

## CallN

```rust
#[repr(C)]
struct CallN {
    header: u8,
    nav: u8,
    node_field: u16, // 0 = no field constraint
    target: u16,     // CodeAddr: callee entry
    arity: u8,       // 2..=8
    consumed_mask: u8,
    returns: [u16; 8],
}
```

Header flag bit 0 stores entry ownership; bit 1 is reserved and must be zero.
The first `arity` return slots are nonzero `SuccessorAddr` values indexed by
the callee-local dense `PortId`; unused slots are zero. `consumed_mask` records
the cursor contract of each port. Bits outside `arity` must be zero, and every
port of a caller-owned call must be consuming.

## Return

```rust
#[repr(C)]
struct Return {
    header: u8,
    port: u8, // 0..=7
    nav: u8,
    _reserved0: u8,
    node_field: u16, // 0 = no field constraint
    _reserved1: u16,
}
```

Header flag bit 0 stores the callee entry ownership and bit 1 is reserved.
Caller-owned returns must encode the canonical body contract `Stay` with no
field. Callee-owned returns encode the exact authored navigation and optional
field embedded in that specialization. Reserved bytes must be zero.

At runtime, only `port` participates in dispatch: Return pops a frame containing
the immutable call-site address, then resolves `(call_site, port)` through that
call's continuation map. The remaining metadata is load-time validation data.
If no frame exists, only port 0 may accept the entry point.

## Validation

The loader verifies:

- section tiling by instruction size;
- reserved header/count/padding bits are zero;
- every target and successor lands on an instruction boundary;
- effect, predicate, member, type, node-kind, and field operands are in range;
- span effect operands address a real span entry;
- `RecordSet`/`VariantOpen` payloads address a real member, `BoolClose`/`BoolValue` are `0..=1`, and
  every unit effect has a zero payload;
- every return reachable from one callee declares one uniform entry contract;
- every call exactly matches its callee's entry ownership, authored navigation,
  optional field, dense arity, and per-port cursor contract;
- callee ports are dense, every call supplies exactly the callee arity, calls
  to one specialized target agree on entry ownership and port behavior, and
  entry point definitions use the caller-owned contract and return through port 0 only;
- every body returns at its declared cursor depth, and cursor-reading effects
  cannot execute before a path consumes a node;
- every accepted output-event path keeps the materializer stack balanced;
- suppression effects cannot underflow their depth and finish balanced;
- the committed match journal cannot underflow or mis-nest the inspection span
  stack.
