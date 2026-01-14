# Binary Format: Symbols

Symbol tables map external tree-sitter IDs to internal string names.

## 1. Regex

Precompiled DFA patterns for predicate matching. Uses the sentinel pattern like StringTable.

### RegexBlob

- **Section Offset**: Computed (follows StringBlob)
- **Size**: `header.regex_blob_size`

Contains concatenated serialized DFAs (from `regex-automata`). Each DFA is deserialized via `DFA::from_bytes()` for O(1) loading.

### RegexTable

- **Section Offset**: Computed (follows StringTable)
- **Record Size**: 4 bytes (`u32`)
- **Count**: `header.regex_table_count + 1`

Each entry is a byte offset into RegexBlob. The final entry is the blob size.

To retrieve regex `i`:
1. `start = table[i]`
2. `end = table[i+1]`
3. `bytes = blob[start..end]`

## 2. Node Types

Maps tree-sitter node type IDs to their string names.

- **Section Offset**: Computed (follows RegexTable)
- **Record Size**: 4 bytes
- **Count**: `header.node_types_count`

```rust
#[repr(C)]
struct NodeSymbol {
    id: u16,        // Tree-sitter node type ID
    name: u16,      // StringId
}
```

In **linked** bytecode, this table enables name lookup for debugging and error messages. In **unlinked** bytecode, this section is empty.

## 3. Node Fields

Maps tree-sitter field IDs to their string names.

- **Section Offset**: Computed (follows NodeTypes)
- **Record Size**: 4 bytes
- **Count**: `header.node_fields_count`

```rust
#[repr(C)]
struct FieldSymbol {
    id: u16,        // Tree-sitter field ID
    name: u16,      // StringId
}
```

## 4. Trivia

Node types considered "trivia" (whitespace, comments). The runtime skips these during navigation with `NextSkip`, `DownSkip`, etc.

- **Section Offset**: Computed (follows NodeFields)
- **Record Size**: 2 bytes
- **Count**: `header.trivia_count`

```rust
#[repr(C)]
struct TriviaEntry {
    node_type: u16, // Tree-sitter node type ID
}
```

Unsorted. Loaders should build a lookup structure (e.g., bitset indexed by node type) for O(1) trivia checks.
