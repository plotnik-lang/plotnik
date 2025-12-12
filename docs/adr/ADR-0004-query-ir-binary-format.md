# ADR-0004: Query IR Binary Format

- **Status**: Accepted
- **Date**: 2025-12-12
- **Supersedes**: Parts of ADR-0003

## Context

The Query IR lives in a single contiguous allocation—cache-friendly, zero fragmentation, portable to WASM. This ADR defines the binary layout. Graph structures are in [ADR-0005](ADR-0005-transition-graph-format.md). Type metadata is in [ADR-0007](ADR-0007-type-metadata-format.md).

## Decision

### Container

```rust
struct QueryIR {
    ir_buffer: QueryIRBuffer,
    successors_offset: u32,
    effects_offset: u32,
    negated_fields_offset: u32,
    string_refs_offset: u32,
    string_bytes_offset: u32,
    type_defs_offset: u32,
    type_members_offset: u32,
    entrypoints_offset: u32,
}
```

Transitions start at offset 0. Default entrypoint is always at offset 0.

### QueryIRBuffer

```rust
const BUFFER_ALIGN: usize = 64;  // cache-line alignment for transitions

struct QueryIRBuffer {
    ptr: *mut u8,
    len: usize,
}
```

Allocated via `Layout::from_size_align(len, BUFFER_ALIGN)`. Standard `Box<[u8]>` won't work—it assumes 1-byte alignment and corrupts `dealloc`. The 64-byte alignment ensures transitions never straddle cache lines.

**Deallocation**: `QueryIRBuffer` must implement `Drop` to reconstruct the exact `Layout` (size + 64-byte alignment) and call `std::alloc::dealloc`. Using `Box::from_raw` or similar would assume align=1 and cause undefined behavior.

### Segments

| Segment        | Type                | Offset                  | Align |
| -------------- | ------------------- | ----------------------- | ----- |
| Transitions    | `[Transition; N]`   | 0                       | 64    |
| Successors     | `[TransitionId; M]` | `successors_offset`     | 4     |
| Effects        | `[EffectOp; P]`     | `effects_offset`        | 2     |
| Negated Fields | `[NodeFieldId; Q]`  | `negated_fields_offset` | 2     |
| String Refs    | `[StringRef; R]`    | `string_refs_offset`    | 4     |
| String Bytes   | `[u8; S]`           | `string_bytes_offset`   | 1     |
| Type Defs      | `[TypeDef; T]`      | `type_defs_offset`      | 4     |
| Type Members   | `[TypeMember; U]`   | `type_members_offset`   | 2     |
| Entrypoints    | `[Entrypoint; V]`   | `entrypoints_offset`    | 4     |

Each offset is aligned: `(offset + align - 1) & !(align - 1)`.

For `Transition`, `EffectOp` see [ADR-0005](ADR-0005-transition-graph-format.md). For `TypeDef`, `TypeMember` see [ADR-0007](ADR-0007-type-metadata-format.md).

### Strings

Single pool for all strings (field names, variant tags, entrypoint names, type names):

```rust
type StringId = u16;

#[repr(C)]
struct StringRef {
    offset: u32,  // into string_bytes
    len: u16,
    _pad: u16,
}
// 8 bytes, align 4

type DataFieldId = StringId;   // field names in effects
type VariantTagId = StringId;  // variant tags in effects

type TypeId = u16;  // see ADR-0007 for semantics
```

`StringId` indexes into `string_refs`. `DataFieldId` and `VariantTagId` are aliases for type safety. `TypeId` indexes into type_defs (with reserved primitives 0-2).

Strings are interned during construction—identical strings share storage and ID.

### Entrypoints

```rust
#[repr(C)]
struct Entrypoint {
    name_id: StringId,      // 2
    _pad: u16,              // 2
    target: TransitionId,   // 4
    result_type: TypeId,    // 2 - see ADR-0007
    _pad2: u16,             // 2
}
// 12 bytes, align 4
```

### Serialization

```
Header (48 bytes):
  magic: [u8; 4]           b"PLNK"
  version: u32             format version + ABI hash
  checksum: u32            CRC32(offsets || buffer_data)
  buffer_len: u32
  successors_offset: u32
  effects_offset: u32
  negated_fields_offset: u32
  string_refs_offset: u32
  string_bytes_offset: u32
  type_defs_offset: u32
  type_members_offset: u32
  entrypoints_offset: u32

Buffer Data (buffer_len bytes)
```

Little-endian always. UTF-8 strings. Version mismatch or checksum failure → recompile.

### Construction

Three passes:

1. **Analysis**: Count elements, intern strings, infer types
2. **Layout**: Compute aligned offsets, allocate once
3. **Emission**: Write via `ptr::write`

No `realloc`.

### Example

Query:

```
Func = (function_declaration name: (identifier) @name)
Expr = [ Ident: (identifier) @name  Num: (number) @value ]
```

Buffer layout:

```
0x0000  Transitions    [T0, T1, T2, ...]
0x0180  Successors     [1, 2, 3, ...]
0x0200  Effects        [StartObject, Field(0), ...]
0x0280  Negated Fields []
0x0280  String Refs    [{0,4}, {4,5}, {9,5}, ...]
0x02C0  String Bytes   "namevalueIdentNumFuncExpr"
0x0300  Type Defs      [Record{...}, Enum{...}, ...]
0x0340  Type Members   [{name,Str}, {Ident,Ty5}, ...]
0x0380  Entrypoints    [{name=Func, target=Tr0, type=Ty3}, ...]
```

`"name"` stored once, used by both `@name` captures.

## Consequences

**Positive**: Cache-efficient, O(1) string lookup, zero-copy access, simple validation. Self-contained binaries enable query caching by input hash.

**Negative**: Format changes require rebuild. No version migration.

**WASM**: Explicit alignment prevents traps. `u32` offsets fit WASM32.

## References

- [ADR-0005: Transition Graph Format](ADR-0005-transition-graph-format.md)
- [ADR-0006: Dynamic Query Execution](ADR-0006-dynamic-query-execution.md)
- [ADR-0007: Type Metadata Format](ADR-0007-type-metadata-format.md)
