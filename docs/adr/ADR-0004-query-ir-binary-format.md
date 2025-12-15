# ADR-0004: Compiled Query Binary Format

- **Status**: Accepted
- **Date**: 2024-12-12
- **Supersedes**: Parts of ADR-0003

## Context

The compiled query lives in a single contiguous allocation—cache-friendly, zero fragmentation, portable to WASM. This ADR defines the binary layout. Graph structures are in [ADR-0005](ADR-0005-transition-graph-format.md). Type metadata is in [ADR-0007](ADR-0007-type-metadata-format.md).

## Decision

### Container

```rust
struct CompiledQuery {
    buffer: CompiledQueryBuffer,
    successors_offset: u32,
    effects_offset: u32,
    negated_fields_offset: u32,
    string_refs_offset: u32,
    string_bytes_offset: u32,
    type_defs_offset: u32,
    type_members_offset: u32,
    entrypoints_offset: u32,
    trivia_kinds_offset: u32,  // 0 = no trivia kinds
}
```

Transitions start at buffer offset 0. The default entrypoint is **Transition 0** (the root of the graph). The `entrypoints` table provides named exports for multi-definition queries; it does not affect the default entrypoint.

### CompiledQueryBuffer

```rust
const BUFFER_ALIGN: usize = 64;  // cache-line alignment for transitions

struct CompiledQueryBuffer {
    ptr: *mut u8,
    len: usize,
    owned: bool,  // true if allocated, false if mmap'd
}
```

Allocated via `Layout::from_size_align(len, BUFFER_ALIGN)`. Standard `Box<[u8]>` won't work—it assumes 1-byte alignment and corrupts `dealloc`. The 64-byte alignment ensures transitions never straddle cache lines.

**Ownership semantics**:

| `owned` | Source              | `Drop` action                                    |
| ------- | ------------------- | ------------------------------------------------ |
| `true`  | `std::alloc::alloc` | Reconstruct `Layout`, call `std::alloc::dealloc` |
| `false` | `mmap` / external   | No-op (caller manages lifetime)                  |

For mmap'd queries, the OS maps file pages directly into address space. The 64-byte header ensures buffer data starts aligned. `CompiledQueryBuffer` with `owned: false` provides a view without taking ownership—the backing file mapping must outlive the `CompiledQuery`.

**Deallocation**: When `owned: true`, `Drop` must reconstruct the exact `Layout` (size + 64-byte alignment) and call `std::alloc::dealloc`. Using `Box::from_raw` or similar would assume align=1 and cause undefined behavior.

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
| Trivia Kinds   | `[NodeTypeId; W]`   | `trivia_kinds_offset`   | 2     |

Each offset is aligned: `(offset + align - 1) & !(align - 1)`.

For `Transition`, `EffectOp` see [ADR-0005](ADR-0005-transition-graph-format.md). For `TypeDef`, `TypeMember` see [ADR-0007](ADR-0007-type-metadata-format.md).

### Strings

Single pool for all strings (field names, variant tags, entrypoint names, type names):

```rust
type StringId = u16;
const STRING_NONE: StringId = 0xFFFF;  // sentinel for unnamed types

#[repr(C)]
struct StringRef {
    offset: u32,  // byte offset into string_bytes (NOT element index)
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
Header (64 bytes):
  magic: [u8; 4]           b"PLNK"
  version: u32             format version + ABI hash
  checksum: u32            CRC32(header[12..64] || buffer_data)
  buffer_len: u32
  successors_offset: u32
  effects_offset: u32
  negated_fields_offset: u32
  string_refs_offset: u32
  string_bytes_offset: u32
  type_defs_offset: u32
  type_members_offset: u32
  entrypoints_offset: u32
  trivia_kinds_offset: u32
  _pad: [u8; 12]           reserved, zero-filled

Buffer Data (buffer_len bytes)
```

Header is 64 bytes to ensure buffer data starts at a 64-byte aligned offset. This enables true zero-copy `mmap` usage where transitions at offset 0 within the buffer are correctly aligned.

Little-endian always. UTF-8 strings. Version mismatch or checksum failure → recompile.

**Checksum coverage**: The checksum covers bytes 12–63 of the header (everything after the checksum field) plus all buffer data. The magic and version are verified independently before checksum validation—a version mismatch triggers recompile without checking the checksum.

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
0x0300  Type Defs      [Struct{...}, Enum{...}, ...]
0x0340  Type Members   [{name,Str}, {Ident,Ty5}, ...]
0x0380  Entrypoints    [{name=Func, target=Tr0, type=Ty3}, ...]
0x03A0  Trivia Kinds   [comment, ...]
```

`"name"` stored once, used by both `@name` captures.

## Consequences

**Positive**: Cache-efficient, O(1) string access, zero-copy access, simple validation. Self-contained binaries enable query caching by input hash.

**Negative**: Format changes require rebuild. No version migration.

**WASM**: Explicit alignment prevents traps. `u32` offsets fit WASM32.

## References

- [ADR-0005: Transition Graph Format](ADR-0005-transition-graph-format.md)
- [ADR-0006: Dynamic Query Execution](ADR-0006-dynamic-query-execution.md)
- [ADR-0007: Type Metadata Format](ADR-0007-type-metadata-format.md)
