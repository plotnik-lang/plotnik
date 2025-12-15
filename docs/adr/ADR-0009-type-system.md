# ADR-0009: Type System

- **Status**: Superseded by [ADR-0010](ADR-0010-type-system-v2.md)
- **Date**: 2025-01-14

## Context

Type inference transforms a query into typed structures. This ADR formalizes the inference rules with a unified conceptual model.

## Decision

### Core Principle

The type system reduces to two orthogonal concepts:

1. **Scope boundaries** — where captures land
2. **Payload rule** — what type a scope produces

> Captures bubble up to the nearest scope boundary; each scope's type is determined by its capture count and scope kind.

### Type Universe

```
τ ::= Void              -- no captures (TypeId = 0)
    | Node              -- AST node reference (TypeId = 1)
    | String            -- extracted source text (TypeId = 2)
    | Optional(τ)       -- zero or one
    | ArrayStar(τ)      -- zero or more
    | ArrayPlus(τ)      -- one or more
    | Struct(fields)    -- named fields
    | Enum(variants)    -- tagged union
```

### Captures

A capture `@name` creates a field that bubbles up to the nearest enclosing scope.

| Pattern         | Field Type           |
| --------------- | -------------------- |
| `(node) @x`     | `Node`               |
| `"literal" @x`  | `Node`               |
| `@x ::string`   | `String`             |
| `@x ::TypeName` | `TypeName` (nominal) |
| `{...} @x`      | scope payload        |
| `[...] @x`      | scope payload        |

### Scope Boundaries

**Golden rule**: `{}` and `[]` create a scope **only when captured**.

Scopes are created by:

1. **Definition root**: `Def = expr` — always a scope
2. **Captured sequence**: `{...} @name` — creates Struct scope
3. **Captured tagged alternation**: `[A: ... B: ...] @name` — creates Enum scope
4. **Captured untagged alternation**: `[...] @name` — creates Struct scope (merged fields)
5. **QIS** (Quantifier-Induced Scope): auto-created when quantifier has ≥2 captures
6. **Reference**: `(Def)` is opaque — blocks propagation entirely

**Uncaptured containers are transparent**:

- `{...}` without `@name` — captures pass through to outer scope
- `[...]` without `@name` — captures pass through (asymmetric ones become Optional)
- `[A: ... B: ...]` without `@name` — **tags ignored**, behaves like untagged

### Payload Rule

| Captures | Payload Type            |
| -------- | ----------------------- |
| 0        | `Void`                  |
| 1        | unwrap OR `Struct`      |
| ≥2       | `Struct { field, ... }` |

