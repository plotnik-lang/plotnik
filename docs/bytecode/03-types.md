# Types

- [Design](#design)
- [Implementation](#implementation)
  - [Type definitions](#type-definitions)
  - [Members](#members)
  - [Names](#names)
  - [Emitted form](#emitted-form)
  - [Validation](#validation)

## Design

The type tables describe the values a query can produce. An entry point selects a result type, and [effects](04-effects.md) describe how to construct the result when a match succeeds. The tables contain the shape of the result, while its values come from the matched document and syntax tree.

A result can combine nodes, text, and booleans through records, lists, options, and variants. `NoValue` describes a match that produces no result value.

For example, suppose a result type named `Item` has a text field `name` and an optional node field `selected`. Three tables describe it:

```text
TypeNames                         TypeDefs
  "Item" -----------------------> Record
                                    |
                                    | members
                                    v
                                  TypeMembers
                                    |
                                    +-- "name" ------> Text
                                    |
                                    +-- "selected" --> Option --> Node
```

`TypeDefs` describes the shapes. The record's fields are consecutive entries in `TypeMembers`, where each entry supplies a field name and its type. `TypeNames` gives the record the name `Item`. The field `selected` remains present even when its option is absent.

Keeping these references separate lets fields share a type and lets several names refer to the same shape. A type name does not add a field or a wrapper around the value. An alias also has its target's shape without adding a wrapper.

Type references can point forward or backward. This allows recursive results without copying a type's definition into each place that uses it. Loading checks fields and references, but does not prove that every type graph has a finite value or that the instructions produce their declared result. The writer has to keep the types and effects consistent.

## Implementation

`TypeDefs`, `TypeMembers`, and `TypeNames` each contain at most 65535 entries. Their counts come from the bytecode header. Type IDs and member indices are zero-based table positions.

### Type definitions

Each `TypeDefs` entry occupies 4 bytes. Its `kind` selects the shape and determines how the other fields are used.

```text
0                  2        3        4
+------------------+--------+--------+
| data: u16        | count  | kind   |
+------------------+--------+--------+
                       u8       u8
```

| `kind` | Shape            | `data`             | `count`     |
| ------ | ---------------- | ------------------ | ----------- |
| 0      | `NoValue`        | 0                  | 0           |
| 1      | `Node`           | 0                  | 0           |
| 2      | `Option`         | Inner type ID      | 0           |
| 3      | `ListZeroOrMore` | Element type ID    | 0           |
| 4      | `ListOneOrMore`  | Element type ID    | 0           |
| 5      | `Record`         | First member index | Field count |
| 6      | `Variant`        | First member index | Case count  |
| 7      | `Alias`          | Target type ID     | 0           |
| 8      | `Text`           | 0                  | 0           |
| 9      | `Bool`           | 0                  | 0           |

All other `kind` values are invalid. Every zero in the table is required. A type's ID depends on its position even when its shape is primitive. For example, a `Node` entry at index 7 has type ID 7 and `kind` 1.

For `Option`, either list kind, and `Alias`, `data` identifies an existing type definition.

`NoValue` means a successful match without a value. `Node` refers to a matched node in the external syntax tree. `Text` holds UTF-8 text selected from the matched document. The bytecode stores neither result node handles nor result text. Effects select an explicit `false` or `true` for a `Bool` result, with no conversion through truthiness.

`Option` holds either an absent value or one value of its inner type. Absence is still a field value, so an optional record field is not omitted. Both list kinds hold ordered elements of their declared type. `ListZeroOrMore` permits an empty list. `ListOneOrMore` requires at least one element.

`Record` contains its declared fields with their declared types. `Variant` selects one declared case and, when that case has a payload, a value of its payload type. Their field and case definitions are in `TypeMembers`.

`Alias` has the value shape of the type identified by `data`. `TypeNames` supplies its name, if any.

### Members

Each `TypeMembers` entry occupies 4 bytes. `name` is a required [StringId](06-tables.md#strings). `type_id` identifies the field's value type or the case's payload type.

```text
0                  2                  4
+------------------+------------------+
| name: u16        | type_id: u16     |
+------------------+------------------+
```

For a record or variant, `data` identifies the first member and `count` gives the number of consecutive members. One type can have at most 255 members. This consecutive group is its member run.

```text
Record or Variant                   TypeMembers
                                      ...
  data ---------------------------> +--------------+
                                    | first member | --+
                                    | ...          |   | count entries
                                    | last member  | --+
                                    +--------------+
                                      ...
```

Effects and [inspection references to result members](05-spans.md#bindings) use absolute indices in `TypeMembers`. For example, a run starting at index 5 has members 5 and 6 when its count is 2. An effect selecting the second member carries 6. Member effects have only a 10-bit payload, so they can address indices 0 through 1023 even when the table is larger.

A record member names a field. A variant member names a case. A case has no payload exactly when its `type_id` directly selects a `NoValue` definition. An alias targeting `NoValue` still counts as a payload type. The [variant effects](04-effects.md) use this distinction when constructing the result.

Member order does not determine which query alternative is preferred. That preference comes from instruction successors.

### Names

Each `TypeNames` entry occupies 4 bytes and uses the same layout as a member: `name: u16` at offset 0, followed by `type_id: u16` at offset 2. Here `name` is a required [StringId](06-tables.md#strings) for the type's name, and `type_id` selects an existing type definition.

A type can have no name, one name, or several names. A name can also refer to several types. The reader requires neither unique names nor a particular table order, and imposes no additional name syntax.

### Emitted form

The compiler emits only types needed by reachable definition outputs. Used primitives precede other types in the order `NoValue`, `Node`, `Text`, `Bool`, with unused primitives omitted. Composite member runs are disjoint and together cover the emitted member table.

Type names identify named value-producing definitions and explicit result type names. Match-only definitions do not require a type-name entry. When the compiler binds the same name to different type IDs, those types are structurally equal.

### Validation

A record or variant's member run fits entirely in `TypeMembers`. An empty run can start at any member index or immediately after the last member. In an empty table, the only valid start is zero. The end of the run is checked without integer truncation or wrapping.

Every member in a run identifies an existing type definition. A member outside all runs has no type-target check from the type tables. The effect that begins a variant case, `VariantOpen`, still checks the target of any member it uses. Every member's `name` has to be a valid required string ID, including members outside all runs. The same string requirement applies to every type-name entry.

Beyond these field and reference checks, the reader accepts unused entries, overlapping runs, empty composites, duplicate member names, repeated primitives, and any table order. It also accepts cycles, including cycles made entirely of aliases. Acceptance does not establish that a type has a finite value or that the program's effects produce that type.
