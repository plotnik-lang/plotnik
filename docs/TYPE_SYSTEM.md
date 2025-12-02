-------------------------------------------------------------------------------------------
NOTE: THIS DOCUMENT IS NOT A PART OF SPECIFICATION, IT'S EXPERIMENTAL AND WILL BE REWRITTEN
-------------------------------------------------------------------------------------------

# Plotnik Type System

This document specifies how types are inferred from Plotnik queries. It serves both as a formal specification and a practical user guide with extensive examples.

---

## Type Algebra

### Base Types

```
T ::= Node                              -- tree-sitter node reference
    | string                            -- extracted text (via :: string)
    | { f₁: T₁, ..., fₙ: Tₙ }           -- record (struct)
    | T?                                -- optional (zero or one)
    | T*                                -- array (zero or more)
    | T+                                -- non-empty array (one or more)
    | <L₁: T₁, ..., Lₙ: Tₙ>             -- tagged union (labeled alternation)
    | Name                              -- named type reference
    | ()                                -- unit (no captures)
```

### Cardinality

| Quantifier | Type modifier | Rust          | TypeScript       |
| ---------- | ------------- | ------------- | ---------------- |
| (none)     | `T`           | `T`           | `T`              |
| `?`        | `T?`          | `Option<T>`   | `T \| undefined` |
| `*`        | `T*`          | `Vec<T>`      | `T[]`            |
| `+`        | `T+`          | `(T, Vec<T>)` | `[T, ...T[]]`    |

Non-greedy variants (`??`, `*?`, `+?`) have the same types as their greedy counterparts.

### Type Equivalences

```
()? ≡ ()                    -- optional unit is unit
()* ≡ ()                    -- array of unit is unit
()+ ≡ ()                    -- non-empty array of unit is unit
{}  ≡ ()                    -- empty record is unit
{ f: () } ≡ ()              -- record with only unit fields collapses
{ f: (), g: T } ≡ { g: T }  -- unit fields are eliminated
T | T ≡ T                   -- union with itself (idempotence)
```

---

## Core Inference Model

Type inference tracks two things for each expression:

- **type**: the type when this expression is captured as a whole
- **bindings**: captures that bubble up to the enclosing scope if NOT captured

### The Duality Explained

