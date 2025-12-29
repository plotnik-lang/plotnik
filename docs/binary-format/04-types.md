# Binary Format: Type Metadata

This section defines the type system metadata used for code generation and runtime validation. It allows consumers to understand the shape of the data extracted by the query.

## 1. Primitives

**TypeId (u16)**: Index into the Type Definition table.

- `0`: `Void` (Captures nothing)
- `1`: `Node` (AST Node reference)
- `2`: `String` (Source text)
- `3..N`: Composite types (Index = `TypeId - 3`)

### Node Semantics

`TYPE_NODE` (1) represents a platform-dependent handle to a tree-sitter AST node:

| Context    | Representation                                             |
| :--------- | :--------------------------------------------------------- |
| Rust       | `tree_sitter::Node<'tree>` (lifetime-bound reference)      |
| TypeScript | Binding-provided object with `startPosition`, `text`, etc. |
| JSON       | Unique node identifier (e.g., `"node:42"` or path-based)   |

The handle provides access to node metadata (kind, span, text) without copying the source. Lifetime management is platform-specific—Rust enforces it statically, bindings may use reference counting or arena allocation.

**TypeKind (u8)**: Discriminator for `TypeDef`.

- `0`: `Optional` (Wraps another type)
- `1`: `ArrayStar` (Zero or more)
- `2`: `ArrayPlus` (One or more)
- `3`: `Struct` (Record with named fields)
- `4`: `Enum` (Discriminated union)
- `5`: `Alias` (Named reference to another type, e.g., `@x :: Identifier`)

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

| Kind        | `data` (u16)   | `count` (u8)   | Interpretation        |
| :---------- | :------------- | :------------- | :-------------------- |
| `Optional`  | `InnerTypeId`  | 0              | Wrapper `T?`          |
| `ArrayStar` | `InnerTypeId`  | 0              | Wrapper `T[]`         |
| `ArrayPlus` | `InnerTypeId`  | 0              | Wrapper `[T, ...T[]]` |
| `Struct`    | `MemberIndex`  | `FieldCount`   | Record with fields    |
| `Enum`      | `MemberIndex`  | `VariantCount` | Discriminated union   |
| `Alias`     | `TargetTypeId` | 0              | Named type reference  |

> **Note**: The interpretation of `data` depends on `kind`. For wrappers and `Alias`, it's a `TypeId`. For `Struct` and `Enum`, it's an index into the TypeMembers section. Parsers must dispatch on `kind` first.

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

### 3.1. Simple Struct

Query: `Q = (function name: (identifier) @name)`

```text
Strings: ["name", "Q"]
          Str#0   Str#1

TypeDefs:
  T3: Struct { data=0, count=1, kind=Struct }

TypeMembers:
  [0]: name=Str#0 ("name"), ty=1 (Node)

TypeNames:
  [0]: name=Str#1 ("Q"), type_id=T3
```

### 3.2. Recursive Enum

Query:

```
List = [
    Nil: (nil)
    Cons: (cons (a) @head (List) @tail)
]
```

```text
Strings: ["List", "Nil", "Cons", "head", "tail"]
          Str#0   Str#1  Str#2   Str#3   Str#4

TypeDefs:
  T3: Enum { data=0, count=2, kind=Enum }
  T4: Struct { data=2, count=2, kind=Struct }  // Cons payload (anonymous)

TypeMembers:
  [0]: name=Str#1 ("Nil"),  ty=0 (Void)        // unit variant
  [1]: name=Str#2 ("Cons"), ty=T4              // payload is struct
  [2]: name=Str#3 ("head"), ty=1 (Node)
  [3]: name=Str#4 ("tail"), ty=T3              // self-reference

TypeNames:
  [0]: name=Str#0 ("List"), type_id=T3
```

The `tail` field's type (`T3`) points back to the `List` enum. Recursive types are naturally representable since everything is indexed.

### 3.3. Custom Type Annotation

Query: `Q = (identifier) @name :: Identifier`

```text
Strings: ["Identifier", "name", "Q"]
          Str#0         Str#1   Str#2

TypeDefs:
  T3: Alias { data=1 (Node), count=0, kind=Alias }
  T4: Struct { data=0, count=1, kind=Struct }

TypeMembers:
  [0]: name=Str#1 ("name"), ty=T3 (Identifier alias)

TypeNames:
  [0]: name=Str#0 ("Identifier"), type_id=T3
  [1]: name=Str#2 ("Q"), type_id=T4
```

The `Alias` type creates a distinct TypeId so the emitter can render `Identifier` instead of `Node`.

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
