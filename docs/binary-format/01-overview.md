# Binary Format: Overview

64-byte Header + 8 aligned Sections.

## Architecture

- **Alignment**: Sections start on 64-byte boundaries; internal structures align to natural size (2/4/8 bytes)
- **Sequential**: Fixed order for single-pass writing
- **Endianness**: Little Endian
- **Limits**: All indices u16 (max 65,535). Transitions: 512 KB max. Use `Call` to share patterns.

### Addressing

| Type                | Description                      |
| ------------------- | -------------------------------- |
| `StepId` (u16)      | 8-byte step index in Transitions |
| `StringId` (u16)    | String Table index               |
| `TypeId` (u16)      | Type Definition index            |
| `NodeTypeId` (u16)  | Tree-sitter node type ID         |
| `NodeFieldId` (u16) | Tree-sitter field ID             |

## Memory Layout

Section offsets defined in Header for robust parsing.

| Section       | Content                  | Record Size |
| ------------- | ------------------------ | ----------- |
| Header        | Meta                     | 64          |
| [StringBlob]  | UTF-8                    | 1           |
| [StringTable] | StringId → Offset+Length | 4           |
| [NodeTypes]   | NodeTypeId → StringId    | 4           |
| [NodeFields]  | NodeFieldId → StringId   | 4           |
| [Trivia]      | List of NodeTypeId       | 2           |
| [TypeMeta]    | Types (3 sub-sections)   | 4           |
| [Entrypoints] | Definitions              | 8           |
| [Transitions] | Tree walking graph       | 8           |

**TypeMeta sub-sections** (contiguous, offsets computed from counts):

- **TypeDefs**: Structural topology
- **TypeMembers**: Fields and variants
- **TypeNames**: Name → TypeId mapping

[StringBlob]: 02-strings.md
[StringTable]: 02-strings.md
[NodeTypes]: 03-symbols.md
[NodeFields]: 03-symbols.md
[Trivia]: 03-symbols.md
[TypeMeta]: 04-types.md
[Entrypoints]: 05-entrypoints.md
[Transitions]: 06-transitions.md

## Header

First 64 bytes: magic (`PTKQ`), version (1), CRC32 checksum, section offsets.

```rust
#[repr(C, align(64))]
struct Header {
    magic: [u8; 4],          // b"PTKQ"
    version: u32,            // 1
    checksum: u32,           // CRC32
    total_size: u32,         // Total file size in bytes

    // Section Offsets (Absolute byte offsets)
    str_blob_offset: u32,
    str_table_offset: u32,
    node_types_offset: u32,
    node_fields_offset: u32,
    trivia_offset: u32,
    type_meta_offset: u32,   // Points to TypeMeta header (see 04-types.md)
    entrypoints_offset: u32,
    transitions_offset: u32,

    // Element Counts
    str_table_count: u16,
    node_types_count: u16,
    node_fields_count: u16,
    trivia_count: u16,
    entrypoints_count: u16,
    transitions_count: u16,
    flags: u16,              // Bit 0: linked flag
    _pad: u16,
}
// Size: 16 + 32 + 16 = 64 bytes
//
// Note: TypeMeta sub-section counts are stored in the TypeMeta header,
// not in the main header. See 04-types.md for details.
```

### Flags Field

| Bit | Name    | Description                                              |
| --- | ------- | -------------------------------------------------------- |
| 0   | LINKED  | If set, bytecode contains grammar NodeTypeId/NodeFieldId |

**Linked vs Unlinked Bytecode**:

- **Linked** (`flags & 0x01 != 0`): Match instructions store tree-sitter `NodeTypeId` and `NodeFieldId` in bytes 2-5. Executable directly. NodeTypes and NodeFields sections contain symbol tables for verification.
- **Unlinked** (`flags & 0x01 == 0`): Match instructions store `StringId` references in bytes 2-5 pointing to type/field names in the string table. Requires linking against a grammar before execution. NodeTypes and NodeFields sections are empty.
