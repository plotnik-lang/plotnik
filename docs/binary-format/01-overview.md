# Binary Format: Overview

64-byte Header + 13 Sections. All sections 64-byte aligned. Offsets computed from counts.

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
| `NodeTypeId` (u16)  | Tree-sitter node type ID         |
| `NodeFieldId` (u16) | Tree-sitter field ID             |
| `RegexId` (u16)     | Regex Table index                |

## Section Layout

Sections appear in fixed order, each starting on a 64-byte boundary:

| #  | Section       | Record Size | Count Source          |
| -- | ------------- | ----------- | --------------------- |
| 0  | Header        | 64 bytes    | (fixed)               |
| 1  | [StringBlob]  | 1           | `str_blob_size`       |
| 2  | [RegexBlob]   | 1           | `regex_blob_size`     |
| 3  | [StringTable] | 4           | `str_table_count + 1` |
| 4  | [RegexTable]  | 8           | `regex_table_count + 1`     |
| 5  | [NodeTypes]   | 4           | `node_types_count`    |
| 6  | [NodeFields]  | 4           | `node_fields_count`   |
| 7  | [Trivia]      | 2           | `trivia_count`        |
| 8  | [TypeDefs]    | 4           | `type_defs_count`     |
| 9  | [TypeMembers] | 4           | `type_members_count`  |
| 10 | [TypeNames]   | 4           | `type_names_count`    |
| 11 | [Entrypoints] | 8           | `entrypoints_count`   |
| 12 | [Transitions] | 8           | `transitions_count`   |

[StringBlob]: 02-strings.md
[StringTable]: 02-strings.md
[RegexBlob]: 03-symbols.md#4-regex
[RegexTable]: 03-symbols.md#4-regex
[NodeTypes]: 03-symbols.md
[NodeFields]: 03-symbols.md
[Trivia]: 03-symbols.md
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

## Header (v2)

```rust
#[repr(C, align(64))]
struct Header {
    // Bytes 0-23: Identity and sizes (6 × u32)
    magic: [u8; 4],          // b"PTKQ"
    version: u32,            // 2
    checksum: u32,           // CRC32 of everything after header
    total_size: u32,
    str_blob_size: u32,
    regex_blob_size: u32,

    // Bytes 24-45: Element counts (11 × u16) — order matches section order
    str_table_count: u16,
    regex_table_count: u16,
    node_types_count: u16,
    node_fields_count: u16,
    trivia_count: u16,
    type_defs_count: u16,
    type_members_count: u16,
    type_names_count: u16,
    entrypoints_count: u16,
    transitions_count: u16,
    flags: u16,

    // Bytes 46-63: Reserved
    _reserved: [u8; 18],
}
```

### Flags

| Bit | Name   | Description                                              |
| --- | ------ | -------------------------------------------------------- |
| 0   | LINKED | If set, bytecode contains resolved NodeTypeId/NodeFieldId |

**Linked vs Unlinked**:

- **Linked**: Match instructions store tree-sitter `NodeTypeId` and `NodeFieldId` directly. Executable immediately.
- **Unlinked**: Match instructions store `StringId` references. Requires linking against a grammar before execution.
