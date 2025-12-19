# Binary Format: Symbols

This section defines the symbol tables used to map external Tree-sitter IDs to internal string representations, and to define trivia kinds.

## 1. Node Types

A mapping from Tree-sitter's internal `u16` node type ID to a `StringId` in the query's string table. This allows the runtime to verify node kinds by name or display them for debugging.

- **Section Offset**: `header.node_types_offset`
- **Record Size**: 4 bytes
- **Count**: `header.node_types_count`

```rust
#[repr(C)]
struct NodeSymbol {
    id: u16,        // Tree-sitter Node Type ID
    name: u16,      // StringId
}
```

## 2. Node Fields

A mapping from Tree-sitter's internal `u16` field ID to a `StringId`. Used for field verification during matching.

- **Section Offset**: `header.node_fields_offset`
- **Record Size**: 4 bytes
- **Count**: `header.node_fields_count`

```rust
#[repr(C)]
struct FieldSymbol {
    id: u16,        // Tree-sitter Field ID
    name: u16,      // StringId
}
```

## 3. Trivia

A list of node type IDs that are considered "trivia" (e.g., whitespace, comments). The runtime uses this list when executing navigation commands like `NextSkipTrivia` or `DownSkipTrivia`.

- **Section Offset**: `header.trivia_offset`
- **Record Size**: 2 bytes
- **Count**: `header.trivia_count`

```rust
#[repr(C)]
struct TriviaEntry {
    node_type: u16, // Tree-sitter Node Type ID
}
```

The list is not required to be sorted. Runtimes should build a lookup structure (e.g., bitset indexed by node type) on load for O(1) trivia checks.
