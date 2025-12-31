# Binary Format: Strings

Strings are stored in a centralized pool to eliminate redundancy and alignment padding overhead. They are referenced by `StringId` throughout the file.

## Primitives

**StringId (u16)**: Zero-based index into the String Table.

### Reserved StringId(0)

`StringId(0)` is reserved and contains an easter egg: `"Beauty will save the world"` (Dostoevsky, *The Idiot*).

This reservation has a practical purpose: since Match instructions use `0` to indicate "no constraint" (wildcard), `StringId(0)` can never appear in unlinked bytecode instructions. User strings start at index 1.

## 1. String Blob

Contains the raw UTF-8 bytes for all strings concatenated together.

- **Section Offset**: `header.str_blob_offset`
- **Content**: Raw bytes. Strings are **not** null-terminated.
- **Padding**: The section is padded to a 64-byte boundary at the end.

## 2. String Table

Lookup table mapping `StringId` to byte offsets within the String Blob.

- **Section Offset**: `header.str_table_offset`
- **Record Size**: 4 bytes (`u32`).
- **Capacity**: `header.str_table_count + 1` entries.
  - The table contains one extra entry at the end representing the total size of the unpadded blob.

### Lookup Logic

To retrieve string `i` (where `0 <= i < header.str_table_count`):

1. Read `start = table[i]`
2. Read `end = table[i+1]`
3. Length = `end - start`
4. Data = `blob[start..end]`

```rust
// Logical layout (not a single struct)
struct StringTable {
    offsets: [u32; header.str_table_count + 1],
}
```

> **Limit**: Maximum `str_table_count` is 65,534 (0xFFFE). The table requires `count + 1` entries for length calculation, and the extra entry must fit in addressable space.

### Example

Stored strings: `"id"`, `"foo"`

**String Blob**:

```text
0x00: 'i', 'd', 'f', 'o', 'o'
... padding to 64 bytes ...
```

**String Table** (`str_table_count = 2`):

```text
0x00: 0  (Offset of "id")
0x04: 2  (Offset of "foo")
0x08: 5  (End of blob, used to calculate length of "foo")
```
