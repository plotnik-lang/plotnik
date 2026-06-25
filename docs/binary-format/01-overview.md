# Binary Format: Overview

64-byte header + 11 data sections. All sections are 64-byte aligned. Offsets are computed from counts.

## Architecture

- **Alignment**: Sections start on 64-byte boundaries; internal structures align to natural size (2/4/8 bytes)
- **Sequential**: Fixed order for single-pass writing
- **Endianness**: Little-endian throughout
- **Limits**: All indices u16 (max 65,535). Transitions: 512 KB max. Use `Call` to share patterns.

### Addressing

| Type                | Description                      |
| ------------------- | -------------------------------- |
| `StepId` (u16)      | 8-byte step index in Transitions |
| `StringId` (u16)    | String Table index               |
| `TypeId` (u16)      | Type Definition index            |
| `NodeKindId` (u16)  | Tree-sitter node kind ID         |
| `NodeFieldId` (u16) | Tree-sitter field ID             |
| `RegexId` (u16)     | Regex Table index                |

## Section Layout

Sections appear in fixed order, each starting on a 64-byte boundary:

| #   | Section       | Record Size | Count Source            |
| --- | ------------- | ----------- | ----------------------- |
| 0   | Header        | 64 bytes    | (fixed)                 |
| 1   | [StringBlob]  | 1           | `str_blob_size`         |
| 2   | [RegexBlob]   | 1           | `regex_blob_size`       |
| 3   | [StringTable] | 4           | `str_table_count + 1`   |
| 4   | [RegexTable]  | 8           | `regex_table_count + 1` |
| 5   | [NodeKinds]   | 4           | `node_kinds_count`      |
| 6   | [NodeFields]  | 4           | `node_fields_count`     |
| 7   | [TypeDefs]    | 4           | `type_defs_count`       |
| 8   | [TypeMembers] | 4           | `type_members_count`    |
| 9   | [TypeNames]   | 4           | `type_names_count`      |
| 10  | [Entrypoints] | 8           | `entrypoints_count`     |
| 11  | [Transitions] | 8           | `transitions_count`     |

[StringBlob]: 02-strings.md
[StringTable]: 02-strings.md
[RegexBlob]: 03-symbols.md#1-regex
[RegexTable]: 03-symbols.md#1-regex
[NodeKinds]: 03-symbols.md
[NodeFields]: 03-symbols.md
[TypeDefs]: 04-types.md
[TypeMembers]: 04-types.md
[TypeNames]: 04-types.md
[Entrypoints]: 05-entrypoints.md
[Transitions]: 06-transitions.md

### Sentinel Pattern

StringTable and RegexTable use `count + 1` entries. The final entry stores the blob size, enabling O(1) length calculation: `length[i] = table[i+1] - table[i]`.

### Offset Computation

Section offsets are not stored in the header. Loaders compute them by:

1. Start after header (offset 64)
2. For each section in order:
   - Current offset = previous section end, rounded up to 64-byte boundary
   - Section size = count × record size (or explicit size for blobs)
3. Blob sizes come from header: `str_blob_size` and `regex_blob_size`

The bytes filling each 64-byte alignment gap (and the final tail up to `total_size`) are reserved zero; loaders reject a non-zero byte in any gap.

## Header (v5)

```rust
#[repr(C, align(64))]
struct Header {
    // Bytes 0-23: Identity and sizes (6 × u32)
    magic: [u8; 4],          // b"PTKQ"
    version: u32,            // 6
    checksum: u32,           // CRC32 of everything after header
    total_size: u32,
    str_blob_size: u32,
    regex_blob_size: u32,

    // Bytes 24-41: Element counts (9 × u16) — order matches section order
    str_table_count: u16,
    regex_table_count: u16,
    node_kinds_count: u16,
    node_fields_count: u16,
    type_defs_count: u16,
    type_members_count: u16,
    type_names_count: u16,
    entrypoints_count: u16,
    transitions_count: u16,

    // Bytes 42-63: Reserved
    _reserved: [u8; 22],
}
```

## Loading & validation

`Module::load` rejects malformed input instead of failing open. A loaded module
is guaranteed not to panic on later view/decode access — for _any_ input that
passes these checks, including a deliberately forged module whose CRC was
recomputed over crafted bytes. The CRC is not a MAC, so the structural checks
(steps 5–11), not the checksum, are what uphold the no-panic guarantee.
Validation, in order:

1. **Magic / version / size** — `PTKQ`, version 6, and `total_size` equal to the
   byte length.
2. **Reserved bytes** — bytes 42–63 must be zero (the checksum does not cover the
   header, so these are checked explicitly).
3. **Section bounds** — the section layout is recomputed in 64-bit arithmetic; the
   Transitions section (and therefore every earlier section) must fit within
   `total_size`.
4. **Checksum** — CRC32 of everything after the 64-byte header must equal
   `checksum`. This catches accidental corruption of the body.
5. **Table sentinels** — String and Regex offset tables must be non-decreasing and
   end exactly at their blob length; string slices must be valid UTF-8.
6. **Regex DFAs** — every real regex entry's serialized sparse DFA must
   deserialize, so the VM's per-evaluation deserialize is a sound invariant.
7. **TypeDefs** — each kind byte must be known, and every Struct/Enum member range
   (`data + count`) must stay within `type_members_count`.
8. **String IDs** — every _required_ embedded `StringId` (entrypoint, node/field
   symbol, type, member, and regex pattern names) must address a real string-table
   entry (`1..str_table_count`), so the `NonZeroU16` accessors never panic.
9. **Transitions** — the instruction stream is walked twice. Pass 1 decodes each
   instruction's fixed-size slot, validating opcode, segment, nav, node kind,
   effect opcodes, `Set`/`EnumOpen` member operands, and predicate operands, and
   rejecting any zero successor; it records each instruction start and must tile
   the section exactly. Pass 2 requires every jump target (successor, call
   next/target, trampoline next) to land on a recorded instruction start. This
   makes every lazy `decode_step` / view / materializer access panic-free.
10. **Entrypoints** — each `target` must land on a recorded instruction start
    (not merely in range — an entrypoint into the interior of a multi-step
    instruction would start decoding mid-instruction) and `result_type` must
    address a real TypeDef.
11. **Effect stack** — an interprocedural walk of the committed-effect order
    (across `Call`/`Return`/`Trampoline`, under the suppression filter) proves no
    path can drive the materializer's builder stack (`Push`/`Set`/`ArrayClose`/
    `StructClose`/`EnumClose`) or the VM's suppression counter into a panic. This closes
    the last forged-module panic class — the materializer's builder-stack panics
    and the VM's `SuppressEnd` underflow — that decode-level checks cannot see.
