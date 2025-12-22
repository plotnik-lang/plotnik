# Plotnik Type System

Plotnik infers static types from query structure. This governs how captures materialize into output (JSON, structs, etc.).

## Design Philosophy

Plotnik prioritizes **predictability** and **structural clarity** over terseness.

Two principles guide the type system:

1. **Flat structure**: Captures bubble up to the nearest scope boundary.

2. **Strict dimensionality**: Quantifiers (`*`, `+`) containing captures require an explicit row capture. The alternative could be creating parallel arrays, but it's hard to maintain the per-iteration association for `a[i]` and `b[i]`.

### Why Transparent Scoping

Extracting a pattern into a definition shouldn't change output:

```
// Inline
(function name: (identifier) @name)
→ { name: Node }

// Extracted
Func = (function name: (identifier) @name)
(Func)
→ { name: Node }   // Same shape—@name bubbles through
```

If definitions created implicit boundaries, extraction would wrap output in a new struct, breaking downstream types.

## 1. Strict Dimensionality

This is the core rule that prevents association loss.

### The Rule

**Any quantified pattern (`*`, `+`) containing captures must have an explicit row capture.**

| Pattern                           | Status  | Reason                                     |
| --------------------------------- | ------- | ------------------------------------------ |
| `(identifier)* @ids`              | ✓ Valid | No internal captures → scalar list         |
| `{ (a) @a (b) @b }* @rows`        | ✓ Valid | Internal captures + row capture → row list |
| `{ (a) @a (b) @b }*`              | ✗ Error | Internal captures, no row capture          |
| `(func (id) @name)*`              | ✗ Error | Internal capture, no row structure         |
| `(func (id) @name)* @funcs`       | ✗ Error | `@funcs` captures nodes, not rows          |
| `(Item)*` where Item has captures | ✗ Error | Transitive: definition's captures count    |

### Transitive Application

Strict dimensionality applies **transitively through definitions**. Since definitions are transparent (captures bubble up), quantifying a definition that contains captures is equivalent to quantifying those captures directly:

```
// Definition with captures
Item = (pair (key) @k (value) @v)

// These are equivalent after expansion:
(Item)*                              // ✗ Error
(pair (key) @k (value) @v)*          // ✗ Error (same thing)

// Fix: wrap in row capture
{ (Item) @item }* @items             // ✓ Valid
```

The compiler expands definitions before validating strict dimensionality. This prevents a loophole where extracting a pattern into a definition would bypass the rule.

### Scalar Lists

When the quantified pattern has **no internal captures**, the outer capture collects nodes directly:

```
(decorator)* @decorators
→ { decorators: Node[] }

(identifier)+ @names
→ { names: [Node, ...Node[]] }  // Non-empty array
```

Use case: collecting simple tokens (identifiers, keywords, literals).

### Row Lists

When the quantified pattern **has internal captures**, wrap in a sequence and capture the sequence:

```
{
  (decorator) @dec
  (function_declaration) @fn
}* @items
→ { items: { dec: Node, fn: Node }[] }
```

For node patterns with internal captures, wrap explicitly:

```
// ERROR: internal capture without row structure
(parameter (identifier) @name)*

// OK: explicit row
{ (parameter (identifier) @name) @param }* @params
→ { params: { param: Node, name: string }[] }
```

The strict rule forces you to think about structure upfront.

### Optional Bubbling

The `?` quantifier does **not** add dimensionality—it produces at most one value, not a list. Therefore, optional groups without captures are allowed:

```
{ (decorator) @dec }?
→ { dec?: Node }   // Bubbles to parent as optional field

{ (modifier) @mod (decorator) @dec }?
→ { mod?: Node, dec?: Node }   // Both bubble as optional
```

This lets optional fragments contribute fields directly to the parent struct without forcing an extra wrapper object.

## 2. Scope Model

### Universal Bubbling

Scopes are transparent by default. Captures bubble up through definitions and containers until hitting an explicit scope boundary.

This enables reusable pattern fragments that contribute fields directly to parent output without creating nesting.

- **Definitions (`Def = ...`)**: Transparent (macro-like)
- **Uncaptured Containers (`{...}`, `[...]`)**: Transparent
- **References (`(Def)`)**: Transparent

### Explicit Scope Boundaries

New data structures are created only when explicitly requested:

1. **Captured Groups**: `{...} @name` → Struct
2. **Captured Alternations**: `[...] @name` → Union
3. **Tagged Alternations**: `[ L: ... ] @name` → Tagged Union

In case of using quantifiers with captures, compiler forces you to create scope boundaries.

## 3. Data Shapes

### Structs

Created by `{ ... } @name`:

| Captures | Result                             |
| -------- | ---------------------------------- |
| 0        | `Struct {}` (Empty)                |
| 1+       | `Struct { field_1, ..., field_N }` |

**No Implicit Unwrap**: `(node) @x` produces `{ x: Node }`, never bare `Node`.

**Empty Structs**: `{ ... } @x` with no internal captures produces `{ x: {} }`. This ensures `x` is always an object, so adding fields later is non-breaking.

