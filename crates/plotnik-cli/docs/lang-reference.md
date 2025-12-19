# Plotnik Query Language Reference

Plotnik is a pattern-matching language for tree-sitter syntax trees. It extends [tree-sitter's query syntax](https://tree-sitter.github.io/tree-sitter/using-parsers/queries/1-syntax.html) with named expressions, recursion, and static type inference.

Predicates (`#eq?`, `#match?`) and directives (`#set!`) are intentionally unsupported—filtering logic belongs in your host language.

---

## Execution Model

NFA-based cursor walk with backtracking.

### Key Properties

- **Root-anchored**: Matches the entire tree structure (like `^...$` in regex)
- **Backtracking**: Failed branches restore state and try alternatives
- **Ordered choice**: `[A B C]` tries branches left-to-right; first match wins

### Trivia Handling

Comments and "extra" nodes (per tree-sitter grammar) are automatically skipped unless explicitly matched.

```plotnik/docs/lang-reference.md#L24-24
(function_declaration (identifier) @name (block) @body)
```

Matches even with comments between children:

```plotnik/docs/lang-reference.md#L28-31
function foo /* comment */() {
  /* body */
}
```

The `.` anchor enforces strict adjacency:

```plotnik/docs/lang-reference.md#L35-35
(array . (identifier) @first)  ; must be immediately after bracket
```

### Partial Matching

Node patterns are open—unmentioned children are ignored:

```plotnik/docs/lang-reference.md#L46-46
(binary_expression left: (identifier) @left)
```

Matches any `binary_expression` with an `identifier` in `left`, regardless of `operator`, `right`, etc.

Sequences `{...}` advance through siblings in order, skipping non-matching nodes.

### Field Constraints

`field: pattern` requires the child to have that field AND match the pattern:

```plotnik/docs/lang-reference.md#L58-61
(binary_expression
  left: (identifier) @x
  right: (number) @y
)
```

Fields participate in sequential matching—they're not independent lookups.

---

## File Structure

A `.ptk` file contains definitions:

```plotnik/docs/lang-reference.md#L78-82
; Internal (mixin/fragment)
Expr = [(identifier) (number) (string)]

; Public entrypoint
pub Stmt = (statement) @stmt
```

### Visibility

| Syntax          | Role              | In Binary |
| --------------- | ----------------- | --------- |
| `Def = ...`     | Internal mixin    | No        |
| `pub Def = ...` | Public entrypoint | Yes       |

Internal definitions exist only to support `pub` definitions.

### Script vs Module Mode

**Script** (`-q` flag): Anonymous expressions allowed, auto-wrapped in language root.

```sh
plotnik exec -q '(identifier) @id' -s app.js
```

**Module** (`.ptk` files): Only named definitions allowed.

```plotnik/docs/lang-reference.md#L106-110
; ERROR in .ptk file
(identifier) @id

; OK
pub Query = (identifier) @id
```

---

## Workspace

A directory of `.ptk` files loaded as a single compilation unit.

### Properties

- **Flat namespace**: `Foo` in `a.ptk` visible in `b.ptk` without imports
- **Global uniqueness**: Duplicate names are errors
- **Non-recursive**: Subdirectories are separate workspaces
- **Dead code elimination**: Unreachable internals stripped

### Language Inference

Inferred from directory name (`queries.ts/` → TypeScript, `java-checks/` → Java). Override with `-l/--lang`.

### Execution

- Single `pub`: Default entrypoint
- Multiple `pub`: Use `--entry <Name>`
- No `pub`: Compilation error

### Example

`helpers.ptk`:

```plotnik/docs/lang-reference.md#L147-153
Ident = (identifier)

DeepSearch = [
    (Ident) @target
    (_ (DeepSearch)*)
]
```

`main.ptk`:

```plotnik/docs/lang-reference.md#L157-158
pub AllIdentifiers = (program (DeepSearch)*)
```

---

## Naming Conventions

| Kind                       | Case         | Examples                             |
| -------------------------- | ------------ | ------------------------------------ |
| Definitions, labels, types | `PascalCase` | `Expr`, `Statement`, `BinaryOp`      |
| Node kinds                 | `snake_case` | `function_declaration`, `identifier` |
| Captures, fields           | `snake_case` | `@name`, `@func_body`                |

Tree-sitter allows `@function.name`; Plotnik requires `@function_name` because captures map to struct fields.

---

## Data Model

Plotnik infers output types from your query. The key rule may surprise you—but it's intentional for schema stability.

### Flat by Default

Query nesting does NOT create output nesting. All captures become fields in a single flat record.

**Why?** Adding a new `@capture` to an existing query shouldn't break downstream code using other captures. Flat output makes capture additions non-breaking. See [Type System](type-system.md#design-philosophy) for the full rationale.

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

Default capture type—a reference to a tree-sitter node:

```plotnik/docs/lang-reference.md#L205-210
interface Node {
  kind: string;    // e.g. "identifier"
  text: string;    // source text
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

Capture a sequence `{...}` or alternation `[...]` to create a new scope. Braces alone don't introduce nesting:

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

`::` after a capture controls the output type:

| Annotation     | Effect                        |
| -------------- | ----------------------------- |
| `@x`           | Inferred (usually `Node`)     |
| `@x :: string` | Extract `node.text` as string |
| `@x :: T`      | Name the type `T` in codegen  |

Only `:: string` changes data; other `:: T` affect only generated type names.

Example:

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

| Pattern                 | Output                    |
| ----------------------- | ------------------------- |
| `@name`                 | Field in current scope    |
| `(x)? @a`               | Optional field            |
| `(x)* @a`               | Array field               |
| `{...} @x` / `[...] @x` | Nested object (new scope) |
| `@x :: string`          | String value              |
| `@x :: T`               | Custom type name          |

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

Match literal tokens (operators, keywords, punctuation) with double or single quotes:

```
(binary_expression operator: "!=")
(return_statement "return")
```

Single quotes are equivalent to double quotes, useful when the query itself is wrapped in double quotes (e.g., in tool calls or JSON):

```
(return_statement 'return')
```

Anonymous nodes can be captured directly:

```
(binary_expression "+" @op)
"return" @keyword
```

Output type:

```typescript
{
  op: Node;
}
{
  keyword: Node;
}
```

### Wildcards

| Syntax | Matches                       |
| ------ | ----------------------------- |
| `(_)`  | Any named node                |
| `_`    | Any node (named or anonymous) |

```plotnik/docs/lang-reference.md#L370-371
(call_expression function: (_) @fn)
(pair key: _ @key value: _ @value)
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

Query abstract node types directly, or narrow with `/`:

```plotnik/docs/lang-reference.md#L406-409
(expression) @expr
(expression/binary_expression) @binary
(expression/"()") @empty_parens
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

Match sibling patterns in order with braces.

> **⚠️ Syntax Difference from Tree-sitter**
>
> Tree-sitter: `((a) (b))` — parentheses for sequences
> Plotnik: `{(a) (b)}` — braces for sequences
>
> This avoids ambiguity: `(foo)` is always a node, `{...}` is always a sequence.
> Using tree-sitter's `((a) (b))` syntax in Plotnik is a parse error.

Plotnik uses `{...}` to visually distinguish grouping from node patterns, and adds scope creation when captured (`{...} @name`).

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

Match alternatives with `[...]`:

- **Untagged**: Fields merge across branches
- **Tagged** (with labels): Discriminated union

```plotnik/docs/lang-reference.md#L570-573
[
  (identifier)
  (string_literal)
] @value
```

### Merge Style (Unlabeled)

Captures merge: present in all branches → required; some branches → optional. Same-name captures must have compatible types.

Branches must be type-compatible. Bare nodes are auto-promoted to single-field structs when mixed with structured branches.

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

Labels create a discriminated union (`$tag` + `$data`):

```plotnik/docs/lang-reference.md#L657-660
[
  Assign: (assignment_expression left: (identifier) @left)
  Call: (call_expression function: (identifier) @func)
] @stmt :: Stmt
```

```plotnik/docs/lang-reference.md#L664-667
type Stmt =
  | { $tag: "Assign"; $data: { left: Node } }
  | { $tag: "Call"; $data: { func: Node } };
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

Define reusable patterns:

```plotnik/docs/lang-reference.md#L744-748
BinaryOp =
  (binary_expression
    left: (_) @left
    operator: _ @op
    right: (_) @right)
```

Use as node types:

```plotnik/docs/lang-reference.md#L752-752
(return_statement (BinaryOp) @expr)
```

**Encapsulation**: `(Name)` matches but extracts nothing. You must capture (`(Name) @x`) to access fields. This separates structural reuse from data extraction.

Named expressions define both pattern and type:

```plotnik/docs/lang-reference.md#L764-764
Expr = [(BinaryOp) (UnaryOp) (identifier) (number)]
```

---

## Recursion

Named expressions can self-reference:

```plotnik/docs/lang-reference.md#L794-798
NestedCall =
  (call_expression
    function: [(identifier) @name (NestedCall) @inner]
    arguments: (arguments))
```

Matches `a()`, `a()()`, `a()()()`, etc. → `{ name?: Node, inner?: NestedCall }`

Tagged recursive example:

```plotnik/docs/lang-reference.md#L810-815
MemberChain = [
  Base: (identifier) @name
  Access: (member_expression
    object: (MemberChain) @object
    property: (property_identifier) @property)
]
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
  | { $tag: "Assign"; $data: { target: string; value: Expression } }
  | { $tag: "Call"; $data: { func: string; args: Expression[] } }
  | { $tag: "Return"; $data: { value?: Expression } };

type Expression =
  | { $tag: "Ident"; $data: { name: string } }
  | { $tag: "Num"; $data: { value: string } }
  | { $tag: "Str"; $data: { value: string } };

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
| Public entrypoint    |                  | `pub Name = pattern`      |
| Use named expression |                  | `(Name)`                  |

---

## Diagnostics

Priority-based suppression: when diagnostics overlap, lower-priority ones are hidden. You see the root cause, not cascading symptoms.
