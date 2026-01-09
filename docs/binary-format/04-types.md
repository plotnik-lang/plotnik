# Binary Format: Type Metadata

This section defines the type system metadata used for code generation and runtime validation. It allows consumers to understand the shape of the data extracted by the query.

## 1. Primitives

**TypeId (u16)**: Zero-based index into the TypeDefs array. All types, including primitives, are stored as TypeDef entries.

### Node Semantics

The `Node` type (`TypeKind = 1`) represents a platform-dependent handle to a tree-sitter AST node:

| Context    | Representation                                             |
| :--------- | :--------------------------------------------------------- |
| Rust       | `tree_sitter::Node<'tree>` (lifetime-bound reference)      |
| TypeScript | Binding-provided object with `startPosition`, `text`, etc. |
| JSON       | Unique node identifier (e.g., `"node:42"` or path-based)   |

The handle provides access to node metadata (kind, span, text) without copying the source. Lifetime management is platform-specific — Rust enforces it statically, bindings may use reference counting or arena allocation.

**TypeKind (u8)**: Discriminator for `TypeDef`.

- `0`: `Void` (Unit type, captures nothing)
- `1`: `Node` (AST node reference)
- `2`: `String` (Source text)
- `3`: `Optional` (Wraps another type)
- `4`: `ArrayZeroOrMore` (Zero or more, aka ArrayStar)
- `5`: `ArrayOneOrMore` (One or more, aka ArrayPlus)
- `6`: `Struct` (Record with named fields)
- `7`: `Enum` (Discriminated union)
- `8`: `Alias` (Named reference to another type, e.g., `@x :: Identifier`)

## 2. Layout

The TypeMeta section begins with an 8-byte header containing sub-section counts, followed by three 64-byte aligned sub-sections:

```
type_meta_offset
│
├─ TypeMetaHeader (8 bytes)
│    type_defs_count: u16
│    type_members_count: u16
│    type_names_count: u16
│    _pad: u16
│
├─ [padding to 64-byte boundary]
│
├─ TypeDefs[type_defs_count] (4 bytes each)
│
├─ [padding to 64-byte boundary]
│
├─ TypeMembers[type_members_count] (4 bytes each)
│
├─ [padding to 64-byte boundary]
│
└─ TypeNames[type_names_count] (4 bytes each)
```

```rust
#[repr(C)]
struct TypeMetaHeader {
    type_defs_count: u16,
    type_members_count: u16,
    type_names_count: u16,
    _pad: u16,
}
```

**Sub-section offsets** (each aligned to 64-byte boundary):

- TypeDefs: `align64(type_meta_offset + 8)`
- TypeMembers: `align64(TypeDefs_offset + type_defs_count * 4)`
- TypeNames: `align64(TypeMembers_offset + type_members_count * 4)`

This separation ensures:

