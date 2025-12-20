# Plotnik Type System

Plotnik infers static types from query structure. This governs how captures materialize into output (JSON, structs, etc.).

## Design Philosophy

Plotnik prioritizes **schema evolution** and **refactoring safety** over local intuition.

Two principles guide the type system:

1. **Additive captures are non-breaking**: Adding a new `@capture` to an existing query should not invalidate downstream code that uses other captures.

2. **Extract-refactor equivalence**: Moving a pattern fragment into a named definition should not change the output shape.

These constraints produce designs that may initially surprise users (parallel arrays instead of row objects, transparent scoping instead of nesting), but enable queries to evolve without breaking consumers.

### Why Parallel Arrays

Traditional row-oriented output breaks when queries evolve:

```
// v1: Extract names
(identifier)* @names
→ { names: Node[] }

// v2: Also extract types (row-oriented would require restructuring)
{ (identifier) @name (type) @type }* @items
→ { items: [{ name, type }, ...] }   // BREAKING: names[] is gone
```

Plotnik's columnar approach:

```
// v1
(identifier)* @names
→ { names: Node[] }

// v2: Add types alongside
{ (identifier) @names (type) @types }*
→ { names: Node[], types: Node[] }   // NON-BREAKING: names[] unchanged
```

Existing code using `result.names[i]` continues to work.

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

## Mental Model

| Operation          | Nested (tree-sitter) | Transparent (Plotnik) |
| ------------------ | -------------------- | --------------------- |
| Extract definition | `res.def.x`          | `res.x` (unchanged)   |
| List of items      | Implicit row struct  | Explicit `{...} @row` |
| Capture collision  | Silent data loss     | Compiler error        |
| Fix collision      | Manual re-capture    | Wrap: `(Def) @alias`  |

## 1. Transparent Graph Model

### Universal Bubbling

Scopes are transparent by default. Captures bubble up through definitions and containers until hitting an explicit scope boundary.

This enables reusable fragments ("mixins") that contribute fields to parent output without creating nesting.

- **Definitions (`Def = ...`)**: Transparent (macro-like)
- **Uncaptured Containers (`{...}`, `[...]`)**: Transparent
- **References (`(Def)`)**: Transparent

### Explicit Scope Boundaries

New data structures are created only when explicitly requested:

1. **Captured Groups**: `{...} @name` → Struct
2. **Captured Alternations**: `[...] @name` → Union
3. **Tagged Alternations**: `[ L: ... ] @name` → Tagged Union

## 2. Data Shapes

### Structs

Created by `{ ... } @name`:

| Captures | Result                             |
| -------- | ---------------------------------- |
| 0        | `Void`                             |
| 1+       | `Struct { field_1, ..., field_N }` |

**No Implicit Unwrap**: `(node) @x` produces `{ x: Node }`, never bare `Node`. Adding fields later is non-breaking.

### Unions

Created by `[ ... ]`:

- **Tagged**: `[ L1: (A) @a  L2: (B) @b ]` → `{ "$tag": "L1", "$data": { a: Node } }`
- **Untagged**: `[ (A) @a  (B) @b ]` → `{ a?: Node, b?: Node }` (merged)

### Enum Variants

| Captures | Payload   |
| -------- | --------- |
| 0        | Unit/Void |
| 1+       | Struct    |

```plotnik/docs/type-system.md#L58-61
Result = [
    Ok: (value) @val
    Err: (error (code) @code (message) @msg)
]
```

Single-capture variants stay wrapped (`result.$data.val`), making field additions non-breaking.

## 3. Parallel Arrays (Columnar Output)

Quantifiers (`*`, `+`) produce arrays per-field, not lists of objects:

```plotnik/docs/type-system.md#L75-75
{ (Key) @k (Value) @v }*
```

Output: `{ "k": ["key1", "key2"], "v": ["val1", "val2"] }`

This Struct-of-Arrays layout enables non-breaking schema evolution: adding `@newfield` to an existing loop doesn't restructure existing fields. It also avoids implicit row creation and is efficient for columnar analysis.

For List-of-Objects, wrap explicitly:

```plotnik/docs/type-system.md#L84-84
( { (Key) @k (Value) @v } @entry )*
```

Output: `{ "entry": [{ "k": "key1", "v": "val1" }, ...] }`

## 4. Row Integrity

Parallel arrays require `a[i]` to correspond to `b[i]`. The compiler enforces this:

**Rule**: Quantified scopes cannot mix synchronized and desynchronized fields.

| Type           | Cardinality | Behavior                                             |
| -------------- | ----------- | ---------------------------------------------------- |
| Synchronized   | `1` or `?`  | One value per iteration (`?` emits null when absent) |
| Desynchronized | `*` or `+`  | Variable values per iteration                        |

