# Bytecode layout

- [Design](#design)
- [Implementation](#implementation)
  - [Header](#header)
  - [Section layout](#section-layout)
  - [Padding and size](#padding-and-size)
  - [Checksum](#checksum)
  - [Offsets and references](#offsets-and-references)

## Design

The bytecode stores a compiled query as a program and the data that program uses. An entry point lets the caller choose a named query to run, such as `Q` in the introduction. It stores the starting instruction and result type. Several entry points can share the same bytecode. The VM runs the chosen entry against a document and its syntax tree.

```text
Bytecode + entry point --+
                         |
Document text -----------+--> VM --> Match results
                         |
Syntax tree -------------+
```

Keeping the document and tree external lets the same bytecode run against different documents. Their trees must use the grammar the query was compiled against. The bytecode stores neither the grammar definition nor its identity, so selecting the correct grammar is the caller's responsibility.

Each kind of data occupies its own section. Tables contain fixed-size records. Strings and compiled regular expressions vary in length, so their bytes live in separate blobs, located through the string and regex tables. The header at the start of the bytecode records each section's size or count.

The design targets 64-byte cache lines. The header occupies one line, and every section starts on a line boundary. The bytecode buffer itself is also aligned to 64 bytes in memory, so these boundaries hold at runtime. Separating sections keeps the instruction stream contiguous, with larger constants and descriptive metadata outside it.

Several instructions can share one table entry. For example, two text tests can refer to the same string:

```text
Text test --+
            |
            +-- string ID --> StringTable --> StringBlob
            |                 byte range      string bytes
Text test --+
```

Each test stores the ID, while the string's bytes are stored once. Compiled regular expressions are shared in the same way.

Type tables describe the shape of the results. A record field or variant case is a member, stored by name and type in `TypeMembers`. Optional inspection spans identify locations in the query text, so an inspector can show which `(identifier)` pattern matched an identifier in the document. A span can also refer to the result type or member that comes from that location. The query text itself is external to the bytecode.

The header records how much data each section contains. Because the sections have a fixed order and alignment, these sizes also determine where each section begins. No separate directory of section offsets is needed. Zero padding fills alignment gaps.

The checksum helps detect accidental changes to the bytes after the header. The reader also validates the header and section contents before the VM uses them. A matching checksum alone cannot establish that the program is valid.

## Implementation

The first 64 bytes are the header. The rest of the bytecode contains section data and zero padding. Byte offsets in this document are measured from the beginning of the bytecode unless a different origin is stated.

Bytecode integers are little-endian. The embedded [regex automata](06-tables.md#sparse-dfa) have their own byte-order check.

### Header

| Byte offset | Width    | Field                    |
| ----------- | -------- | ------------------------ |
| 0           | 4 bytes  | `magic`                  |
| 4           | `u32`    | `version`                |
| 8           | `u32`    | `checksum`               |
| 12          | `u32`    | `total_size`             |
| 16          | `u32`    | `str_blob_size`          |
| 20          | `u32`    | `regex_blob_size`        |
| 24          | `u16`    | `str_table_count`        |
| 26          | `u16`    | `regex_table_count`      |
| 28          | `u16`    | `node_kinds_count`       |
| 30          | `u16`    | `node_fields_count`      |
| 32          | `u16`    | `type_defs_count`        |
| 34          | `u16`    | `type_members_count`     |
| 36          | `u16`    | `type_names_count`       |
| 38          | `u16`    | `entry_points_count`     |
| 40          | `u16`    | `instruction_word_count` |
| 42          | `u16`    | `spans_count`            |
| 44          | 20 bytes | Reserved                 |

`magic` identifies the format with ASCII `PTKQ`, bytes `50 54 4B 51`. `version` and all reserved bytes are zero. `checksum` stores the [body checksum](#checksum).

`total_size` includes the header, section data, and all padding. It must equal the length of the supplied bytes. The other size and count fields describe section data only, excluding padding.

### Section layout

`StringBlob` begins immediately after the header, at byte 64. The remaining sections follow in the order below. Blob sizes count bytes, table counts count records, and `instruction_word_count` counts eight-byte words. An instruction can occupy more than one word.

| Section        | Header field             | Bytes per unit |
| -------------- | ------------------------ | -------------- |
| [StringBlob]   | `str_blob_size`          | 1              |
| [RegexBlob]    | `regex_blob_size`        | 1              |
| [StringTable]  | `str_table_count`        | 4              |
| [RegexTable]   | `regex_table_count`      | 8              |
| [NodeKinds]    | `node_kinds_count`       | 4              |
| [NodeFields]   | `node_fields_count`      | 4              |
| [TypeDefs]     | `type_defs_count`        | 4              |
| [TypeMembers]  | `type_members_count`     | 4              |
| [TypeNames]    | `type_names_count`       | 4              |
| [EntryPoints]  | `entry_points_count`     | 8              |
| [Instructions] | `instruction_word_count` | 8              |
| [Spans]        | `spans_count`            | 16             |

For each section, the header's count and the unit size above determine its data length. `StringTable` and `RegexTable` also contain a final sentinel record, outside the count, that marks the end of their blob. The sentinel has the same size as an ordinary record but no entry ID.

For example, with `str_table_count` set to 3, `StringTable` occupies 16 bytes: three counted records and a sentinel. Entry 0 is one of the counted records. The offsets below are relative to the table's start.

```text
    +-----------+-----------+-----------+-----------+
    | Entry 0   | Entry 1   | Entry 2   | Sentinel  |
    +-----------+-----------+-----------+-----------+
    0           4           8          12          16 bytes
```

The sentinel is present even when the count is zero. In that case, it is the table's only record.

Each section starts at the first multiple of 64 at or after the preceding section's data end. Any intervening bytes are zero padding. These boundaries are measured from the start of the bytecode. An empty section can share its start with the next section and does not reserve an extra block.

```text
64-byte boundary                         Next section start
       |                                        |
       +--------------------------+-------------+-----------------
       | Section data             | Zero bytes  | Next section ...
       +--------------------------+-------------+-----------------
       |<-- section data length ->|<-- 0..63 -->|
```

The loader copies the bytes into immutable storage whose starting address is a multiple of 64. The supplied input need not already have that alignment. Storage capacity can extend beyond `total_size`, but those extra bytes are outside the bytecode.

[StringBlob]: 06-tables.md#strings
[RegexBlob]: 06-tables.md#regexes
[StringTable]: 06-tables.md#strings
[RegexTable]: 06-tables.md#regexes
[NodeKinds]: 06-tables.md#node-kinds-and-fields
[NodeFields]: 06-tables.md#node-kinds-and-fields
[TypeDefs]: 03-types.md
[TypeMembers]: 03-types.md
[TypeNames]: 03-types.md
[EntryPoints]: 02-instructions.md#entry-points
[Instructions]: 02-instructions.md
[Spans]: 05-spans.md

### Padding and size

All padding is zero, including any bytes after the final `Spans` data. Padding belongs to neither a table nor a blob and is excluded from their sizes and counts.

The writer uses only enough final padding to reach a 64-byte boundary. If the data already ends on a boundary, it adds nothing. The reader accepts a wider range: the final tail can have any length, including zero, as long as every byte is zero. The supplied length must still equal `total_size`, so an extra byte outside that declared length is an error.

The reader rejects input shorter than the header, a section whose start or end exceeds `total_size`, and nonzero padding. Bounds calculations must retain the full result rather than truncate or wrap an overflowing integer. These bounds are checked before section data is accessed.

Invalid input produces an error. Before exposing bytecode for execution, the reader also checks each section's contents using its linked specification, including the [control-flow](02-instructions.md#valid-control-flow) and [effect](04-effects.md#valid-effect-sequences) rules.

### Checksum

The checksum covers everything after the header, including the gaps between sections and the final padding. The header is excluded, so its fields need separate validation.

```text
        Header                        Bytecode body
+------------------------+---------------------------------------+
| Excluded from CRC      | Included in CRC, with all padding     |
+------------------------+---------------------------------------+
0                        64                             total_size
```

`checksum` stores CRC-32/ISO-HDLC, also known as IEEE CRC-32, as a little-endian integer:

| Parameter                         | Value        |
| --------------------------------- | ------------ |
| Width                             | 32 bits      |
| Polynomial                        | `0x04C11DB7` |
| Reflected polynomial              | `0xEDB88320` |
| Initial register                  | `0xFFFFFFFF` |
| Input reflection                  | Yes          |
| Output reflection                 | Yes          |
| Final XOR                         | `0xFFFFFFFF` |
| Check value for ASCII `123456789` | `0xCBF43926` |
| Check value for empty input       | `0`          |

The computed CRC must equal `checksum`. A mismatch is an error.

### Offsets and references

Bytecode and blob offsets both count bytes, but start from different places:

| Offset   | Width | Measured from                        |
| -------- | ----- | ------------------------------------ |
| Bytecode | `u32` | First header byte                    |
| Blob     | `u32` | Start of `StringBlob` or `RegexBlob` |

Code addresses count eight-byte words from the start of `Instructions`. They use `u16` and must point to an instruction's first word. Whether zero is a usable address depends on the [instruction operand](02-instructions.md#addresses-and-instruction-boundaries).

Table references select rows by zero-based index:

| Reference    | Width   | Table         |
| ------------ | ------- | ------------- |
| String index | `u16`   | `StringTable` |
| `RegexId`    | `u16`   | `RegexTable`  |
| `TypeId`     | `u16`   | `TypeDefs`    |
| Span ID      | 10 bits | `Spans`       |

A [StringId](06-tables.md#strings) is a nonzero string index used for a required name. An instruction's string comparison can also use index zero. A [RegexId](06-tables.md#regexes) is always nonzero and selects a real regex entry. Neither string nor regex references can select the final sentinel.

The fields below carry indices into `TypeMembers`. Types use them to locate their fields or cases, spans to identify a captured member, and effects to construct that member's value. The effect `RecordSet` attaches a field to the result, and `VariantOpen` begins a variant case. The index's width depends on where it is stored:

| Member reference                            | Width   |
| ------------------------------------------- | ------- |
| `TypeDefs.data` for `Record` or `Variant`   | `u16`   |
| `Spans.member`                              | `u16`   |
| `RecordSet` or `VariantOpen` effect payload | 10 bits |

`TypeDefs.data` locates the first entry in a consecutive group of members, its [member run](03-types.md#members). An empty run may begin just after the table's last row. `Spans.member` uses `0xffff` when the span [identifies no member](05-spans.md#bindings).

An [effect word](04-effects.md#encoding) uses six bits for its opcode and ten for its payload. The payload of `RecordSet` or `VariantOpen` selects the member, so these effects can address only indices 0 through 1023, even if `TypeMembers` contains more entries. Effects that open or close inspection spans use the same payload field for the span ID. The width is fixed by the encoding, not chosen according to the index's value.

A node kind identifies what a node is, such as `identifier`. A grammar field identifies its role in its parent, such as the `name` child of a variable declaration. Both use `u16` IDs from the source-language grammar. The `NodeKinds` and `NodeFields` tables supply names for those IDs, so an ID is not a row number in either table.
