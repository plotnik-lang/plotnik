# ADR-0009: Type System

- **Status**: Proposed
- **Date**: 2025-01-14

## Context

Type inference transforms a `BuildGraph` into `TypeDef`/`TypeMember` structures (ADR-0007). This ADR formalizes the inference rules, particularly the semantics of alternations.

## Decision

### Type Universe

```
τ ::= Void              -- definition with no captures (TypeId = 0)
    | Node              -- AST node reference (TypeId = 1)
    | String            -- extracted source text (TypeId = 2)
    | Optional(τ)       -- nullable wrapper
    | ArrayStar(τ)      -- zero or more
    | ArrayPlus(τ)      -- one or more
    | Struct(fields)    -- struct with named fields
    | Enum(variants)    -- tagged union
```

### Cardinality

Cardinality describes how many values a capture produces:

| Cardinality | Notation | Wrapper     | Semantics    |
| ----------- | -------- | ----------- | ------------ |
| Required    | `1`      | none        | exactly one  |
| Optional    | `?`      | `Optional`  | zero or one  |
| Star        | `*`      | `ArrayStar` | zero or more |
| Plus        | `+`      | `ArrayPlus` | one or more  |

Cardinality propagates through nesting:

```
outer * inner = result
──────────────────────
  1   *   1   =   1
  1   *   ?   =   ?
  1   *   *   =   *
  1   *   +   =   +
  ?   *   1   =   ?
  ?   *   ?   =   ?
  ?   *   *   =   *
  ?   *   +   =   *
  *   *   1   =   *
  *   *   ?   =   *
  *   *   *   =   *
  *   *   +   =   *
  +   *   1   =   +
  +   *   ?   =   *
  +   *   *   =   *
  +   *   +   =   +
```

### Scope Rules

A **scope** is a container that collects captures into fields.

Scopes are created by:

1. **Definition root**: inherits the scope type of its root expression (see below)
2. **Captured sequence**: `{...} @name` creates a nested Struct scope
3. **Captured tagged alternation**: `[A: ... B: ...] @name` creates an Enum; each variant has its own scope
4. **Captured untagged alternation**: `[...] @name` creates a Struct; captures from branches merge

**Definition root semantics**: A definition `Foo = expr` is equivalent to capturing the root expression with the definition name. Therefore:

- `Foo = [ A: ... B: ... ]` → `Foo` is an Enum (tagged alternation at root)
- `Foo = { ... }` or `Foo = (node ...)` → `Foo` is a Struct (captures propagate to root scope)
- `Foo = (node) @x` → `Foo` is a Struct with field `x`

**Critical rule**: Tags only have effect when the alternation is captured. An _inline_ uncaptured tagged alternation behaves identically to an untagged one—captures propagate to parent scope.

### Flat Scoping Principle

Query nesting does NOT create data nesting. Intermediate structure is invisible:

```plotnik
Query = (a (b (c) @val))
```

Result type: `Struct { val: Node }` — the `(a ...)` and `(b ...)` wrappers contribute nothing.

Only explicit scope markers (`{...} @x`, `[...] @x` with tags) introduce nesting in the output type.

### Type Inference for Captures

| Pattern                       | Inferred Type        |
| ----------------------------- | -------------------- |
| `(node) @x`                   | `Node`               |
| `"literal" @x`                | `Node`               |
| `@x ::string`                 | `String`             |
| `@x ::TypeName`               | `TypeName` (nominal) |
| `{...} @x`                    | synthetic Struct     |
| `[A: ... B: ...] @x` (tagged) | Enum with variants   |
| `[...] @x` (untagged)         | merged Struct        |

### Alternation Semantics

This is the most complex part of type inference. The key insight:

> **Tags only matter when the alternation is captured.**

#### Case 1: Uncaptured Alternation (Tagged or Untagged)

Captures propagate to the parent scope. Asymmetric captures become Optional.

```plotnik
Foo = [ A: (a) @x  B: (b) @y ]
```

Despite tags, this is uncaptured. Behavior:

- `@x` appears only in branch A → propagates as `Optional(Node)`
- `@y` appears only in branch B → propagates as `Optional(Node)`
- Result: `Foo { x: Optional(Node), y: Optional(Node) }`
- Diagnostic (warning): asymmetric captures

```plotnik
Bar = [ (a) @v  (b) @v ]
```

Untagged, uncaptured. Both branches have `@v`:

- `@v` appears in all branches with type `Node` → propagates as `Node`
- Result: `Bar { v: Node }`

#### Case 2: Captured Untagged Alternation

Creates a Struct scope. Captures from branches merge into it.

```plotnik
Foo = [ (a) @x  (b) @y ] @z
```

- `@z` creates a Struct scope
- `@x` and `@y` are asymmetric → both become Optional within `@z`'s scope
- Result: `Foo { z: FooZ }` where `FooZ { x: Optional(Node), y: Optional(Node) }`

```plotnik
Bar = [ (a) @v  (b) @v ] @z
```

- `@z` creates a Struct scope
- `@v` appears in all branches → required within `@z`'s scope
- Result: `Bar { z: BarZ }` where `BarZ { v: Node }`

