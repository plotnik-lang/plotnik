# Binary Format: Type Metadata

This section defines the type system metadata used for code generation and runtime validation. It allows consumers to understand the shape of the data extracted by the query.

## 1. Primitives

**TypeId (u16)**: Index into the Type Definition table.

- `0`: `Void` (Captures nothing)
- `1`: `Node` (AST Node reference)
- `2`: `String` (Source text)
- `3..N`: Composite types (Index = `TypeId - 3`)
- `0xFFFF`: Invalid/Sentinel

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

## 2. Layout

The **TypeMeta** section contains two contiguous arrays:

1. **Definitions**: `[TypeDef; header.type_defs_count]`
2. **Members**: `[TypeMember; header.type_members_count]`

Both `header.type_members_count` and `Slice.ptr` are `u16`, so the addressable range (0..65535) is identical—no capacity mismatch is possible by construction.

### 2.1. TypeDef (8 bytes)

Describes a single type.

```rust
#[repr(C)]
struct TypeDef {
    members: Slice,     // 4 bytes
    name: u16,          // StringId (0xFFFF for anonymous/wrappers)
    kind: u8,           // TypeKind
    _pad: u8,
}

#[repr(C)]
struct Slice {
    ptr: u16,           // Index or Data
    len: u16,           // Count
}
```

**Semantics of `members` field**:

| Kind       | `ptr` (u16)   | `len` (u16)    | Interpretation |
| :--------- | :------------ | :------------- | :------------- |
| `Optional` | `InnerTypeId` | 0              | Wrapper `T?`   |
| `Array*`   | `InnerTypeId` | 0              | Wrapper `T*`   |
| `Array+`   | `InnerTypeId` | 0              | Wrapper `T+`   |
| `Struct`   | `MemberIndex` | `MemberCount`  | Record fields  |
| `Enum`     | `MemberIndex` | `VariantCount` | Union variants |

> **Note**: The interpretation of `members.ptr` depends entirely on `kind`. For wrappers (`Optional`, `Array*`, `Array+`), `ptr` is a `TypeId`. For composites (`Struct`, `Enum`), `ptr` is an index into the TypeMember array. Parsers must dispatch on `kind` first.

- `MemberIndex`: Index into the **TypeMember** array (relative to the start of the members region).

### 2.2. TypeMember (4 bytes)

Describes a field in a struct or a variant in an enum.

```rust
#[repr(C)]
struct TypeMember {
    name: u16,      // StringId
    ty: u16,        // TypeId
}
```

**Storage**:
Members are tightly packed. Since `TypeDef` is 8 bytes, keeping `TypeMember` arrays aligned to 8 bytes ensures the whole section is dense.

Example of `Struct { x: Node, y: String }`:

1. `TypeDef`: `kind=Struct`, `members={ptr=0, len=2}`
2. `TypeMember[0]`: `name="x"`, `ty=Node`
3. `TypeMember[1]`: `name="y"`, `ty=String`

**Padding**: Like all sections, TypeMeta is padded to a 64-byte boundary at the end. Since `TypeDef` is 8 bytes and `TypeMember` is 4 bytes, the section naturally maintains internal alignment; only end-of-section padding is needed.

## 3. Recursive Types

Recursive types reference themselves via TypeId. Since types are addressed by index, cycles are naturally representable.

Example query:

```plotnik
List = [
    Nil: (nil)
    Cons: (cons (T) @head (List) @tail)
]
```

Type graph:

```text
Strings: ["List", "Nil", "Cons", "head", "tail"]
          Str#0   Str#1  Str#2   Str#3   Str#4

TypeDefs:
  T3: Enum "List" (Str#0), members={ptr=0, len=2}

TypeMembers:
  [0]: name=Str#1 ("Nil"),  ty=0 (Void)      // unit variant
  [1]: name=Str#2 ("Cons"), ty=T4            // payload is struct

TypeDefs (continued):
  T4: Struct 0xFFFF (anonymous), members={ptr=2, len=2}

TypeMembers (continued):
  [2]: name=Str#3 ("head"), ty=1 (Node)
  [3]: name=Str#4 ("tail"), ty=T3            // <-- self-reference to List
```

The `tail` field's type (`T3`) points back to the `List` enum. The runtime handles this via lazy evaluation or boxing, depending on the target language.