- No wasted space (anonymous types don't need name storage)
- Clean concerns (structure vs. naming)
- Uniform 4-byte records within each sub-section
- 64-byte alignment for cache-friendly access

### 2.1. TypeDef (4 bytes)

Describes the structure of a single type.

```rust
#[repr(C)]
struct TypeDef {
    data: u16,      // TypeId OR MemberIndex (depends on kind)
    count: u8,      // Member count (0 for wrappers/alias)
    kind: u8,       // TypeKind
}
```

**Semantics of `data` and `count` fields**:

| Kind              | `data` (u16)   | `count` (u8)   | Interpretation        |
| :---------------- | :------------- | :------------- | :-------------------- |
| `Void`            | 0              | 0              | Unit type             |
| `Node`            | 0              | 0              | AST node reference    |
| `String`          | 0              | 0              | Source text           |
| `Optional`        | `InnerTypeId`  | 0              | Wrapper `T?`          |
| `ArrayZeroOrMore` | `InnerTypeId`  | 0              | Wrapper `T[]`         |
| `ArrayOneOrMore`  | `InnerTypeId`  | 0              | Wrapper `[T, ...T[]]` |
| `Struct`          | `MemberIndex`  | `FieldCount`   | Record with fields    |
| `Enum`            | `MemberIndex`  | `VariantCount` | Discriminated union   |
| `Alias`           | `TargetTypeId` | 0              | Named type reference  |

> **Note**: For primitives (`Void`, `Node`, `String`), `data` and `count` are unused. For wrappers and `Alias`, `data` is a `TypeId`. For `Struct` and `Enum`, `data` is an index into the TypeMembers section. Parsers must dispatch on `kind` first.

> **Limit**: `count` is u8, so structs/enums are limited to 255 members.

### 2.2. TypeMember (4 bytes)

Describes a field in a struct or a variant in an enum.

```rust
#[repr(C)]
struct TypeMember {
    name: u16,      // StringId (field or variant name)
    ty: u16,        // TypeId (field type or variant payload)
}
```

For struct fields: `name` is the field name, `ty` is the field's type.
For enum variants: `name` is the variant tag, `ty` is the payload type (use `Void` for unit variants).

### 2.3. TypeName (4 bytes)

Maps a name to a type. Only types that have names appear here.

```rust
#[repr(C)]
struct TypeName {
    name: u16,      // StringId
    type_id: u16,   // TypeId
}
```

**Ordering**: Entries are sorted lexicographically by name (resolved via String Table) for binary search.

**Usage**:

- Named definitions (`List = [...]`) get an entry mapping "List" to their TypeId
- Custom type annotations (`@x :: Identifier`) create an Alias TypeDef, with an entry here
- Anonymous types (inline structs, wrappers) have no entry

For code generation, build a reverse map (`TypeId → Option<StringId>`) to look up names when emitting types.

## 3. Examples

> **Note**: In bytecode, only **used** primitives are emitted to TypeDefs. The emitter writes them first in order (Void, Node, String), then composite types. TypeId values depend on which primitives the query actually uses.

### 3.1. Simple Struct

Query: `Q = (function name: (identifier) @name)`

Run `plotnik dump -q '<query>'` to see:

```
[type_defs]
T0 = <Node>
T1 = Struct  M0:1  ; { name }

[type_members]
M0: S1 → T0  ; name: <Node>

[type_names]
N0: S2 → T1  ; Q
```

- `T0` is the `Node` primitive (only used primitive is emitted)
- `T1` is a `Struct` with 1 member starting at `M0`
- `M0` maps "name" to type `T0` (Node)

### 3.2. Recursive Enum

Query:

```
List = [
    Nil: (nil)
    Cons: (cons (a) @head (List) @tail)
]
```

Run `plotnik dump -q '<query>'` to see:

```
[type_defs]
T0 = <Void>
T1 = <Node>
T2 = Struct  M0:2  ; { head, tail }
T3 = Enum    M2:2  ; Nil | Cons

[type_members]
M0: S1 → T1  ; head: <Node>
M1: S2 → T3  ; tail: List
M2: S3 → T0  ; Nil: <Void>
M3: S4 → T2  ; Cons: T2

[type_names]
N0: S5 → T3  ; List
```

- `T0` (Void) and `T1` (Node) are primitives used by the query
- `T2` is the Cons payload struct with `head` and `tail` fields
- `T3` is the `List` enum with `Nil` and `Cons` variants
- `M1` shows `tail: List` — self-reference to `T3`

### 3.3. Custom Type Annotation

Query: `Q = (identifier) @name :: Identifier`

Run `plotnik dump -q '<query>'` to see:

```
[type_defs]
T0 = <Node>
T1 = Alias(T0)
T2 = Struct  M0:1  ; { name }

[type_members]
M0: S2 → T1  ; name: Identifier

[type_names]
N0: S1 → T1  ; Identifier
N1: S3 → T2  ; Q
```

- `T0` is the underlying `Node` primitive
- `T1` is an `Alias` pointing to `T0`, named "Identifier"
- The `name` field has type `T1` (the alias), so code generators emit `Identifier` instead of `Node`

## 4. Validation

Loaders must verify for `Struct`/`Enum` kinds:

- `(data as u32) + (count as u32) ≤ type_members_count`

This prevents out-of-bounds reads from malformed binaries.

## 5. Code Generation

To emit types (TypeScript, Rust, etc.):

1. Build reverse map: `TypeId → Option<StringId>` from TypeNames
2. Start from entrypoints or iterate TypeNames
3. For each type:
   - Look up structure in TypeDefs
   - Look up name (if any) in reverse map
   - Emit named types with their name; anonymous types inline or with generated names
4. Detect when multiple names map to the same TypeId → emit aliases