#### Case 3: Captured Tagged Alternation

Creates an Enum. Each variant has its own independent scope.

```plotnik
Foo = [ A: (a) @x  B: (b) @y ] @z
```

- `@z` creates an Enum because tags are present AND alternation is captured
- Variant `A` has scope with `@x: Node`
- Variant `B` has scope with `@y: Node`
- Result: `Foo { z: FooZ }` where `FooZ` is:
  ```
  Enum FooZ {
      A: FooZA { x: Node }
      B: FooZB { y: Node }
  }
  ```

### Unification Rules (for merging)

When merging captures across untagged alternation branches:

```
unify(τ, τ) = τ
unify(Node, Node) = Node
unify(String, String) = String
unify(Struct(f₁), Struct(f₂)) = Struct(f₁) if f₁ = f₂
unify(τ₁, τ₂) = ⊥ (error)
```

### Cardinality Join (for merging)

When the same capture appears in multiple branches with different cardinalities:

```
        +
       /|\
      * | (arrays collapse to *)
       \|
        ?
        |
        1
```

| Left | Right | Join |
| ---- | ----- | ---- |
| 1    | 1     | 1    |
| 1    | ?     | ?    |
| 1    | \*    | \*   |
| 1    | +     | +    |
| ?    | ?     | ?    |
| ?    | \*    | \*   |
| ?    | +     | \*   |
| \*   | \*    | \*   |
| \*   | +     | \*   |
| +    | +     | +    |

### Cardinality Lifting Coercion

When cardinality join produces an array type (`*` or `+`) but a branch has scalar cardinality (`1` or `?`), the compiler inserts coercion effects to wrap the scalar in a singleton array.

| Original | Lifted to  | Effect transformation                                                                       |
| -------- | ---------- | ------------------------------------------------------------------------------------------- |
| `1`      | `*` or `+` | `CaptureNode` → `StartArray, CaptureNode, PushElement, EndArray`                            |
| `?`      | `*`        | absent → `StartArray, EndArray`; present → `StartArray, CaptureNode, PushElement, EndArray` |

This ensures the materializer always receives homogeneous values matching the declared type.

Example:

```plotnik
Items = [ (single) @item  (multi { (x)+ @item }) ]
```

Branch 1 has `@item: 1`, branch 2 has `@item: +`. Join is `+`. Branch 1's effects are lifted:

```
// Before lifting:
CaptureNode, Field("item")

// After lifting:
StartArray, CaptureNode, PushElement, EndArray, Field("item")
```

### Quantifier-Induced Scope (QIS)

When a quantified expression contains multiple captures, they must stay coupled per-iteration. QIS creates an implicit scope to preserve this structural relationship.

**Trigger**: Quantifier `Q ∈ {*, +, ?}` applied to expression `E`, where `E` has **≥2 propagating captures** (captures not absorbed by inner scopes).

**Mechanism**: QIS creates an implicit scope around `E`. Captures propagate to this scope (not the parent), forming a struct element type.

**Containers**: Any expression can trigger QIS:

- Node: `(node ...)Q`
- Sequence: `{...}Q`
- Alternation: `[...]Q`

**Naming**:

| Context                      | Element Type Name                   |
| ---------------------------- | ----------------------------------- |
| At definition root           | `{Def}Item`                         |
| Explicit capture `E Q @name` | `{Parent}{Name}`                    |
| Neither                      | **Error**: require explicit `@name` |

**Result Type**:

| Q   | Result                   |
| --- | ------------------------ |
| `*` | `ArrayStar(ElementType)` |
| `+` | `ArrayPlus(ElementType)` |
| `?` | `Optional(ElementType)`  |

**Interior rules**: Standard type inference within the implicit scope:

- Uncaptured alternations (tagged or not): asymmetric captures → Optional
- Captured tagged alternations: Enum with variant scopes

**Non-trigger** (≤1 propagating capture): No QIS. Single capture propagates with cardinality multiplication `Q × innerCard`.

**Examples**:

```plotnik
// Node as container - keeps name/body paired
Functions = (function_declaration
    name: (identifier) @name
    body: (block) @body
)*
// → Functions = ArrayStar(FunctionsItem)
// → FunctionsItem = { name: Node, body: Node }

// Alternation in quantified sequence
Foo = { [ (a) @x  (b) @y ] }*
// → Foo = ArrayStar(FooItem)
// → FooItem = { x: Optional(Node), y: Optional(Node) }

// Tagged but uncaptured (tags ignored, same result)
Bar = { [ A: (a) @x  B: (b) @y ] }*
// → Bar = ArrayStar(BarItem)
// → BarItem = { x: Optional(Node), y: Optional(Node) }

// Tagged AND captured (no QIS - single propagating capture)
Baz = { [ A: (a) @x  B: (b) @y ] @choice }*
// → Baz = ArrayStar(BazChoice)
// → BazChoice = Enum { A: { x: Node }, B: { y: Node } }

// Nested with explicit capture
Outer = (parent { [ (a) @x  (b) @y ] }* @items)
// → Outer = { items: ArrayStar(OuterItems) }
// → OuterItems = { x: Optional(Node), y: Optional(Node) }

// Single capture - no QIS, standard rules
Single = { (a) @item }*
// → Single = { item: ArrayStar(Node) }

// Error: QIS triggered but no capture, not at root
Bad = (parent { [ (a) @x  (b) @y ] }* (other) @z)
// → Error: quantified expression with multiple captures requires @name
```

