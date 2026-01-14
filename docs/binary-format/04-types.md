# Binary Format: Type Metadata

Type system metadata for code generation and runtime validation. Describes the shape of data extracted by queries.

## Primitives

**TypeId (u16)**: Zero-based index into TypeDefs. All types, including primitives, are stored as TypeDef entries.

**TypeKind (u8)**: Discriminator for TypeDef.

| Value | Kind            | Description                    |
| ----- | --------------- | ------------------------------ |
| 0     | `Void`          | Unit type, captures nothing    |
| 1     | `Node`          | AST node reference             |
| 2     | `String`        | Source text                    |
| 3     | `Optional`      | Wraps another type             |
| 4     | `ArrayZeroOrMore` | Zero or more (T*)            |
| 5     | `ArrayOneOrMore`  | One or more (T+)             |
| 6     | `Struct`        | Record with named fields       |
| 7     | `Enum`          | Discriminated union            |
| 8     | `Alias`         | Named reference to another type |

### Node Semantics

The `Node` type represents a platform-dependent handle to a tree-sitter AST node:

| Context    | Representation                                             |
| :--------- | :--------------------------------------------------------- |
| Rust       | `tree_sitter::Node<'tree>` (lifetime-bound reference)      |
| TypeScript | Binding-provided object with `startPosition`, `text`, etc. |
| JSON       | Unique node identifier (e.g., `"node:42"`)                 |

## Sections

Three separate sections store type metadata. Counts are in the main header.

### TypeDefs

- **Section Offset**: Computed (follows Trivia)
- **Record Size**: 4 bytes
- **Count**: `header.type_defs_count`

```rust
#[repr(C)]
struct TypeDef {
    data: u16,      // TypeId OR MemberIndex (depends on kind)
    count: u8,      // Member count (0 for wrappers/alias)
    kind: u8,       // TypeKind
}
```

**Field semantics by kind**:

| Kind              | `data`         | `count`        |
| :---------------- | :------------- | :------------- |
| `Void`            | 0              | 0              |
| `Node`            | 0              | 0              |
| `String`          | 0              | 0              |
| `Optional`        | Inner TypeId   | 0              |
| `ArrayZeroOrMore` | Inner TypeId   | 0              |
| `ArrayOneOrMore`  | Inner TypeId   | 0              |
| `Alias`           | Target TypeId  | 0              |
| `Struct`          | MemberIndex    | FieldCount     |
| `Enum`            | MemberIndex    | VariantCount   |

> **Limit**: `count` is u8, so composites are limited to 255 members.

### TypeMembers

- **Section Offset**: Computed (follows TypeDefs)
- **Record Size**: 4 bytes
- **Count**: `header.type_members_count`

```rust
#[repr(C)]
struct TypeMember {
    name: u16,      // StringId (field or variant name)
    ty: u16,        // TypeId (field type or variant payload)
}
```

For struct fields: `name` is the field name, `ty` is the field's type.
For enum variants: `name` is the variant tag, `ty` is the payload type (use `Void` for unit variants).

### TypeNames

- **Section Offset**: Computed (follows TypeMembers)
- **Record Size**: 4 bytes
- **Count**: `header.type_names_count`

```rust
#[repr(C)]
struct TypeName {
    name: u16,      // StringId
    type_id: u16,   // TypeId
}
```

Sorted lexicographically by name (resolved via String Table) for binary search.

**Usage**:
- Named definitions (`List = [...]`) get an entry
- Custom type annotations (`@x :: Identifier`) create an Alias TypeDef with an entry
- Anonymous types have no entry

## Examples

> **Note**: Only **used** primitives are emitted to TypeDefs. The emitter writes them first in order (Void, Node, String), then composite types.

### Simple Struct

Query: `Q = (function name: (identifier) @name)`

```
[type_defs]
T0 = <Node>
T1 = Struct  M0:1  ; { name }

[type_members]
M0: S1 → T0  ; name: <Node>

[type_names]
N0: S2 → T1  ; Q
```

### Recursive Enum

Query:
```
List = [
    Nil: (nil)
    Cons: (cons (a) @head (List) @tail)
]
```

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

### Custom Type Annotation

Query: `Q = (identifier) @name :: Identifier`

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

## Validation

Loaders must verify for `Struct`/`Enum` kinds:

- `(data as u32) + (count as u32) ≤ type_members_count`

This prevents out-of-bounds reads from malformed binaries.

## Code Generation

To emit types (TypeScript, Rust, etc.):

1. Build reverse map: `TypeId → Option<StringId>` from TypeNames
2. Start from entrypoints or iterate TypeNames
3. For each type:
   - Look up structure in TypeDefs
   - Look up name (if any) in reverse map
   - Emit named types with their name; anonymous types inline or with generated names
4. Detect when multiple names map to the same TypeId → emit aliases
