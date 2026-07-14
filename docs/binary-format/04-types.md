# Binary Format: Type Metadata

Type system metadata for code generation and runtime validation. Describes the shape of data extracted by queries.

## Primitives

**TypeId (u16)**: Zero-based index into TypeDefs. All types, including primitives, are stored as TypeDef entries.

**TypeKind (u8)**: Discriminator for TypeDef.

| Value | Kind             | Description                     |
| ----- | ---------------- | ------------------------------- |
| 0     | `NoValue`        | Successful match with no value  |
| 1     | `Node`           | Tree-sitter node reference      |
| 2     | `Option`         | Zero or one value               |
| 3     | `ListZeroOrMore` | Zero or more (T\*)              |
| 4     | `ListOneOrMore`  | One or more (T+)                |
| 5     | `Record`         | Record with named fields        |
| 6     | `Variant`        | Variant type with named cases   |
| 7     | `Alias`          | Named reference to another type |
| 8     | `Text`           | Borrowed source text            |
| 9     | `Bool`           | Boolean value                   |

### Node Semantics

The `Node` type represents a platform-dependent handle to a Tree-sitter syntax-tree node:

| Context    | Representation                                                         |
| :--------- | :--------------------------------------------------------------------- |
| Rust       | `tree_sitter::Node<'tree>` (lifetime-bound reference)                  |
| TypeScript | Target-configured; serialized values default to `{ kind, text, span }` |
| JSON       | `{ kind, text, span: [start, end] }`                                   |

### Text and Boolean Semantics

`Text` is the UTF-8 source slice selected by a direct node-scalar effect or a
balanced scalar-provenance frame; it renders as `string` in TypeScript and
`&'s str` in Rust. `Bool` is an explicit boolean carried by a scalar effect;
the runtime does not infer list or text truthiness. Neither primitive contains
member metadata.

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

| Kind             | `data`        | `count`    |
| :--------------- | :------------ | :--------- |
| `NoValue`        | 0             | 0          |
| `Node`           | 0             | 0          |
| `Text`           | 0             | 0          |
| `Bool`           | 0             | 0          |
| `Option`         | Inner TypeId  | 0          |
| `ListZeroOrMore` | Inner TypeId  | 0          |
| `ListOneOrMore`  | Inner TypeId  | 0          |
| `Alias`          | Target TypeId | 0          |
| `Record`         | MemberIndex   | FieldCount |
| `Variant`        | MemberIndex   | CaseCount  |

> **Limit**: `count` is u8, so composites are limited to 255 members.

### TypeMembers

- **Section Offset**: Computed (follows TypeDefs)
- **Record Size**: 4 bytes
- **Count**: `header.type_members_count`

```rust
#[repr(C)]
struct TypeMember {
    name: u16,      // StringId (field or case name)
    ty: u16,        // TypeId (field type or case payload)
}
```

For record fields, `name` is the field name and `ty` is the field type. For
variant cases, `name` is the case name and `ty` is the payload type (`NoValue`
marks a no-payload case).

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
- Custom capture types (`@x :: Identifier`) create an Alias TypeDef with an entry
- Anonymous types have no entry

## Examples

> **Note**: Only **used** primitives are emitted to TypeDefs. The emitter writes
> them first in order (`NoValue`, `Node`, `Text`, `Bool`), then custom result types.

### Simple Record

Query: `Q = (function name: (identifier) @name)`

```
[type_defs]
T0 = <Node>
T1 = Record  M0:1  ; { name }

[type_members]
M0: S1 → T0  ; name: <Node>

[type_names]
N0: S2 → T1  ; Q
```

### Recursive Variant

Query:

```
List = [
    Nil: (nil)
    Cons: (cons (a) @head (List) @tail)
]
```

```
[type_defs]
T0 = <NoValue>
T1 = <Node>
T2 = Record  M0:2  ; { head, tail }
T3 = Variant M2:2  ; Nil | Cons

[type_members]
M0: S1 → T1  ; head: <Node>
M1: S2 → T3  ; tail: List
M2: S3 → T0  ; Nil: <NoValue>
M3: S4 → T2  ; Cons: T2

[type_names]
N0: S5 → T3  ; List
```

### Custom Capture Type

Query: `Q = (identifier) @name :: Identifier`

```
[type_defs]
T0 = <Node>
T1 = Alias(T0)
T2 = Record  M0:1  ; { name }

[type_members]
M0: S2 → T1  ; name: Identifier

[type_names]
N0: S1 → T1  ; Identifier
N1: S3 → T2  ; Q
```

## Validation

Loaders must verify:

- `Record`/`Variant`: `(data as u32) + (count as u32) ≤ type_members_count`, and every
  member's `ty` is a valid TypeId (`< type_defs_count`).
- Wrapper/`Alias`: `data` (inner/target TypeId) is `< type_defs_count`, and the
  reserved `count` is `0`.
- `NoValue`/`Node`/`Text`/`Bool`: the reserved `data` and `count` are both `0`.

The bounds checks prevent out-of-bounds reads from malformed binaries; the
reserved-zero checks reject smuggled state where the format pins a field to zero.

## Code Generation

The compiler's naming pass makes the name table complete and consistent, so a
code generator is a pure renderer:

1. Build reverse map: `TypeId → Option<StringId>` from TypeNames
2. Start from entry points, walk reachable types
3. For each type:
   - Look up structure in TypeDefs
   - Named → emit a declaration under that name, verbatim; anonymous (variant
     case payloads, foreign bytecode) → render inline at use sites
4. The same name may appear on several TypeIds only for structurally identical
   types (nominal twins from repeated custom capture types) — one declaration serves
   them all. Never invent names.
