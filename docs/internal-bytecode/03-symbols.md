# Internal Bytecode: Symbols

Symbol tables map external tree-sitter IDs to internal string names.

## 1. Regex

Precompiled DFA patterns for predicate matching. Uses the sentinel pattern like StringTable.

### RegexBlob

- **Section Offset**: Computed (follows StringBlob)
- **Size**: `header.regex_blob_size`

Contains concatenated serialized DFAs (from `regex-automata`). Internal
construction deserializes each DFA once via `DFA::from_bytes()`, validates it,
and caches the owned automaton for predicate evaluation (issue #426).

### RegexTable

- **Section Offset**: Computed (follows StringTable)
- **Record Size**: 8 bytes
- **Count**: `header.regex_table_count + 1`

Each entry stores both a StringId (for pattern display) and offset into RegexBlob (for DFA access):

```rust
#[repr(C)]
struct RegexEntry {
    string_id: u16,     // StringId of pattern text (for dump/trace)
    reserved: u16,      // Reserved for future use; must be zero, rejected at load
    offset: u32,        // Byte offset into RegexBlob
}
```

The final entry is a sentinel with `string_id = 0` and `offset = blob_size`.

To retrieve regex `i`:

1. `pattern_id = table[i].string_id` → look up in StringTable for display
2. `start = table[i].offset`
3. `end = table[i+1].offset`
4. `dfa_bytes = blob[start..end]`

## 2. Node Kinds

Maps tree-sitter node kind IDs to their string names.

- **Section Offset**: Computed (follows RegexTable)
- **Record Size**: 4 bytes
- **Count**: `header.node_kinds_count`

```rust
#[repr(C)]
struct SymbolNameEntry {
    symbol: u16,    // Tree-sitter node kind ID
    name: u16,      // StringId
}
```

This table enables name lookup for debugging and error messages.

## 3. Node Fields

Maps tree-sitter field IDs to their string names.

- **Section Offset**: Computed (follows NodeKinds)
- **Record Size**: 4 bytes
- **Count**: `header.node_fields_count`

```rust
#[repr(C)]
struct SymbolNameEntry {
    symbol: u16,    // Tree-sitter field ID
    name: u16,      // StringId
}
```
