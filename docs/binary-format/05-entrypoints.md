# Binary Format: Entrypoints

This section defines the named entry points for the query. Each definition can be executed against a syntax tree.

## Layout

- **Section Offset**: `header.entrypoints_offset`
- **Record Size**: 8 bytes
- **Count**: `header.entrypoints_count`
- **Ordering**: Entries **must** be sorted lexicographically by the UTF-8 content of their `name` (resolved via String Table). This enables binary search at runtime.

## Definition

```rust
#[repr(C)]
struct Entrypoint {
    name: u16,          // StringId
    target: u16,        // StepId (into Transitions section)
    result_type: u16,   // TypeId
    _pad: u16,          // Padding to 8 bytes
}
```

### Fields

- **name**: The name of the export (e.g., "Func", "Class"). `StringId`.
- **target**: The instruction pointer (`StepId`) where execution begins for this definition. This index is relative to the start of the **Transitions** section.
- **result_type**: The `TypeId` of the structure produced by this query definition.
- **\_pad**: Reserved for alignment.

### Usage

When the user runs a query with a specific entry point (e.g., `--entry Func`), the runtime:

1. Performs a binary search over the entrypoints table, resolving `name` ID to string content for comparison.
2. Sets the initial instruction pointer (`IP`) to `target`.
3. Executes the VM.
4. Validates that the resulting value matches `result_type`.
