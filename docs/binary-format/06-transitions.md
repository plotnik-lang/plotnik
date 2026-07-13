# Binary Format: Transitions

The transitions section stores VM instructions as 8-byte-aligned steps addressed
by `StepId`. Byte offset is:

```text
transitions_offset + StepId * 8
```

StepId `0` is also the terminal successor sentinel. A real instruction may live
at step 0 as an entrypoint target, but encoded successors and call return/target
operands use `0` only for terminal where that operand allows it.

Multi-step `Match` instructions occupy consecutive 8-byte slots. For example,
`Match32` at step 5 occupies steps 5 through 8, and the next instruction starts
at step 9.

## Header Byte

```text
type_id (u8)
┌───────────┬──────────────┬────────────┐
│ segment(2)│ node_kind(2) │ opcode(4)  │
└───────────┴──────────────┴────────────┘
  bits 7-6     bits 5-4      bits 3-0
```

- `segment`: reserved, must be `0`.
- `node_kind`: used only by `Match`; must be `0` for calls and `Return`.
- `opcode`: instruction kind.

| Opcode | Name       | Size     | Description                                    |
| :----- | :--------- | :------- | :--------------------------------------------- |
| 0x0    | Match8     | 8 bytes  | Fast-path match                                |
| 0x1    | Match16    | 16 bytes | Extended match with inline payload             |
| 0x2    | Match24    | 24 bytes | Extended match with inline payload             |
| 0x3    | Match32    | 32 bytes | Extended match with inline payload             |
| 0x4    | Match48    | 48 bytes | Extended match with inline payload             |
| 0x5    | Match64    | 64 bytes | Extended match with inline payload             |
| 0x6    | Call       | 8 bytes  | Definition call                                |
| 0x7    | Return     | 8 bytes  | Return from definition or entrypoint           |
| 0x8    | SplitCall  | 8 bytes  | Nullable call with two continuations           |
| 0x9    | RoutedCall | 8 bytes  | Matched-only call with callee-owned navigation |

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

| Opcode | Name            | Payload       |
| :----- | :-------------- | :------------ |
| 0      | `Node`          | -             |
| 1      | `ArrayOpen`     | -             |
| 2      | `Push`          | -             |
| 3      | `ArrayClose`    | -             |
| 4      | `StructOpen`    | -             |
| 5      | `Set`           | Member index  |
| 6      | `StructClose`   | -             |
| 7      | `EnumOpen`      | Variant index |
| 8      | `EnumClose`     | -             |
| 9      | `Null`          | -             |
| 10     | `SuppressBegin` | -             |
| 11     | `SuppressEnd`   | -             |
| 12     | `SpanStartAt`   | Span index    |
| 13     | `SpanStart`     | Span index    |
| 14     | `SpanEnd`       | Span index    |
| 15     | `ScalarOpen`    | -             |
| 16     | `ScalarMark`    | -             |
| 17     | `StrClose`      | -             |
| 18     | `BoolClose`     | Boolean 0/1   |
| 19     | `NodeStr`       | -             |
| 20     | `NodeBool`      | -             |
| 21     | `BoolValue`     | Boolean 0/1   |

Match effects execute only after navigation and all match checks succeed, in
the list order encoded on that instruction. Span payloads index the Spans
section and must be `< spans_count`.

`ScalarOpen` and either close form one balanced scalar value frame.
`ScalarMark` snapshots the current explicit pattern match into every open
scalar frame; it is cursor-reading like `Node` and `SpanStartAt`. Scalar open
and close effects are motion barriers and must not be moved across a consuming
match. `NodeStr` and `NodeBool` are direct scalar values for one matched node;
they avoid a scalar frame when no source hull needs to be accumulated.
`BoolValue` is the equivalent no-provenance path, used notably for an absent
optional boolean. `BoolClose` and `BoolValue` accept only `0` or `1`. Every effect shown with `-`,
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
- `missing`: one bit; when set, the node must be a tree-sitter MISSING node (a
  zero-width node inserted by error recovery) — the `(MISSING …)` constraint.
  Independent of `predicate`; it forces at least the `Match16` form since the
  `Match8` fast path has no counts word.
- `reserved`: must be zero.

### Payload Order

Payload slots are 16-bit little-endian words placed immediately after the
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

## Call

```rust
#[repr(C)]
struct Call {
    type_id: u8,
    nav: u8,
    node_field: u16, // 0 = no field constraint
    next: u16,       // return address
    target: u16,     // callee entry
}
```

`Call` applies its navigation and optional field constraint before entering the
callee.

## SplitCall

```rust
#[repr(C)]
struct SplitCall {
    type_id: u8,
    entry_nav: u8,
    matched: u16, // matched return address
    zero: u16,    // zero-width return address
    target: u16,  // callee entry
}
```

`SplitCall` performs no navigation or field check. `entry_nav` records the
navigation routed into its specialized callee so the loader can verify cursor
depth without reconstructing compiler provenance. Candidate-search checkpoints
therefore remain inside the nullable body's authored branch order. A matched
`Return` resumes at `matched` at the routed navigation depth; a zero-width
`Return` resumes at `zero` at the caller's original depth.

## RoutedCall

```rust
#[repr(C)]
struct RoutedCall {
    type_id: u8,
    entry_nav: u8,
    reserved: u16,
    next: u16,
    target: u16,
}
```

`RoutedCall` is the matched-only counterpart to `SplitCall`. Its specialized
callee owns `entry_nav`, so the instruction performs no navigation itself;
the encoded value exists so validation can prove the matched return depth.
`reserved` must be zero. A routed call cannot target an ordinary or
split-return body.

## Return

```rust
#[repr(C)]
struct Return {
    type_id: u8,
    outcome: u8, // 0 = matched, 1 = zero-width
    entry: u8,   // 0 = caller-owned, 1 = routed
    _reserved: [u8; 5],
}
```

Reserved bytes must be zero. `entry` lets the loader prove that ordinary calls
target caller-navigated bodies while routed and split calls target bodies that
own entry navigation; it is not needed by the VM after validation. Return pops
a frame and selects the continuation for its outcome. If no frame exists, only
a matched, caller-owned return may accept the entrypoint.

## Validation

The loader verifies:

- section tiling by instruction size;
- reserved header/count/padding bits are zero;
- every target and successor lands on an instruction boundary;
- effect, predicate, member, type, node-kind, and field operands are in range;
- span effect operands address a real span entry;
- `Set`/`EnumOpen` payloads address a real member, `BoolClose`/`BoolValue` are `0..=1`, and
  every unit effect has a zero payload;
- calls and returns uphold cursor-depth neutrality;
- ordinary calls target matched-only bodies, split calls target bodies with
  both outcomes, and entrypoint wrappers return matched only;
- the committed effect stream cannot underflow the materializer stack or
  suppression depth, and all array/struct/enum/scalar frames are balanced;
- the committed effect stream cannot underflow or mis-nest the inspection span
  stack.
