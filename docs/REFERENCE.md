# Plotnik Query Language Reference

Plotnik QL is a pattern-matching language for tree-sitter syntax trees. It extends [tree-sitter's query language](https://tree-sitter.github.io/tree-sitter/using-parsers/queries/1-syntax.html) with named expressions, recursion, and type inference.

> Predicates (`#eq?`, `#match?`, etc.) and directives (`#set!`, etc.) from tree-sitter QL are intentionally not supported. Plotnik focuses on structural pattern matching; filtering logic belongs in the host language.

---

## File Structure

A Plotnik file contains one or more definitions. All definitions must be named (`Name = expr`) except optionally the last one, which becomes the entry point:

```
; named definitions (required for all but last)
Expr = [(identifier) (number) (string)]
Stmt = (statement)

; unnamed entry point (only allowed as last definition)
(assignment_expression right: (Expr) @value)
```

An unnamed definition that is not the last in the file produces an error. The error message includes the entire unnamed definition to help identify and fix it.

---

## Naming Conventions

- Capitalized names (`Expr`, `Statement`, `BinaryOp`) are user-defined: named expressions, alternation labels, type annotations
- Lowercase names (`function_declaration`, `identifier`, `binary_expression`) are language-defined: node types from tree-sitter grammars
- Capture names must be snake_case (e.g., `@name`, `@func_body`)

This distinction is enforced by the parser.

> **Difference from tree-sitter:** Tree-sitter allows arbitrary capture names including dots (e.g., `@function.name`). Plotnik restricts captures to snake*case identifiers (`[a-z]a-z0-9*]\*`) because they map directly to struct fields in generated code (Rust, TypeScript, Python). Use underscores instead: `@function_name`.

---

## Data Model

Plotnik infers structured output types from your query. Understanding this section is essential—the rules are simple but may surprise users expecting nested output to mirror nested patterns.

### Core Concept: Flat by Default

Query nesting does NOT create output nesting. All captures within a query become fields in a single flat record, regardless of how deeply nested the pattern is.

```
(function_declaration
  name: (identifier) @name
  body: (block
    (return_statement (expression) @retval)))
```

Output type:

```typescript
{ name: Node, retval: Node }  // flat, not nested
```

The pattern is 4 levels deep, but the output is flat. This is intentional: you're usually extracting specific pieces from an AST, not reconstructing its shape.

### The Node Type

Every capture produces a `Node` by default—a reference to a tree-sitter node:

```typescript
interface Node {
  kind: string; // node type, e.g. "identifier"
  text: string; // source text
  start: Position; // { row, column }
  end: Position;
}
```

### Cardinality: Quantifiers → Arrays

Quantifiers on the captured pattern determine whether a field is singular, optional, or an array:

| Pattern   | Output Type      | Meaning      |
| --------- | ---------------- | ------------ |
| `(x) @a`  | `a: T`           | exactly one  |
| `(x)? @a` | `a?: T`          | zero or one  |
| `(x)* @a` | `a: T[]`         | zero or more |
| `(x)+ @a` | `a: [T, ...T[]]` | one or more  |

### Creating Nested Structure

To create nested structure, place a capture on a sequence `{...}` or alternation `[...]`. It's the capture on the grouping construct that creates a new scope—the braces alone don't introduce nesting:

```
{
  (function_declaration
    name: (identifier) @name
    body: (_) @body
  ) @node
} @func
```

Output type:

```typescript
{ func: { node: Node, name: Node, body: Node } }
```

The `@func` capture on the group creates a nested scope. All captures inside (`@node`, `@name`, `@body`) become fields of that nested object.

### Type Annotations

The `::` syntax after a capture names the output type for codegen:

```
@x :: MyType    // name this capture's type "MyType"
@x :: string    // special: extract node.text as a string
```

| Annotation     | Effect                                      |
| -------------- | ------------------------------------------- |
| `@x`           | inferred type (usually `Node`)              |
| `@x :: string` | converts to `string` (extracts `node.text`) |
| `@x :: T`      | names the type `T` in generated code        |

Only `:: string` changes the actual data. Other `:: T` annotations only affect generated type/interface names.

Example with type annotation on a group:

```
{
  (function_declaration
    name: (identifier) @name :: string
    body: (_) @body
  ) @node
} @func :: FunctionDeclaration
```

Output type:

```typescript
interface FunctionDeclaration {
  node: Node;
  name: string; // :: string converted this
  body: Node;
}

{
  func: FunctionDeclaration;
}
```

### Summary

| What you write            | What you get                           |
| ------------------------- | -------------------------------------- |
| `@name` anywhere in query | field `name` in current scope          |
| `(pattern)? @x`           | optional field                         |
| `(pattern)* @x`           | array field                            |
| `{...} @x` or `[...] @x`  | nested object (new scope for captures) |
| `@x :: string`            | string value instead of Node           |
| `@x :: TypeName`          | custom type name in codegen            |

---

## Nodes

### Named Nodes

Match named nodes (non-terminals and named terminals) by type:

```
(function_declaration)
(binary_expression (identifier) (number))
```

Children can be partial—this matches any `binary_expression` with at least one `string_literal` child:

```
(binary_expression (string_literal))
```

With captures:

```
(binary_expression
  (identifier) @left
  (number) @right)
```

Output type:

```typescript
{ left: Node, right: Node }
```

### Anonymous Nodes

Match literal tokens (operators, keywords, punctuation) with double quotes:

```
(binary_expression operator: "!=")
(return_statement "return")
```

Anonymous nodes cannot be captured directly. Capture the parent or use a wildcard:

```
(binary_expression operator: _ @op)   ; captures the operator token
```

Output type:

```typescript
{
  op: Node;
}
```

### Wildcards

- `(_)` — matches any named node
- `_` — matches any node (named or anonymous)

```
(call_expression function: (_) @fn)
(pair key: _ @key value: _ @value)
```

Output type:

```typescript
{ fn: Node }
{ key: Node, value: Node }
```

### Special Nodes

- `(ERROR)` — matches parser error nodes
- `(MISSING)` — matches nodes inserted by error recovery
- `(MISSING identifier)` — matches a specific missing node type
- `(MISSING ";")` — matches a missing anonymous node

```
(ERROR) @syntax_error
(MISSING ";") @missing_semicolon
```

Output type:

```typescript
{
  syntax_error: Node;
}
{
  missing_semicolon: Node;
}
```

### Supertypes

Some grammars define supertypes (abstract node types). Query them directly:

```
(expression) @expr
```

Query a specific subtype within a supertype context:

```
(expression/binary_expression) @binary
(expression/"()") @empty_parens
```

Output type:

```typescript
{
  binary: Node;
}
{
  empty_parens: Node;
}
```

---

## Fields

Constrain children to named fields. A field value must be a node pattern, an alternation, or a quantifier applied to one of these. Groups `{...}` are not allowed as direct field values.

```
(assignment_expression
  left: (identifier) @target
  right: (call_expression) @value)
```

Output type:

```typescript
{ target: Node, value: Node }
```

With type annotations:

```
(assignment_expression
  left: (identifier) @target :: string
  right: (call_expression) @value)
```

Output type:

```typescript
{ target: string, value: Node }
```

### Negated Fields

Assert a field is absent with `!`:

```
(function_declaration
  name: (identifier) @name
  !type_parameters)
```

Negated fields don't affect the output type—they're purely structural constraints:

```typescript
{
  name: Node;
}
```

---

## Quantifiers

- `?` — zero or one (optional)
- `*` — zero or more
- `+` — one or more (non-empty)

```
(function_declaration (decorator)? @decorator)
(function_declaration (decorator)* @decorators)
(function_declaration (decorator)+ @decorators)
```

Output types:

```typescript
{ decorator?: Node }
{ decorators: Node[] }
{ decorators: [Node, ...Node[]] }
```

The `+` quantifier always produces non-empty arrays—no opt-out.

Plotnik also supports non-greedy variants: `*?`, `+?`, `??`

---

## Sequences

Match sibling patterns in order with braces. Tree-sitter uses `((a) (b))` for the same purpose. Plotnik uses `{...}` to visually distinguish grouping from node patterns, and adds scope creation when captured (`{...} @name`).

```
{
  (comment)
  (function_declaration)
}
```

Quantifiers apply to sequences:

```
{
  (number)
  {"," (number)}*
}
```

### Sequences with Captures

Capture elements inside a sequence:

```
{
  (decorator)* @decorators
  (function_declaration) @fn
}
```

Output type:

```typescript
{ decorators: Node[], fn: Node }
```

Capture the entire sequence with a type name:

```
{
  (comment)+
  (function_declaration) @fn
}+ @sections :: Section
```

Output type:

```typescript
interface Section {
  fn: Node;
}

{ sections: [Section, ...Section[]] }
```

---

## Alternations

Match one of several alternatives with `[...]`:

```
[
  (identifier)
  (string_literal)
] @value
```

### Merge Style (Unlabeled)

Without labels, captures from all branches merge. If a capture appears in all branches, it's required; otherwise optional. Captures with the same name must have the same type across all branches where they appear.

All branches must be type-compatible: either all branches produce bare nodes (no internal captures), or all branches produce structures (have internal captures). When branches mix nodes and structures, bare node captures are auto-promoted to single-field structures. When merging structures, the captured alternation requires an explicit type annotation (`@x :: TypeName`) for codegen.

```
(statement
  [
    (assignment_expression left: (identifier) @left)
    (call_expression function: (identifier) @func)
  ])
```

Output type:

```typescript
{ left?: Node, func?: Node }  // each appears in one branch only
```

When the same capture appears in all branches:

```
[
  (identifier) @name
  (string) @name
]
```

Output type:

```typescript
{
  name: Node;
} // required: present in all branches, same type
```

Mixed presence:

```
[
  (binary_expression
    left: (_) @x
    right: (_) @y)
  (identifier) @x
]
```

The second branch `(identifier) @x` is auto-promoted to a structure `{ x: Node }`, making it compatible with the first branch.

Output type:

```typescript
{ x: Node, y?: Node }  // x in all branches (required), y in one (optional)
```

Type mismatch is an error:

```
[(identifier) @x :: string (number) @x :: number]  // ERROR: @x has different types
```

With a capture on the alternation itself, the type is non-optional since exactly one branch must match:

```
[
  (identifier)
  (number)
] @value
```

Output type:

```typescript
{
  value: Node;
}
```

### Tagged Style (Labeled)

Labels create a discriminated union:

```
[
  Assign: (assignment_expression left: (identifier) @left)
  Call: (call_expression function: (identifier) @func)
] @stmt :: Stmt
```

Output type (discriminant is always `tag`):

```typescript
type Stmt = { tag: "Assign"; left: Node } | { tag: "Call"; func: Node };
```

In Rust, tagged alternations become enums:

```rust
enum Stmt {
    Assign { left: Node },
    Call { func: Node },
}
```

### Alternations with Type Annotations

When a merge alternation produces a structure (branches have internal captures), the capture on the alternation must have an explicit type annotation for codegen:

```
(call_expression
  function: [
    (identifier) @fn
    (member_expression property: (property_identifier) @method)
  ] @target :: Target)
```

Output type:

```typescript
interface Target {
  fn?: Node;
  method?: Node;
}

{
  target: Target;
}
```

---

## Anchors

The anchor `.` constrains sibling positions. Anchors don't affect types—they're structural constraints.

First child:

```
(array . (identifier) @first)
```

Last child:

```
(block (_) @last .)
```

Immediate adjacency:

```
(dotted_name (identifier) @a . (identifier) @b)
```

Without the anchor, `@a` and `@b` would match non-adjacent pairs too.

Output type for all examples:

```typescript
{ first: Node }
{ last: Node }
{ a: Node, b: Node }
```

Anchors ignore anonymous nodes.

---

## Named Expressions

Define reusable patterns with `Name = pattern`:

```
BinaryOp =
  (binary_expression
    left: (_) @left
    operator: _ @op
    right: (_) @right)
```

Use named expressions as node types:

```
(return_statement (BinaryOp) @expr)
```

Output type:

```typescript
{
  expr: BinaryOp;
} // BinaryOp = { left: Node, op: Node, right: Node }
```

Named expressions define both a pattern and a type. The type is inferred from captures within:

```
Expr = [(BinaryOp) (UnaryOp) (identifier) (number)]
```

When used:

```
(assignment_expression right: (Expr) @value)
```

Output type:

```typescript
{
  value: Expr;
} // union of BinaryOp, UnaryOp, or Node
```

---

## Recursion

Named expressions can reference themselves:

```
NestedCall =
  (call_expression
    function: [(identifier) @name (NestedCall) @inner]
    arguments: (arguments))
```

This matches `a()`, `a()()`, `a()()()`, etc.

Output type:

```typescript
type NestedCall = {
  name?: Node;
  inner?: NestedCall;
};
```

Another example—matching arbitrarily nested member chains:

```
MemberChain = [
  Base: (identifier) @name
  Access: (member_expression
    object: (MemberChain) @object
    property: (property_identifier) @property)
]
```

Output type:

```typescript
type MemberChain =
  | { tag: "Base"; name: Node }
  | { tag: "Access"; object: MemberChain; property: Node };
```

---

## Full Example

```
Statement = [
  Assign: (assignment_expression
    left: (identifier) @target :: string
    right: (Expression) @value)
  Call: (call_expression
    function: (identifier) @func :: string
    arguments: (arguments (Expression)* @args))
  Return: (return_statement
    (Expression)? @value)
]

Expression = [
  Ident: (identifier) @name :: string
  Num: (number) @value :: string
  Str: (string) @value :: string
]

(program (Statement)+ @statements)
```

Output types:

```typescript
type Statement =
  | { tag: "Assign"; target: string; value: Expression }
  | { tag: "Call"; func: string; args: Expression[] }
  | { tag: "Return"; value?: Expression };

type Expression =
  | { tag: "Ident"; name: string }
  | { tag: "Num"; value: string }
  | { tag: "Str"; value: string };

type Root = {
  statements: [Statement, ...Statement[]];
};
```

---

## Quick Reference

| Feature              | Tree-sitter      | Plotnik                   |
| -------------------- | ---------------- | ------------------------- |
| Capture              | `@name`          | `@name` (snake_case only) |
| Type annotation      |                  | `@x :: T`                 |
| Text extraction      |                  | `@x :: string`            |
| Named node           | `(type)`         | `(type)`                  |
| Anonymous node       | `"text"`         | `"text"`                  |
| Any node             | `_`              | `_`                       |
| Any named node       | `(_)`            | `(_)`                     |
| Field constraint     | `field: pattern` | `field: pattern`          |
| Negated field        | `!field`         | `!field`                  |
| Quantifiers          | `?` `*` `+`      | `?` `*` `+`               |
| Non-greedy           |                  | `??` `*?` `+?`            |
| Sequence             | `((a) (b))`      | `{(a) (b)}`               |
| Alternation          | `[a b]`          | `[a b]`                   |
| Tagged alternation   |                  | `[A: (a) B: (b)]`         |
| Anchor               | `.`              | `.`                       |
| Named expression     |                  | `Name = pattern`          |
| Use named expression |                  | `(Name)`                  |