### Unions

Created by `[ ... ]`:

- **Tagged**: `[ L1: (a) @a  L2: (b) @b ]` → `{ "$tag": "L1", "$data": { a: Node } }`
- **Untagged**: `[ (a) @a  (b) @b ]` → `{ a?: Node, b?: Node }` (merged 1-level deep)

### Enum Variants

| Captures | Payload             |
| -------- | ------------------- |
| 0        | `Struct {}` (Empty) |
| 1+       | Struct              |

```
Result = [
    Ok: (value) @val
    Err: (error (code) @code (message) @msg)
]
```

Single-capture variants stay wrapped (`result.$data.val`), making field additions non-breaking.

## 4. Cardinality

Quantifiers determine whether a field is singular, optional, or an array:

| Pattern   | Output Type      | Meaning      |
| --------- | ---------------- | ------------ |
| `(A) @a`  | `a: T`           | exactly one  |
| `(A)? @a` | `a?: T`          | zero or one  |
| `(A)* @a` | `a: T[]`         | zero or more |
| `(A)+ @a` | `a: [T, ...T[]]` | one or more  |

### Row Cardinality

When using row lists, the outer quantifier determines list cardinality:

```
{ (a) @a (b) @b }* @rows   → rows: { a: T, b: T }[]
{ (a) @a (b) @b }+ @rows   → rows: [{ a: T, b: T }, ...]
{ (a) @a (b) @b }? @row    → row?: { a: T, b: T }
```

### Nested Quantifiers

Within a row, inner quantifiers apply to fields:

```
{
  (decorator)* @decs      // Array field within each row
  (function) @fn          // Singular field within each row
}* @items
→ { items: { decs: Node[], fn: Node }[] }
```

Each row has its own `decs` array—no cross-row mixing.

## 5. Type Unification in Alternations

Shallow unification across untagged branches:

| Scenario                    | Result        |
| --------------------------- | ------------- |
| Same capture, all branches  | Required      |
| Same capture, some branches | Optional      |
| Type mismatch               | Compile error |

```
[
  (a) @x
  (b) @x
]  // x: Node (required)

[
  (_ (a) @x (b) @y)
  (_ (a) @x)
]  // x: Node, y?: Node

[
  (a) @x ::string
  (b) @x
]  // ERROR: String vs Node
```

The choice of shallow unification is intentional. For more precision, users should use tagged unions.

### Array Captures in Alternations

When a quantified capture appears in some branches but not others, the missing branch emits an empty array:

```
[
  (a)+ @x
  (b)
]  // x: Node[]
```

Untagged alternations are "I don't care which branch matched"—so distinguishing "branch didn't match" from "matched zero times" is irrelevant. The empty array is easier to consume downstream.

When types start to conflict, use tagged alternations:

```
[
    Str: (a) @x ::string
    Node: (b) @x
] @result
```

### Unification Rules

1. Primitives: exact match required
2. Arrays: element types unify; looser cardinality wins (`+` ∪ `*` → `*`)
3. Structs: identical field sets, recursively compatible
4. Enums: identical variant sets

### 1-Level Merge Only

Top-level fields merge with optionality; nested mismatches are errors:

```
// OK: top-level merge (scalars become optional)
{ x: Node, y: Node } ∪ { x: Node, z: String } → { x: Node, y?: Node, z?: String }

// OK: arrays emit [] when missing (not null)
{ items: Node[], x: Node } ∪ { x: Node } → { items: Node[], x: Node }

// OK: identical nested
{ data: { a: Node } } ∪ { data: { a: Node }, extra: Node } → { data: { a: Node }, extra?: Node }

// ERROR: nested differ
{ data: { a: Node } } ∪ { data: { b: Node } } → incompatible struct types
```

Deep merging produces heavily-optional types that defeat typed extraction's purpose.

## 6. Recursion

Self-referential types via:

1. **TypeId indirection**: Types reference by ID, enabling cycles
2. **Escape analysis**: Every cycle needs a non-recursive exit
3. **Guarded recursion**: Every cycle must consume input (descend)
4. **Automatic detection**: Compiler generates Call/Return instead of inlining

### Example

```
Expr = [
    Lit: (number) @value ::string
    Binary: (binary_expression
        left: (Expr) @left
        right: (Expr) @right
    )
]
```

### Requirements

```
Loop = (Loop)                    // ERROR: no escape path
Expr = [ Lit: (n) @n  Rec: (Expr) @e ]  // OK: Lit escapes

A = (B)
B = (A)                          // ERROR: no input consumed

A = (foo (B))
B = (bar (A))                    // OK: descends each step
```

### Scope Boundaries

Recursive definitions get automatic type boundaries:

```
NestedCall = (call_expression
    function: [(identifier) @name (NestedCall) @inner])
```

## 7. Type Metadata

For codegen, types are named:

- **Explicit**: `@name :: TypeName`
- **Synthetic**: `{DefName}{FieldName}` (e.g., `FuncParams`), with numeric suffix on collision