Every expression has both a "type" (what it is if you capture it) and "bindings" (what escapes from inside it if you don't). This is the fundamental insight.

**Example 1: Simple tree node**

```
(identifier)
```

- `type = Node` — if captured, you get a Node
- `bindings = ∅` — nothing bubbles up (no inner captures)

**Example 2: Tree with a capture inside**

```
(function_declaration name: (identifier) @name)
```

- `type = Node` — the tree itself is a Node
- `bindings = { name: Node }` — `@name` bubbles up to enclosing scope

**Example 3: Capturing a tree with inner captures**

```
(function_declaration name: (identifier) @name) @func
```

- The inner expression has `bindings = { name: Node }`
- `@func` captures the tree node itself (adds `func: Node`)
- Inner bindings continue to bubble up alongside the new capture
- Result: `bindings = { func: Node, name: Node }` — FLAT, both bubble up

**Example 4: When DO we get nesting?**

Only `{...}` sequences and `[...]` alternations create scope boundaries when captured:

```
{(function_declaration name: (identifier) @name)} @func
```

- The `{...}` is a sequence (scope boundary when captured)
- `@func` captures the sequence, absorbing inner bindings
- Result: `bindings = { func: { name: Node } }` — nested

Compare:

```
(fn name: (id) @name) @func         → { func: Node, name: Node }      -- flat
{(fn name: (id) @name)} @func       → { func: { name: Node } }        -- nested
```

**Example 5: Multiple captures at same level**

```
(binary_expression
  left: (identifier) @left
  right: (number) @right) @expr
```

All three captures bubble up to the same scope:

- Result: `{ expr: Node, left: Node, right: Node }`

### Why This Design?

The alternative would be to nest captures whenever a parent is captured. But that creates deeply nested types:

```
// hypothetical "captures nest" approach (NOT how Plotnik works)
(module
  (function
    name: (identifier) @name
  ) @fn
) @mod // -> { mod: { fn: { name: Node } } }

// Plotnik's actual behavior
(module
  (function
    name: (identifier) @name
  ) @fn
) @mod // -> { mod: Node, fn: Node, name: Node }
```

You control nesting explicitly by using grouping constructs (`{...}` and `[...]`):

```
// explicit nesting via sequence
{
  (module
    (function
      name: (identifier) @name
    ) @fn
  )
} @mod // -> { mod: { fn: Node, name: Node } }

// even more nesting
{
  (module
    {
      (function
        name: (identifier) @name
      )
    } @fn
  )
} @mod // -> { mod: { fn: { name: Node } } }
```

### Scope Boundaries

Bindings bubble up until they hit one of these:

1. **Root of a definition** — top-level scope collects everything
2. **Captured sequence** — `{...} @name` absorbs inner bindings
3. **Captured alternation** — `[...] @name` absorbs inner bindings
4. **Tagged alternation branches** — each `Label: ...` is its own scope

**NOT scope boundaries:**

- Captured trees: `(node) @name` — inner bindings continue to bubble up
- Captured wildcards: `(_) @name` — just adds a capture
- Field constraints: `field: expr` — transparent

**Example: Root as boundary**

```
FunctionDef = (function name: (identifier) @name body: (_) @body)
```

Both `@name` and `@body` bubble up to the root. The definition's type is `{ name: Node, body: Node }`.

**Example: Captured tree is NOT a boundary**

```
(function name: (identifier) @name body: (_) @body) @func
```

All three captures bubble up together: `{ func: Node, name: Node, body: Node }`

**Example: Captured sequence IS a boundary**

```
{
  (comment)* @comments
  {
    (function name: (identifier) @name)
  } @func
}
```

- Inner `{...} @func` absorbs `@name`
- `@comments` and `@func` bubble up to root
- Output: `{ comments: Node[], func: { name: Node } }`

**Example: Captured alternation IS a boundary**

```
[
  (identifier) @id
  (number) @num
] @value
```

The `[...] @value` absorbs inner bindings: `{ value: { id?: Node, num?: Node } }`

Without the capture on alternation:

```
[
  (identifier) @id
  (number) @num
]
```

Fields bubble up: `{ id?: Node, num?: Node }`

**Example: Tagged branches as boundaries**

```
[
  Func: (function name: (identifier) @name)
  Class: (class name: (identifier) @name)
] @item
```

Each branch has its own scope. The `@name` in `Func:` and the `@name` in `Class:` are independent — they don't merge.

Output: `{ item: <Func: { name: Node }, Class: { name: Node }> }`

**Pattern: Capturing Both Node and Structure**

A common need: you want the parent node itself AND structured access to its children. Trees don't create scope boundaries, so inner captures bubble up flat. The solution: wrap in a sequence.

Without sequence (flat):

```
(function_declaration
  name: (identifier) @name
  parameters: (formal_parameters (identifier)* @params)) @fn
```

Output: `{ fn: Node, name: Node, params: Node[] }` — all flat, no grouping.

With sequence (structured):

```
{
  (function_declaration
    name: (identifier) @name
    parameters: (formal_parameters (identifier)* @params)) @fn_node
} @fn :: FunctionDecl
```

Output: `{ fn: FunctionDecl }` where `FunctionDecl = { fn_node: Node, name: Node, params: Node[] }`

The sequence creates a scope boundary. Now you have:

- `@fn_node` — the matched tree-sitter node (for span, kind, etc.)
- `@name`, `@params` — structured access to children
- `@fn` — bundles everything into a named type

Each capture has one job. The verbosity pays for explicitness — you always know where each field lands by looking at capture positions.

---

## Inference Rules

### Notation

- `Γ` — environment mapping definition names to types
- `infer(E) → (type, bindings)` — inference judgment
- `merge(B₁, B₂)` — combine binding sets (same-name bindings must have same type)

### Terminals

```
infer((node_type children...)) = (Node, merge(bindings(c) for c in children))
infer((_)) = (Node, ∅)
infer(_) = (Node, ∅)
infer("literal") = (Node, ∅)
infer(.) = (Node, ∅)           -- anchor
infer(!field) = (Node, ∅)      -- negated field
```

Trees produce `Node` as their type. Child captures bubble up.

**Example: Tree with children**

```
(binary_expression left: (_) @left right: (_) @right)
```

Step by step:

1. `infer((_) @left)` = `({ left: Node }, { left: Node })`
2. `infer((_) @right)` = `({ right: Node }, { right: Node })`
3. `infer((binary_expression ...))` = `(Node, merge({ left: Node }, { right: Node }))`
4. Final: `(Node, { left: Node, right: Node })`

**Example: Nested trees**

```
(call function: (member object: (_) @obj property: (_) @prop))
```

1. Inner `(member ...)` produces `(Node, { obj: Node, prop: Node })`
2. Outer `(call ...)` bubbles them up: `(Node, { obj: Node, prop: Node })`
3. Nesting in the pattern doesn't create nesting in the type

### Named Expression Reference

```
infer((Name)) = (Name, ∅)
```

References produce the named type, not the expanded structure.

**Example: Using a named expression**

```
Expr = (binary_expression left: (_) @left right: (_) @right)

(return_statement (Expr) @value)
```

1. `Expr` is defined as `{ left: Node, right: Node }`
2. `infer((Expr))` = `(Expr, ∅)` — returns the type name, no bubbling
3. `infer((Expr) @value)` = `({ value: Expr }, { value: Expr })`

Final output: `{ value: Expr }` where `Expr = { left: Node, right: Node }`

**Why no bubbling?** Named expressions are abstraction boundaries. Their internal structure is encapsulated.

> **Key Design Point:** This encapsulation is intentional and has important implications:
>
> - `(Name)` without capture = pattern matching only, no data extraction
> - `(Name) @x` = access the named expression's data via `x`
>
> Named expressions separate two concerns:
>
> - **Structural reuse:** Define a pattern once, use it everywhere
> - **Data extraction:** Explicitly choose what to capture
>
> This prevents accidental field bubbling through recursive definitions and avoids name collisions when the same named expression is used multiple times.

### Field Constraint

```
infer(field: E) = infer(E)
```

Field constraints are transparent to typing.

**Example:**

```
(function_declaration name: (identifier) @name)
(function_declaration (identifier) @name)
// both produce the same type
```

Both produce `(Node, { name: Node })`. The `name:` constraint affects matching, not typing.

### Capture

The capture rule depends on whether the captured expression is a scope boundary:

```
infer(E @name) =
  let (innerType, innerBindings) = infer(E)
  let capturedType = if innerType = () then Node else innerType

  if E is Seq or E is Alt:
    -- SCOPE BOUNDARY: absorb inner bindings into a struct
    let T = if innerBindings = ∅ then capturedType else { innerBindings }
    ({ name: T }, { name: T })
  else:
    -- NOT a scope boundary: capture + inner bindings both bubble up
    (capturedType, merge({ name: capturedType }, innerBindings))
```

**Example 1: Capturing a bare node (no inner bindings)**

```
(identifier) @name
```

1. `infer((identifier))` = `(Node, ∅)`
2. Tree is not a scope boundary, but `innerFields = ∅`
3. Result: `({ name: Node }, { name: Node })`

**Example 2: Capturing a tree with inner captures (NOT a scope boundary)**

```
(function name: (identifier) @name body: (_) @body) @func
```

1. `infer((function ...))` = `(Node, { name: Node, body: Node })`
2. Tree is NOT a scope boundary — bindings continue bubbling
3. Add `func: Node` to the bindings
4. Result: `(Node, { func: Node, name: Node, body: Node })`

All three captures end up at the same level — flat!

**Example 3: Capturing a sequence (IS a scope boundary)**

```
{(function name: (identifier) @name body: (_) @body)} @func
```

1. `infer({...})` = `((), { name: Node, body: Node })`
2. Sequence IS a scope boundary — absorb inner bindings
3. Result: `({ func: { name: Node, body: Node } }, { func: { name: Node, body: Node } })`

Nested structure because `{...}` creates a scope.

**Example 4: Capturing an alternation (IS a scope boundary)**

```
[(identifier) @id (number) @num] @value
```

1. `infer([...])` = `(Node, { id?: Node, num?: Node })`
2. Alternation IS a scope boundary — absorb inner bindings
3. Result: `({ value: { id?: Node, num?: Node } }, { value: { id?: Node, num?: Node } })`

**Example 5: Unit promotion**

```
{ (a) (b) } @group
```

1. `infer({ (a) (b) })` = `((), ∅)` — no captures inside
2. Sequence is scope boundary, but `innerFields = ∅`
3. Promote: `() → Node`
4. Result: `({ group: Node }, { group: Node })`

Rationale: if you capture something with no structure, you want the matched node.

### Capture with Type Annotation

```
infer(E @name :: string) =
  -- :: string always extracts text, bindings still bubble per scope rules
  if E is Seq or E is Alt:
    ({ name: string }, { name: string })
  else:
    let (_, innerBindings) = infer(E)
    (string, merge({ name: string }, innerBindings))

infer(E @name :: TypeName) =
  -- :: TypeName names the type for codegen, follows same scope rules
  ... (same logic as capture, but type gets the name)
```

**Example: Text extraction on bare node**

```
(identifier) @name :: string
```

Result: `{ name: string }` — the node's text is extracted.

**Example: Text extraction on tree with inner captures**

```
(function name: (identifier) @fn_name) @func :: string
```

Tree is not a scope boundary, so inner bindings bubble up:
Result: `{ func: string, fn_name: Node }` — both at same level

**Example: Type naming on sequence**

```
{(function name: (identifier) @name body: (_) @body)} @func :: FunctionDef
```

Sequence IS a scope boundary, so inner bindings are absorbed:
Result: `{ func: FunctionDef }` where `FunctionDef = { name: Node, body: Node }`

### Quantifier

```
infer(E?) = let (v, f) = infer(E) in (v?, apply_optional(f))
infer(E*) = let (v, f) = infer(E) in (v*, apply_array(f))
infer(E+) = let (v, f) = infer(E) in (v+, apply_nonempty(f))
```

Cardinality applies to both the type and all bubbling bindings.

**Example 1: Optional quantifier**

```
(decorator)? @dec
```

1. `infer((decorator))` = `(Node, ∅)`
2. `infer((decorator)?)` = `(Node?, ∅)`
3. `infer((decorator)? @dec)` = `({ dec: Node? }, { dec: Node? })`

**Example 2: Array quantifier on tree with inner captures**

```
(parameter name: (identifier) @name)* @params
```

1. `infer((parameter ...))` = `(Node, { name: Node })`
2. Apply `*`: type becomes `Node[]`, bindings become `{ name: Node[] }`
3. Capture `@params` on a tree (NOT a scope boundary)
4. Add `params: Node[]` to bubbling bindings
5. Result: `{ params: Node[], name: Node[] }`

Both `@params` and `@name` bubble up. Each is an array because of `*`.

**Example 2b: Array quantifier on sequence with inner captures**

```
{(parameter name: (identifier) @name)}* @params
```

1. `infer({...})` = `((), { name: Node })`
2. Apply `*`: type becomes `()[]`, bindings become `{ name: Node[] }`
3. Capture `@params` on a sequence (IS a scope boundary)
4. Absorb inner bindings: `{ name: Node }` per element
5. Result: `{ params: { name: Node }[] }`

Array of structs because `{...}` creates scope, each iteration is a struct.

**Example 3: Quantifier on uncaptured expression with inner captures**

```
(parameter name: (identifier) @name)*
```

Here the quantifier is on the tree, not on a capture. The inner `@name` bubbles up:

1. One match produces `bindings = { name: Node }`
2. `*` quantifier: `apply_array({ name: Node }) = { name: Node[] }`
3. Result: `(Node[], { name: Node[] })`

Output type: `{ name: Node[] }` — all the names collected into one array.

### Cardinality Combination

When quantifiers stack (field already has cardinality, outer quantifier applied):

```
combine(One, q)      = q
combine(Optional, q) = if q = One then Optional else Array
combine(Array, _)    = Array
combine(NonEmpty, q) = if q = One then NonEmpty else Array
```

**Example: Nested quantifiers**

```
{ (x) @item }+ @batch*
```

1. `(x) @item` produces `{ item: Node }` (One)
2. `{ ... }+ @batch` wraps in non-empty array: `{ batch: { item: Node }+ }` (NonEmpty)
3. Outer `*` combines: `combine(NonEmpty, Array) = Array`
4. Result: `{ batch: { item: Node }+* }` — array of non-empty arrays

**Example: Optional over optional**

```
((comment)? @c)?
```

1. `infer((comment)?)` = `(Node?, ∅)`
2. `infer((comment)? @c)` = `({ c: Node? }, { c: Node? })`
3. Outer `?` applies to the captured expression
4. `apply_optional({ c: Node? })` = `{ c: Node?? }`
5. `T??` ≡ `T?` (optional of optional collapses), so result: `{ c: Node? }`

The semantics:

- If outer `?` doesn't match: no `c` at all
- If outer matches but inner `(comment)?` doesn't: `c` is `None`/`undefined`
- If both match: `c` has a value

Both cases where `c` is absent collapse into the same optionality.

**Example: Array over optional**

```
((comment)? @c)*
```

1. Inner: `{ c: Node? }`
2. Outer `*`: `apply_array({ c: Node? })` = `{ c: Node?[] }`
3. But `combine(Optional, Array) = Array`, so: `{ c: Node[] }`

Each match either contributes a node or nothing. The array collects all the nodes that were present.

**Note:** The `combine` table shows what happens when an already-modified field gets another modifier. In practice, nested optionals collapse (`T??` → `T?`) and anything combined with array becomes array (since you can't distinguish "missing" from "present but empty" in an array context).

### Sequence

```
infer({ E₁ E₂ ... }) =
  let bindings = merge(infer(E₁).bindings, infer(E₂).bindings, ...)
  ({ bindings }, bindings)
```

Sequences merge child bindings. When captured, they become a struct.

**Example 1: Simple sequence**

```
{ (a) @x (b) @y }
```

1. `infer((a) @x)` = `({ x: Node }, { x: Node })`
2. `infer((b) @y)` = `({ y: Node }, { y: Node })`
3. Merge bindings: `{ x: Node, y: Node }`
4. Result: `({ x: Node, y: Node }, { x: Node, y: Node })`

**Example 2: Captured sequence (scope boundary)**

```
{ (a) @x (b) @y } @pair
```

1. Inner sequence has `bindings = { x: Node, y: Node }`
2. Sequence IS a scope boundary — `@pair` absorbs inner bindings
3. Result: `({ pair: { x: Node, y: Node } }, { pair: { x: Node, y: Node } })`

Compare to tree capture (not a boundary):

```
(a (b) @x (c) @y) @pair
```

Result: `{ pair: Node, x: Node, y: Node }` — all flat

**Example 3: Nested sequences**

```
{
  { (a) @a } @first
  { (b) @b } @second
}
```

1. Inner `{ (a) @a } @first`: `bindings = { first: { a: Node } }`
2. Inner `{ (b) @b } @second`: `bindings = { second: { b: Node } }`
3. Merge: `{ first: { a: Node }, second: { b: Node } }`

### Alternation (Unlabeled / Merge Style)

```
infer([ E₁ E₂ ... ]) =
  let branches = [infer(E₁), infer(E₂), ...]
  if all(b.bindings = ∅ for b in branches):
    (Node, ∅)
  else:
    let merged = merge_optional(branches.map(b → b.bindings))
    ({ merged }, merged)
```

**Example 1: All branches are bare (no captures inside)**

```
[(identifier) (number) (string)]
```

All branches have `bindings = ∅`, so result is `(Node, ∅)`.

When captured:

```
[(identifier) (number) (string)] @value
```

Result: `{ value: Node }` — simple node capture.

**Example 2: Branches with captures**

```
[
  (identifier) @name
  (number) @num
]
```

Branch 1: `bindings = { name: Node }`
Branch 2: `bindings = { num: Node }`

Merge:

- `name` appears in branch 1 only → optional: `name: Node?`
- `num` appears in branch 2 only → optional: `num: Node?`

Result: `({ name: Node?, num: Node? }, { name: Node?, num: Node? })`

**Example 3: Same capture in all branches**

```
[
  (identifier) @x
  (number) @x
]
```

Branch 1: `bindings = { x: Node }`
Branch 2: `bindings = { x: Node }`

Merge:

- `x` appears in all branches with same type → required: `x: Node`

Result: `({ x: Node }, { x: Node })`

**Example 4: Partial overlap**

```
[
  (binary left: (_) @left right: (_) @right)
  (unary operand: (_) @left)
]
```

Branch 1: `bindings = { left: Node, right: Node }`
Branch 2: `bindings = { left: Node }`

Merge:

- `left` in all branches → required: `left: Node`
- `right` in branch 1 only → optional: `right: Node?`

Result: `{ left: Node, right: Node? }`

**Example 5: Type mismatch (ERROR)**

```
[
  (identifier) @x :: string
  (number) @x
]
```

Branch 1: `x: string`
Branch 2: `x: Node`

Error: `@x` has different types across branches.

### Alternation (Tagged Style)

```
infer([ L₁: E₁  L₂: E₂ ... ]) =
  let variants = [
    (L₁, infer(E₁).bindings),
    (L₂, infer(E₂).bindings),
    ...
  ]
  (⟨L₁: {bindings₁}, L₂: {bindings₂}, ...⟩, ∅)
```

Tagged alternations:

- Produce a discriminated union
- Each branch has its own scope (bindings don't bubble up)
- Must be captured to be useful

**Example 1: Basic tagged alternation**

```
[
  Ident: (identifier) @name
  Num: (number) @value
] @expr
```

- `Ident` branch: `bindings = { name: Node }`
- `Num` branch: `bindings = { value: Node }`

Result: `{ expr: <Ident: { name: Node }, Num: { value: Node }> }`

**Example 2: Different fields per branch**

```
[
  Binary: (binary left: (_) @left op: _ @op right: (_) @right)
  Unary: (unary op: _ @op operand: (_) @operand)
  Literal: (number) @value
] @expr
```

Result:

```
expr: <>
  Binary: { left: Node, op: Node, right: Node },
  Unary: { op: Node, operand: Node },
  Literal: { value: Node }
>
```

Each branch is independent. `@op` in `Binary` and `@op` in `Unary` are separate fields.

**Example 3: Empty branch**

```
[
  Some: (value) @val
  None: (none)
] @option
```

Result: `{ option: <Some: { val: Node }, None: { }> }`

The `None` variant has an empty struct (unit).

### Named Definition

```
Name = E
────────────────────────────────
Γ[Name] = infer(E).value
```

Definitions create named types. When `(Name)` is referenced, it produces `Ref(Name)`.

**Example: Definition type**

```
BinOp = (binary left: (_) @left right: (_) @right)
```

`infer((binary ...))` = `(Node, { left: Node, right: Node })`

The definition's type comes from root bindings: `BinOp = { left: Node, right: Node }`

**Example: Reference in use**

```
BinOp = (binary left: (_) @left right: (_) @right)

(return (BinOp) @expr)
```

`infer((BinOp))` = `(BinOp, ∅)` — the named type, no field bubbling.

Result: `{ expr: BinOp }`

---

## Entry Point

The query's output type comes from:

- The last unnamed definition, or
- The last named definition's body if no unnamed exists

```
entry_type(root) =
  let (type, bindings) = infer(entry_expr)
  if bindings = ∅: type
  else: { bindings }
```

**Example 1: Unnamed entry point**

```
Expr = [(identifier) (number)]
(assignment left: (Expr) @left right: (Expr) @right)
```

The last line is unnamed, so it's the entry point.
Output type: `{ left: Expr, right: Expr }`

**Example 2: No unnamed definition**

```
Expr = [(identifier) (number)]
Assign = (assignment left: (Expr) @left right: (Expr) @right)
```

Last definition is `Assign`, so output type is `Assign = { left: Expr, right: Expr }`.

---

## Examples: Step-by-Step Traces

### Example A: Function Extraction

Query:

```
(function_declaration
  name: (identifier) @name :: string
  parameters: (formal_parameters (parameter)* @params)
  body: (statement_block) @body)
```

Trace:

1. `(identifier) @name :: string` → `({ name: string }, { name: string })`
2. `(parameter)*` → `(Node[], ∅)` (no captures inside)
3. `(parameter)* @params` → `({ params: Node[] }, { params: Node[] })`
4. `(formal_parameters ...)` → bubbles params: `(Node, { params: Node[] })`
5. `(statement_block) @body` → `({ body: Node }, { body: Node })`
6. `(function_declaration ...)` → merge all: `(Node, { name: string, params: Node[], body: Node })`

Output type:

```typescript
{ name: string, params: Node[], body: Node }
```

### Example B: If-Else Statement

Query:

```
(if_statement
  condition: (_) @cond
  consequence: (_) @then
  alternative: (_)? @else)
```

Trace:

1. `(_) @cond` → `({ cond: Node }, { cond: Node })`
2. `(_) @then` → `({ then: Node }, { then: Node })`
3. `(_)?` → `(Node?, ∅)`
4. `(_)? @else` → `({ else: Node? }, { else: Node? })`
5. Merge: `{ cond: Node, then: Node, else: Node? }`

Output type:

```typescript
{ cond: Node, then: Node, else?: Node }
```

### Example C: Recursive Expression

Query:

```
Expr = [
  Lit: (number) @value :: string
  Var: (identifier) @name :: string
  Bin: (binary_expression
    left: (Expr) @left
    operator: _ @op :: string
    right: (Expr) @right)
]
```

Trace for `Bin` branch:

1. `(Expr) @left` → `({ left: Expr }, { left: Expr })`
2. `_ @op :: string` → `({ op: string }, { op: string })`
3. `(Expr) @right` → `({ right: Expr }, { right: Expr })`
4. Merge: `{ left: Expr, op: string, right: Expr }`

Full type:

```typescript
type Expr =
  | { tag: "Lit"; value: string }
  | { tag: "Var"; name: string }
  | { tag: "Bin"; left: Expr; op: string; right: Expr };
```

### Example D: Nested Groups vs Tree Captures

**Query with nested sequences (creates nesting):**

```
{
  {
    (decorator) @decorator
    (function_declaration name: (identifier) @fn_name)
  } @decorated_fn
}* @functions
```

Trace:

1. `(decorator) @decorator` → `{ decorator: Node }`
2. `(identifier) @fn_name` → `{ fn_name: Node }`
3. Inner `{...}` is sequence — scope boundary, absorbs bindings
4. `@decorated_fn` captures: `{ decorated_fn: { decorator: Node, fn_name: Node } }`
5. Outer `{...}*` is also sequence, absorbs on capture
6. `@functions` captures: `{ functions: { decorated_fn: { decorator: Node, fn_name: Node } }[] }`

Output type:

```typescript
{
  functions: {
    decorated_fn: {
      decorator: Node,
      fn_name: Node
    }
  }[]
}
```

**Contrast: Tree captures (stays flat):**

```
(function_declaration
  (decorator)* @decorators
  name: (identifier) @fn_name
  body: (_) @body)* @functions
```

Trace:

1. All captures are on trees, not sequences
2. Trees are NOT scope boundaries
3. All bindings bubble up together

Output type:

```typescript
{ functions: Node[], decorators: Node[], fn_name: Node[], body: Node[] }
```

Flat! To get nesting, wrap in sequences:

```
{
  (function_declaration
    (decorator)* @decorators
    name: (identifier) @fn_name
    body: (_) @body)
}* @functions
```

Output: `{ functions: { decorators: Node[], fn_name: Node, body: Node }[] }`

### Example E: Merge vs Tagged Alternation

**Merge style, uncaptured alternation:**

```
[
  (assignment left: (identifier) @target)
  (call function: (identifier) @target)
]
```

Both branches have `@target`, same type. Fields bubble up.

Output: `{ target: Node }` — required field, flat.

**Merge style, captured alternation (scope boundary):**

```
[
  (assignment left: (identifier) @target)
  (call function: (identifier) @target)
] @stmt
```

Alternation IS a scope boundary when captured.

Output: `{ stmt: { target: Node } }` — nested.

**Tagged style** (labeled):

```
[
  Assign: (assignment left: (identifier) @target)
  Call: (call function: (identifier) @func)
] @stmt
```

Branches are independent, different field names.

Output:

```typescript
type Stmt = { tag: "Assign"; target: Node } | { tag: "Call"; func: Node };
```

### Example F: Quantifier on Sequence vs Tree

**Quantifier on sequence (scope boundary):**

```
(array {(element) @item}+ @items)
```

Trace:

1. `(element) @item` → `({ item: Node }, { item: Node })`
2. `{...}` sequence has bindings `{ item: Node }`
3. `{...}+`: value becomes array, each element is a struct
4. `@items` captures sequence (scope boundary): absorbs inner bindings

Output: `{ items: { item: Node }[] }` (non-empty array of structs)

**Quantifier on tree (NOT a scope boundary):**

```
(array (element @item)+ @items)
```

Trace:

1. `(element) @item` → tree with capture
2. Tree is NOT a scope boundary
3. Both `@item` and `@items` bubble up together

Output: `{ items: Node[], item: Node[] }` — both flat arrays

**Uncaptured sequence:**

```
(array {(element) @item}+)
```

No capture on the sequence, so bindings bubble up: `{ item: Node[] }` (non-empty).

---

## Design Decisions

### 1. No captures → Node

Expressions without captures have type `()`. When captured, `()` becomes `Node`:

```
(binary_expression) @x         -- x: Node
{ (a) (b) } @x                 -- x: Node (no inner captures)
[(identifier) (number)] @x     -- x: Node
```

**Rationale:** If you don't capture structure, you want the matched node itself. The unit-to-Node promotion ensures you always get something useful.

**Counter-example without promotion:**

```
(identifier) @x  -- would be x: () which is useless
```

### 2. Named expressions without captures → Node

```
Binary = (binary_expression)
(call (Binary) @x)             -- x: Binary where Binary = Node
```

The name provides pattern abstraction. Type is `Node`.

**Why not a named empty struct?** An empty struct `{}` carries no data. `Node` at least gives you the matched node.

### 3. Named expressions encapsulate captures

```
Common = (identifier) @id
Stmt   = (expression_statement (Common))      -- Stmt type: Node (not { id: Node })
Stmt2  = (expression_statement (Common) @c)   -- Stmt2 type: { c: Common }
```

Using a named expression without capturing it matches the pattern but extracts no data. The `@id` inside `Common` does not bubble up to `Stmt`.

**Rationale:** Named expressions are abstraction boundaries. If captures bubbled through:

- Recursive definitions would produce unbounded/inexpressible field sets
- Multiple uses of the same ref (`(Common) (Common)`) would cause name collisions
- Captures would leak from implementation details

**The rule:** Named expressions = structural reuse. Captures = data extraction. These are orthogonal.

**Fix when you need the data:** Capture the reference: `(Common) @c`. Access fields via `c.id`.

### 4. `:: string` only on bare captures

```
(identifier) @x :: string      -- valid: x is string
{ (a) @a } @x :: string        -- ERROR: can't extract text from struct
```

**Rationale:** `:: string` means "extract the text of the matched node." A struct has multiple captures — which node's text would you extract?

### 5. Merge alternations with structure need `:: TypeName`

When a merge alternation has captures inside, the outer capture needs a type name:

```
// type annotation required for codegen
[
  (identifier) @x
  (number) @y
] @value :: Value // -> Value = { x?: Node, y?: Node }
```

Without the annotation, inference works but codegen can't name the anonymous struct.

**Why require it?** Generated code needs a name for every type. Anonymous structs work in TypeScript but not in Rust/Python. Explicit naming ensures portable codegen.

### 6. Same-name captures must have same type

```
[
  (identifier) @x
  (number) @x
] @value                       -- valid: x: Node in both

[
  (identifier) @x :: string
  (number) @x
] @value                       -- ERROR: x is string vs Node
```

**Rationale:** If `@x` has different types in different branches, what type should the merged `x` field have? There's no safe answer.

**Solution:** Make types match, or use tagged alternation where each branch is independent.

### 7. Duplicate names across scopes is valid

```
{
  { (x) @item } @first
  { (y) @item } @second
} // -> { first: { item: Node }, second: { item: Node } }
```

Each `@item` is in its own scope. They don't conflict.

**Rationale:** Scopes are exactly for this — allowing local names without global uniqueness.

### 8. Duplicate names within same scope is error

```
{
  (x) @a
  (y) @a                       -- ERROR: duplicate capture @a in scope
}
```

**Rationale:** Which `@a` would you mean? Both bubble up to the same scope, creating ambiguity.

**Solution:** Use different names, or create nested scopes:

```
{
  { (x) @a } @first
  { (y) @a } @second
}
```

### 9. Tagged alternation without capture → warning

```
(statement
  [
    Assign: (assignment) @a
    Call: (call) @b
  ])                           -- WARNING: tags unused without capture
```

Tags only matter when the alternation itself is captured. Without capture, bindings merge as if unlabeled.

**Rationale:** You wrote labels but aren't using them. Probably a mistake.

### 10. Mixed labeled/unlabeled → error

```
[
  Assign: (assignment)
  (call)                       -- ERROR: mixing styles
]
```

An alternation is either fully labeled or fully unlabeled.

**Rationale:** Mixing would be confusing. Is it a union? Is it merged? Pick one style.

### 11. Empty alternation → parse error

```
[] @x                          -- ERROR: empty alternation
```

**Rationale:** An alternation with no branches can never match anything.

### 12. Nested quantifiers stack

```
{ (x) @item }* @batch+
-- batch: { item: Node }⁺[]
```

The inner `*` makes each batch contain zero or more items. The outer `+` requires at least one batch.

**Example trace:**

1. `(x) @item` → `{ item: Node }`
2. `{...}*` → `{ item: Node }[]` (array of structs)
3. `... @batch` → `{ batch: { item: Node }[] }`
4. `...+` → `{ batch: { item: Node }[] }⁺` (non-empty array of arrays)

### 13. Recursive types use nominal references

```
Expr = [
  Leaf: (identifier) @name
  Binary: (binary (Expr) @left (Expr) @right)
]
```

Produces:

```
Expr = <Leaf: { name: Node }, Binary: { left: Expr, right: Expr }>
```

**Why nominal?** Structural recursion is infinite. `Expr` refers to `Expr` by name, not by expanding it.

In Rust, recursive fields need `Box<T>`:

```rust
enum Expr {
    Leaf { name: Node },
    Binary { left: Box<Expr>, right: Box<Expr> },
}
```

**Example: Mutual recursion**

```
Stmt = [
  Expr: (expression_statement (Expr) @expr)
  Block: (block (Stmt)* @stmts)
]

Expr = [
  Lit: (number) @value
  Lambda: (arrow_function body: (Stmt) @body)
]
```

Both types reference each other. Codegen handles this via forward declarations.

---

## Codegen Output

### TypeScript

```typescript
interface FunctionDecl {
  name: string;
  body: Node;
  decorators: Node[];
}

type Statement =
  | { tag: "Assign"; target: string; value: Expression }
  | { tag: "Call"; func: string; args: Expression[] }
  | { tag: "Return"; value?: Expression };
```

**Mapping:**
| Plotnik Type | TypeScript |
|--------------|------------|
| `Node` | `Node` (interface) |
| `string` | `string` |
| `{ f: T }` | `{ f: T }` or `interface Name { f: T }` |
| `T?` | `T \| undefined` or `f?: T` in objects |
| `T*` | `T[]` |
| `T+` | `[T, ...T[]]` |
| `<A: T₁, B: T₂>` | `{ tag: "A" } & T₁ \| { tag: "B" } & T₂` |

### Rust

```rust
struct FunctionDecl {
    name: String,
    body: Node,
    decorators: Vec<Node>,
}

enum Statement {
    Assign { target: String, value: Expression },
    Call { func: String, args: Vec<Expression> },
    Return { value: Option<Expression> },
}
```

**Mapping:**
| Plotnik Type | Rust |
|--------------|------|
| `Node` | `Node` (struct) |
| `string` | `String` |
| `{ f: T }` | `struct Name { f: T }` |
| `T?` | `Option<T>` |
| `T*` | `Vec<T>` |
| `T+` | `(T, Vec<T>)` |
| `<A: T₁, B: T₂>` | `enum Name { A { ... }, B { ... } }` |

**Boxing:** Recursive types require indirection:

```rust
enum Expr {
    Binary { left: Box<Expr>, right: Box<Expr> },
    // ...
}
```

### Python

```python
@dataclass
class FunctionDecl:
    name: str
    body: Node
    decorators: list[Node]

Statement = Assign | Call | Return

@dataclass
class Assign:
    target: str
    value: Expression
```

**Mapping:**
| Plotnik Type | Python |
|--------------|--------|
| `Node` | `Node` |
| `string` | `str` |
| `{ f: T }` | `@dataclass class Name` |
| `T?` | `T \| None` |
| `T*` | `list[T]` |
| `T+` | `list[T]` |
| `<A: T₁, B: T₂>` | Union of dataclasses |

---

## Implementation Phases

### Phase 1: Collect

Walk AST, gather captures with positions and annotations. Build scope tree.

**Input:** Parsed AST
**Output:** Scope tree with capture information

```
Scope {
  captures: [(name, position, type_annotation?)]
  children: [Scope]
  kind: Root | Sequence | Alternation | TaggedBranch
}
```

### Phase 2: Infer

Bottom-up traversal computing `(type, bindings)` for each node.

**Algorithm:**

1. Start from leaves (terminals)
2. Work up to root
3. At each node, apply inference rule based on node type
4. Propagate bindings upward until hitting scope boundary

### Phase 3: Validate

- Type compatibility in merge alternations
- No duplicate captures in same scope
- No mixed labeled/unlabeled
- Warn on uncaptured tagged alternations
- `:: string` only on bare nodes
- Recursive type well-formedness

**Error examples:**

```
-- E001: Type mismatch in alternation
[
  (x) @a :: string
  (y) @a            -- @a has conflicting types: string vs Node
]

-- E002: Duplicate capture in scope
{
  (x) @a
  (y) @a            -- @a already defined in this scope
}

-- E003: Mixed alternation styles
[
  A: (x)
  (y)               -- Cannot mix labeled and unlabeled branches
]
```

### Phase 4: Emit

Generate target language types. Topologically sort by dependency. Box recursive types in Rust.

**Steps:**

1. Build dependency graph between named types
2. Detect cycles (recursive types)
3. Topological sort non-cyclic dependencies
4. For cycles: emit forward declarations or use boxing
5. Generate type definitions in sorted order

---

## Error Recovery

- On type mismatch, pick one arbitrarily and continue
- Report all errors, don't fail fast
- Infer what you can even with errors

**Philosophy:** A partially-correct type is more useful than no type at all. Users can see what would work and fix errors incrementally.

**Example:**

```
[
  (x) @a :: string
  (y) @a :: number    -- ERROR: type mismatch
  (z) @b
]
```

Inference continues:

- `@a` is marked as conflicted, arbitrarily picks `string`
- `@b` infers normally as `Node`
- Output: `{ a: string, b?: Node }` with error reported

The user sees both the error and the inferred structure, helping them understand what to fix.
