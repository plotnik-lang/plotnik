# ADR-0004: Query IR Binary Format

- **Status**: Accepted
- **Date**: 2025-12-12
- **Supersedes**: Parts of [ADR-0003](ADR-0003-query-intermediate-representation.md)

## Context

The Query IR lives in a single contiguous allocation—cache-friendly, zero fragmentation, portable to WASM. This ADR defines the binary layout. Graph structures are in [ADR-0005](ADR-0005-transition-graph-format.md).

## Decision

### Container

```rust
struct QueryIR {
    data: Arena,
    successors_offset: u32,
    effects_offset: u32,
    negated_fields_offset: u32,
    string_refs_offset: u32,
    string_bytes_offset: u32,
    entrypoints_offset: u32,
    default_entrypoint: TransitionId,
}
```

Transitions start at offset 0 (implicit).

### Arena

```rust
const ARENA_ALIGN: usize = 4;

struct Arena {
    ptr: *mut u8,
    len: usize,
}
```

Allocated via `Layout::from_size_align(len, ARENA_ALIGN)`. Standard `Box<[u8]>` won't work—it assumes 1-byte alignment and corrupts `dealloc`.

### Segments

| Segment        | Type                | Offset                  | Align |
| -------------- | ------------------- | ----------------------- | ----- |
| Transitions    | `[Transition; N]`   | 0                       | 4     |
| Successors     | `[TransitionId; M]` | `successors_offset`     | 4     |
| Effects        | `[EffectOp; P]`     | `effects_offset`        | 2     |
| Negated Fields | `[NodeFieldId; Q]`  | `negated_fields_offset` | 2     |
| String Refs    | `[StringRef; R]`    | `string_refs_offset`    | 4     |
| String Bytes   | `[u8; S]`           | `string_bytes_offset`   | 1     |
| Entrypoints    | `[Entrypoint; T]`   | `entrypoints_offset`    | 4     |

Each offset is aligned: `(offset + align - 1) & !(align - 1)`.

### Strings

Single pool for all strings (field names, variant tags, entrypoint names):

```rust
#[repr(C)]
struct StringRef {
    offset: u32,  // into string_bytes
    len: u16,
    _pad: u16,
}

#[repr(C)]
struct Entrypoint {
    name_id: u16,  // into string_refs
    _pad: u16,
    target: TransitionId,
}
```

`DataFieldId(u16)` and `VariantTagId(u16)` index into `string_refs`. Distinct types, same table.

Strings are interned during construction—identical strings share storage and ID.

### Serialization

```
Header (20 bytes):
  magic: [u8; 4]       b"PLNK"
  version: u32         format version + ABI hash
  checksum: u32        CRC32(segment_offsets || arena_data)
  arena_len: u32
  segment_count: u32

Segment Offsets (segment_count × 4 bytes)
Arena Data (arena_len bytes)
```

Little-endian always. UTF-8 strings. Version mismatch or checksum failure → recompile.

### Construction

Three passes:

1. **Analysis**: Count elements, intern strings
2. **Layout**: Compute aligned offsets, allocate once
3. **Emission**: Write via `ptr::write`

No `realloc`.

### Example

Query:

```
Func = (function_declaration name: (identifier) @name)
Expr = [ Ident: (identifier) @name  Num: (number) @value ]
```

Arena layout:

```
0x0000  Transitions    [T0, T1, T2, ...]
0x0180  Successors     [1, 2, 3, ...]
0x0200  Effects        [StartObject, Field(0), ...]
0x0280  Negated Fields []
0x0280  String Refs    [{0,4}, {4,5}, {9,5}, ...]
0x02C0  String Bytes   "namevalueIdentNum FuncExpr"
0x0300  Entrypoints    [{4, T0}, {5, T3}]
```

`"name"` stored once, used by both `@name` captures.

## Consequences

**Positive**: Cache-efficient, O(1) string lookup, zero-copy access, simple validation.

**Negative**: Format changes require rebuild. No version migration.

**WASM**: Explicit alignment prevents traps. `u32` offsets fit WASM32.

## References

- [ADR-0005: Transition Graph Format](ADR-0005-transition-graph-format.md)
- [ADR-0006: Dynamic Query Execution](ADR-0006-dynamic-query-execution.md)
