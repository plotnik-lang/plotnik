# Binary Format: Entry Points

This section defines the named entry points for the query. Only definitions
whose root matches exactly one node are entry points and can be executed against
a syntax tree. Sequence- and quantifier-rooted definitions are fragments: they
may still contribute type metadata and be referenced by entry points, but they
do not appear in this table.

## Layout

- **Section Offset**: Computed (follows TypeNames)
- **Record Size**: 8 bytes
- **Count**: `header.entry_points_count`
- **Ordering**: Definition order, after filtering to selectable definitions. This
  order is also the CLI defaulting order: without `--entry`, the last entry is
  selected.

## Definition

```rust
#[repr(C)]
struct EntryPoint {
    name: u16,          // StringId
    target: u16,        // CodeAddr (into Instructions section)
    result_type: u16,   // TypeId
    _pad: u16,          // Padding to 8 bytes
}
```

### Fields

- **name**: The selectable definition name (e.g., "Func", "Class"). `StringId`.
- **target**: The instruction address (`CodeAddr`) where execution begins for
  this definition. It points at the definition's entry-point wrapper in the
  **Instructions** section.
- **result_type**: The `TypeId` of the structure produced by this query definition.
- **\_pad**: Reserved for alignment. Must be zero; loaders reject a non-zero pad.

### Usage

When the user runs a query with a specific entry point (e.g., `--entry Func`), the runtime:

1. Scans the entry-points table, resolving each `name` ID to string content for comparison.
2. Sets the initial instruction pointer (`IP`) to `target`.
3. Executes the wrapper and definition body in the VM.
4. Validates that the resulting value matches `result_type`.