`?` is synchronized because it emits null placeholders—like nullable columns in Arrow/Parquet.

### Nested Quantifiers

Cardinality multiplies through nesting:

| Outer | Inner | Result |
| ----- | ----- | ------ |
| `1`   | `*`   | `*`    |
| `*`   | `1`   | `*`    |
| `*`   | `*`   | `*`    |
| `+`   | `+`   | `+`    |
| `?`   | `+`   | `*`    |

Example:

```plotnik/docs/type-system.md#L123-123
{ (A)* @a  (B) @b }*  // ERROR: @a is *, @b is 1
{ (A)? @a  (B) @b }*  // OK: both synchronized
```

Fixes:

```plotnik/docs/type-system.md#L128-129
{ (A)* @a  (B)* @b }*           // Both columnar
{ { (A)* @a  (B) @b } @row }*   // Wrap for rows
```

### Multiple Desynchronized Fields

When multiple `*`/`+` fields coexist, each produces an independent array with no alignment guarantee:

```
{ (A)* @a  (B)* @b }*
```

If iteration 1 yields `a: [1,2,3], b: [x]` and iteration 2 yields `a: [4], b: [y,z]`, the result is:

```
{ a: [1,2,3,4], b: [x,y,z] }   // lengths differ, no row correspondence
```

This is valid columnar concatenation—arrays are independent streams. If you need per-iteration grouping, wrap with `{...} @row`.

## 5. Type Unification in Alternations

Shallow unification across untagged branches:

| Scenario                    | Result        |
| --------------------------- | ------------- |
| Same capture, all branches  | Required      |
| Same capture, some branches | Optional      |
| Type mismatch               | Compile error |

```plotnik/docs/type-system.md#L140-160
[
  (A) @x
  (B) @x
]  // x: Node (required)

[
  (_ (A) @x (B) @y)
  (_ (A) @x)
]  // x: Node, y?: Node

[
  (A) @x ::string
  (B) @x
]  // ERROR: String vs Node
```

### Array Captures in Alternations

When a quantified capture appears in some branches but not others, the result is `Array | null`:

```plotnik/docs/type-system.md#L166-170
[
  (A)+ @x
  (B)
]  // x: Node[] | null
```

The missing branch emits `PushNull`, not an empty array. This distinction matters for columnar output—`null` indicates "branch didn't match" vs `[]` meaning "matched zero times."

Note the `*` vs `+` difference:

```
[ (A)+ @x  (B) ]  // x: Node[] | null  — null means B branch
[ (A)* @x  (B) ]  // x: Node[] | null  — null means B branch, [] means A matched zero times
```

In the `*` case, `null` and `[]` are semantically distinct. Check explicitly:

```typescript
if (result.x !== null) {
  // A branch matched (possibly zero times if x.length === 0)
}
```

For type conflicts, use tagged alternations:

```plotnik/docs/type-system.md#L157-160
[
    Str: (A) @x ::string
    Node: (B) @x
] @result
```

### Unification Rules

1. Primitives: exact match required
2. Arrays: element types unify; looser cardinality wins (`+` ∪ `*` → `*`)
3. Structs: identical field sets, recursively compatible
4. Enums: identical variant sets

### 1-Level Merge Only

Top-level fields merge with optionality; nested mismatches are errors:

```/dev/null/merge.txt#L1-8
// OK: top-level merge
{ x: Node, y: Node } ∪ { x: Node, z: String } → { x: Node, y?: Node, z?: String }

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

```plotnik/docs/type-system.md#L213-219
Expr = [
    Lit: (number) @value ::string
    Binary: (binary_expression
        left: (Expr) @left
        right: (Expr) @right
    )
]
```

### Requirements

```plotnik/docs/type-system.md#L226-232
Loop = (Loop)                    // ERROR: no escape path
Expr = [ Lit: (n) @n  Rec: (Expr) @e ]  // OK: Lit escapes

A = (B)
B = (A)                          // ERROR: no input consumed

A = (foo (B))
B = (bar (A))                    // OK: descends each step
```

### Scope Boundaries

Recursive definitions get automatic type boundaries:

```plotnik/docs/type-system.md#L240-241
NestedCall = (call_expression
    function: [(identifier) @name (NestedCall) @inner])
```

### Recursive Deep Search

Combines recursion with bubbling for flat output:

```plotnik/docs/type-system.md#L249-253
DeepSearch = [
    (identifier) @target
    (_ (DeepSearch)*)
]
AllIdentifiers = (program (DeepSearch)*)
```

Output: `{ target: Node[] }` — flat array regardless of tree depth.

## 7. Type Metadata

For codegen, types are named:

- **Explicit**: `@name :: TypeName`
- **Synthetic**: `{DefName}{FieldName}` (e.g., `FuncParams`), with numeric suffix on collision