### Missing Field Rule

If a capture appears in some branches but not all, the field becomes `Optional` (or `*` if original was array).

This is intentional: users can have common fields be required across all branches, while branch-specific fields become optional.

### Synthetic Naming

Types without explicit `::Name` receive synthetic names:

| Context              | Pattern           | Example      |
| -------------------- | ----------------- | ------------ |
| Definition root      | `{DefName}`       | `Func`       |
| Captured sequence    | `{Def}{Capture}`  | `FuncParams` |
| Captured alternation | `{Def}{Capture}`  | `FuncBody`   |
| Enum variant payload | `{Enum}{Variant}` | `FuncBodyOk` |

Collision resolution: append numeric suffix (`Foo`, `Foo2`, `Foo3`, ...).

### Error Conditions

| Condition                            | Severity | Recovery                      | Diagnostic Kind (future)       |
| ------------------------------------ | -------- | ----------------------------- | ------------------------------ |
| Type mismatch in untagged alt        | Error    | Use `TYPE_INVALID`, continue  | `TypeMismatchInAlt`            |
| Duplicate capture in same scope      | Error    | Keep first, ignore duplicates | `DuplicateCapture`             |
| Empty definition (no captures)       | Info     | Type is `Void` (TypeId = 0)   | (no diagnostic)                |
| Inline uncaptured tagged alternation | Warning  | Treat as untagged             | `UnusedBranchLabels`           |
| QIS without capture (not at root)    | Error    | Cannot infer element type     | `MultiCaptureQuantifierNoName` |

The last warning applies only to literal tagged alternations, not references. If `Foo = [ A: ... ]` is used as `(Foo)`, no warning—the user intentionally reuses a definition. But `(parent [ A: ... B: ... ])` inline without capture likely indicates a forgotten `@name`.

## Examples

### Example 1: Captured Sequence

```plotnik
Foo = (foo {(bar) @bar} @baz)
```

- `@bar` captures `(bar)` → `Node`
- `@baz` captures the sequence containing `@bar` → creates scope
- Types:
  - `@bar: Node`
  - `@baz: FooBaz { bar: Node }`
  - `Foo: { baz: FooBaz }`

### Example 2: Uncaptured Sequence

```plotnik
Foo = (foo {(bar) @bar})
```

- `@bar` captures `(bar)` → `Node`
- Sequence `{...}` is NOT captured → `@bar` propagates to `Foo`'s scope
- Types:
  - `Foo: { bar: Node }`

### Example 3: Tagged Alternation at Definition Root

```plotnik
Result = [
    Ok: (value) @val
    Err: (error) @msg ::string
]
```

- Tagged alternation at definition root → `Result` is an Enum
- Types:
  - `Result: Enum { Ok: ResultOk, Err: ResultErr }`
  - `ResultOk: { val: Node }`
  - `ResultErr: { msg: String }`

### Example 4: Tagged Alternation (Inline, Uncaptured)

```plotnik
Foo = (parent [
    Ok: (value) @val
    Err: (error) @msg ::string
])
```

- Tagged alternation is inline and uncaptured → tags ignored, behaves like untagged
- `@val` only in Ok branch → `Optional(Node)`
- `@msg` only in Err branch → `Optional(String)`
- Types:
  - `Foo: { val: Optional(Node), msg: Optional(String) }`
- Diagnostic: warning (inline uncaptured tagged alternation)

### Example 5: Cardinality in Alternation

```plotnik
Items = [ (single) @item  (multi { (x)+ @item }) ]
```

- Branch 1: `@item` cardinality `1`, type `Node`
- Branch 2: `@item` cardinality `+`, type `Node`
- Join: cardinality `+` (both present, LUB of `1` and `+`)
- Types:
  - `Items: { item: ArrayPlus(Node) }`

### Example 6: Nested Quantifier

```plotnik
Funcs = (module { (function)* @fns })
```

- `@fns` has cardinality `*` from quantifier
- Sequence not captured → propagates to root
- Types:
  - `Funcs: { fns: ArrayStar(Node) }`

## Consequences

**Positive**:

- Explicit rules enable deterministic inference
- "Tags only matter when captured" is a simple mental model
- Warning on asymmetric captures catches likely bugs
- Definition root inherits type naturally—no wrapper structs for top-level enums

**Negative**:

- LUB cardinality join can lose precision

**Alternatives Considered**:

- Error on uncaptured tagged alternations (rejected: too restrictive for incremental development)
- Definition root always Struct (rejected: forces wrapper types for enums, e.g., `struct Expr { val: ExprEnum }` instead of `enum Expr`)