**Unwrap applies to** (1 capture → capture's type directly):

- Definition roots
- Enum variants
- QIS element types

**Always Struct** (1 capture → `Struct { field }`):

- Captured sequences `{...} @name`
- Captured untagged alternations `[...] @name`

**Rationale**: Explicit `@name` on a container signals intent to preserve structure. Definition roots and enum variants unwrap because the container name (def name / variant tag) already provides context.

### Reference Opacity

References are opaque barriers. Calling `(Foo)` does NOT inherit `Foo`'s captures.

```plotnik
A = (identifier) @name
B = (A)
C = (A) @node
```

Types:

- `A` → `Node` (1 capture, unwrapped)
- `B` → `Void` (0 captures — A's captures don't leak)
- `C` → `Node` (1 capture of type `A`, which is `Node`)

To access a definition's structure, capture it: `(Foo) @foo` yields a field of type `Foo`.

### Flat Scoping Principle

Query nesting does NOT create data nesting. Only scope boundaries matter:

```plotnik
Query = (a (b (c) @val))
```

Result: `Node` — the `(a ...)` and `(b ...)` wrappers contribute nothing. Single capture at def root unwraps.

```plotnik
Query = (a (b (c) @x (d) @y))
```

Result: `Struct { x: Node, y: Node }` — two captures form a struct.

### Cardinality

Cardinality describes how many values a capture produces:

| Cardinality | Notation | Wrapper     |
| ----------- | -------- | ----------- |
| Required    | `1`      | none        |
| Optional    | `?`      | `Optional`  |
| Star        | `*`      | `ArrayStar` |
| Plus        | `+`      | `ArrayPlus` |

**Propagation through nesting** (outer × inner):

```
  1 × 1 = 1    ? × 1 = ?    * × 1 = *    + × 1 = +
  1 × ? = ?    ? × ? = ?    * × ? = *    + × ? = *
  1 × * = *    ? × * = *    * × * = *    + × * = *
  1 × + = +    ? × + = *    * × + = *    + × + = +
```

**Join** (merging branches with same capture):

```
        +
       /|\
      * |
       \|
        ?
        |
        1
```

When join produces array (`*`/`+`) but branch has scalar (`1`/`?`), compiler inserts lifting coercion to wrap in singleton array.

### Alternation Semantics

**Key insight**: Tags only matter when the alternation is captured.

#### Uncaptured Alternation

Captures propagate to parent scope. Asymmetric captures become `Optional`. Tags are ignored.

```plotnik
// Tagged but uncaptured — tags ignored
Foo = [ A: (a) @x  B: (b) @y ]
```

- `@x` only in A → `Optional(Node)`
- `@y` only in B → `Optional(Node)`
- Result: `Struct { x: Optional(Node), y: Optional(Node) }`

```plotnik
// Symmetric captures
Bar = [ (a) @v  (b) @v ]
```

- `@v` in all branches → `Node` (not Optional)
- Result: `Node` (1 capture at def root, unwraps)

Diagnostic: warning for inline uncaptured tagged alternation (likely forgot `@name`).

#### Captured Untagged Alternation

Creates Struct scope. Branches merge. No unwrapping.

```plotnik
Foo = [ (a) @x  (b) @y ] @z
```

- `@z` creates Struct scope
- Merge: `{ x: Optional(Node), y: Optional(Node) }`
- Result: `Struct { z: Struct { x: Optional(Node), y: Optional(Node) } }`

```plotnik
Bar = [ (a) @v  (b) @v ] @z
```

- `@z` creates Struct scope
- Merge: `{ v: Node }`
- Always Struct (no unwrap): `Struct { v: Node }`
- Result: `Struct { z: Struct { v: Node } }`

#### Captured Tagged Alternation

Creates Enum scope. Each variant is independent, follows payload rule.

```plotnik
Result = [
    Ok: (value) @val
    Err: (error) @msg ::string
] @result
```

- Variant `Ok`: 1 capture → `Node` (unwrap)
- Variant `Err`: 1 capture → `String` (unwrap)
- Result: `Struct { result: Enum { Ok(Node), Err(String) } }`

#### Tagged Alternation at Definition Root

Special case: tagged alternation directly at definition root makes the definition itself an Enum.

```plotnik
Result = [
    Ok: (value) @val
    Err: (error) @msg ::string
]
```

- Result: `Enum Result { Ok(Node), Err(String) }`

No wrapper struct — the definition IS the enum.

### Unification Rules (Branch Merge)

When merging captures across untagged alternation branches:

**1-level merge semantics**: Top-level fields merge with optionality; nested struct mismatches are errors.

```
// OK: top-level field merge
Branch 1: { x: Node, y: Node }
Branch 2: { x: Node, z: String }
Result:   { x: Node, y: Optional(Node), z: Optional(String) }

// OK: nested structs identical
Branch 1: { data: { a: Node }, extra: Node }
Branch 2: { data: { a: Node } }
Result:   { data: { a: Node }, extra: Optional(Node) }

// ERROR: nested structs differ
Branch 1: { data: { a: Node } }
Branch 2: { data: { b: Node } }
→ Error: field `data` has incompatible struct types

// ERROR: primitive mismatch
Branch 1: { val: String }
Branch 2: { val: Node }
→ Error: field `val` has incompatible types
```

**Rationale**: Deep recursive merging produces heavily-optional types, defeating typed extraction's purpose. Use tagged alternations for precise discrimination.

### Quantifier-Induced Scope (QIS)

When a quantified expression has **≥2 propagating captures**, QIS auto-creates a scope to keep values paired per-iteration.

```plotnik
// 2 captures under quantifier → QIS triggers
Functions = (function
    name: (identifier) @name
    body: (block) @body
)*
```

- QIS creates element scope with 2 captures → Struct (always, by payload rule)
- Result: `ArrayStar(FunctionsItem)` where `FunctionsItem { name: Node, body: Node }`
- Definition has 1 propagating capture (the array) → unwrap
- Final: `Functions` is `ArrayStar(FunctionsItem)`

```plotnik
// 1 capture → no QIS, standard cardinality multiplication
Items = { (item) @item }*
```

- No QIS (only 1 capture)
- `@item` gets cardinality `*`
- Result: `Node` would be wrong... actually 1 capture at def root
- Wait, the capture is `ArrayStar(Node)`, so def root has 1 "field"
- Result: `ArrayStar(Node)` (unwrapped)

**Naming**:

- At definition root: `{Def}Item`
- With explicit capture `E* @name`: `{Parent}{Name}`
- Neither (not at root, no capture): Error — require explicit `@name`

### Synthetic Naming

Types without explicit `::Name` receive synthetic names:

| Context              | Pattern           |
| -------------------- | ----------------- |
| Definition root      | `{DefName}`       |
| Captured sequence    | `{Def}{Capture}`  |
| Captured alternation | `{Def}{Capture}`  |
| Enum variant payload | `{Enum}{Variant}` |
| QIS element          | `{Def}Item`       |

Collision resolution: append numeric suffix (`Foo`, `Foo2`, `Foo3`).

### Error Conditions

| Condition                         | Severity | Recovery                   |
| --------------------------------- | -------- | -------------------------- |
| Incompatible types in alternation | Error    | Use invalid type, continue |
| Nested struct mismatch            | Error    | Use invalid type, continue |
| Duplicate capture in same scope   | Error    | Keep first                 |
| Inline uncaptured tagged alt      | Warning  | Treat as untagged          |
| QIS without capture (not at root) | Error    | Cannot infer element type  |

Error reporting is exhaustive: all incompatibilities across all branches are reported, not just the first.

## Examples

### Single Capture at Definition Root

```plotnik
Name = (identifier) @name
```

- 1 capture at def root → unwrap
- Result: `Name` is `Node`

### Multiple Captures at Definition Root

```plotnik
Binding = (variable_declaration
    name: (identifier) @name
    value: (expression) @value
)
```

- 2 captures → Struct
- Result: `Binding { name: Node, value: Node }`

### Captured vs Uncaptured Sequence

```plotnik
// Captured sequence — creates scope, always Struct
Foo = { (bar) @bar } @baz
```

- `@bar` stays in `@baz`'s scope
- Captured sequence: always Struct
- Result: `Struct { baz: Struct { bar: Node } }`

```plotnik
// Uncaptured sequence — transparent, captures pass through
Foo = { (bar) @bar }
```

- `{...}` without `@name` is transparent
- `@bar` bubbles up to definition root
- 1 capture at def root → unwrap
- Result: `Foo` is `Node`

### Enum at Definition Root

```plotnik
Boolean = [
    True: "true"
    False: "false"
]
```

- Tagged alt at root, 0 captures per variant → Void
- Result: `Enum Boolean { True, False }`

### Mixed Variant Payloads

```plotnik
Expr = [
    Lit: (number) @value
    Bin: (binary left: (_) @left right: (_) @right)
]
```

- `Lit`: 1 capture → unwrap → `Node`
- `Bin`: 2 captures → Struct
- Result: `Enum Expr { Lit(Node), Bin { left: Node, right: Node } }`

### QIS with Multiple Captures

```plotnik
Module = (module {
    (function
        name: (identifier) @name
        params: (parameters) @params
    )*
})
```

- 2 captures under `*` → QIS triggers
- Element type: `ModuleItem { name: Node, params: Node }`
- Array propagates to def root (1 capture) → unwrap
- Result: `Module` is `ArrayStar(ModuleItem)`

## Consequences

**Positive**:

- Golden rule ("only captured containers create scopes") is easy to remember
- Payload rule is uniform: 0→void, 1→unwrap, 2+→struct
- Exception for captured containers (always Struct) matches user intent
- "Tags only matter when captured" eliminates confusion

**Negative**:

- Field name loss on single-capture unwrap (mitigated by `::Type` annotation)
- 1-level merge is less flexible than deep merge (intentional trade-off)

**Alternatives Considered**:

- Always wrap in struct (rejected: verbose types like `{ val: Node }` instead of `Node`)
- Deep recursive merge (rejected: heavily-optional types defeat typed extraction)
- Error on uncaptured tagged alternations (rejected: too restrictive)
