# Plotnik Query Language Reference

Plotnik is a pattern-matching language for tree-sitter syntax trees. It extends [tree-sitter's query syntax](https://tree-sitter.github.io/tree-sitter/using-parsers/queries/1-syntax.html) with named expressions, recursion, and static type inference.

Tree-sitter predicates (`#eq?`, `#match?`) and directives (`#set!`) are not supported. Plotnik has its own inline predicate syntax (see [Predicates](#predicates)).

---

## Execution Model

NFA-based cursor walk with backtracking.

### Key Properties

- **Root-anchored**: Matches the entire tree structure (like `^...$` in regex)
- **Backtracking**: Failed branches restore state and try alternatives
- **Ordered choice**: `[A B C]` tries branches left-to-right; first match wins

### Trivia Handling

Plotnik has three sibling-navigation tiers:

1. **Default navigation is permissive.** Without an anchor, sibling patterns advance until they find a match, skipping named nodes, anonymous tokens, and tree-sitter `extra` nodes such as comments.
2. **`.` narrows navigation.** It always skips extras. When both sides are named, it also skips anonymous tokens such as punctuation. When either side is anonymous, it skips extras only.
3. **`.!` is exact.** It allows nothing between operands.

This unanchored query uses default navigation:

```
(function_declaration (identifier) @name (block) @body)
```

It matches even with intervening comments or punctuation:

```javascript
function foo /* comment */() {
  /* body */
}
```

### Anchor Behavior

The `.` anchor is soft adjacency.

```
(dotted_name (identifier) @a . (identifier) @b)
```

Because both operands are named, it matches `a.b` and `a /* x */ .b`, but it won't match if another named node appears between them.

When either side is anonymous, `.` skips extras but not other anonymous tokens:

```
(array "," . (string) @next)  ; comments tolerated, another token is not
```

Bare `_` is anonymous, so `(a) . _` is extras-only. `(_)` is named, so `(a) . (_)` skips trivia.

An anchor before an alternation applies per branch: `(a) . [(b) ","]` uses named-named soft adjacency for `(b)` and extras-only adjacency for `","`. An anchor after an alternation is conservative: `[(b) ","] . (a)` is extras-only because some branch may match an anonymous node.

Use `.!` for exact adjacency:

```
(call_expression (identifier) @fn .! "(")  ; nothing between name and paren
```

### Partial Matching

Node patterns are open — unmentioned children are ignored:

```
(binary_expression left: (identifier) @left)
```

Matches any `binary_expression` with an `identifier` in `left`, regardless of `operator`, `right`, etc.

Sequences `{...}` advance through siblings in order, skipping non-matching nodes.

### Field Constraints

`field: pattern` requires the child to have that field AND match the pattern:

```
(binary_expression
  left: (identifier) @x
  right: (number) @y
)
```

Fields participate in sequential matching — they're not independent lookups.

---

## File Structure

A `.ptk` file contains definitions:

````
```
; Helper (can also be used as entrypoint)
Expr = [(identifier) (number) (string)]

; Another definition
Stmt = (statement) @stmt
````

All definitions are entrypoints and included in the binary. Use `--entry <Name>` to select which one to execute.

### Script vs Module Mode

**Script** (`-q` flag): Anonymous expressions allowed, auto-wrapped in language root.

```sh
plotnik exec -q '(identifier) @id' -s app.js
```

**Module** (`.ptk` files): Only named definitions allowed.

```
; ERROR in .ptk file
(identifier) @id

; OK
Query = (identifier) @id
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

- Single definition: Default entrypoint
- Multiple definitions: Use `--entry <Name>`

### Example

`helpers.ptk`:

```
Ident = (identifier)

DeepSearch = [
    (Ident) @target
    (_ (DeepSearch)*)
]
```

`main.ptk`:

```
AllIdentifiers = (program (DeepSearch)*)
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

Plotnik infers output types from your query. See [Type System](type-system.md) for full details.

### Flat by Default

Query nesting does NOT create output nesting. All captures bubble up to the nearest scope boundary:

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

The pattern is 4 levels deep, but the output is flat. You're extracting specific pieces from an AST, not reconstructing its shape.

### Strict Dimensionality

**Quantifiers (`*`, `+`) containing internal captures require a struct capture.**

```
// ERROR: internal capture without struct capture
(method_definition name: (identifier) @name)*

// OK: struct capture on the sequence
{ (method_definition name: (identifier) @name) @method }* @methods
→ { methods: { method: Node, name: Node }[] }
```

This prevents association loss — each struct is a distinct object, not parallel arrays that lose per-iteration grouping. See [Type System: Strict Dimensionality](type-system.md#1-strict-dimensionality).

### The Node Type

Default capture type — a reference to a tree-sitter node:

```
interface Node {
  kind: string;    // e.g. "identifier"
  text: string;    // source text
  start: Position; // { row, column }
  end: Position;
}
```

### Cardinality: Quantifiers → Arrays

Quantifiers determine whether a field is singular, optional, or an array:

| Pattern   | Output Type      | Meaning                    |
| --------- | ---------------- | -------------------------- |
| `(x) @a`  | `a: T`           | exactly one                |
| `(x)? @a` | `a: T \| null`   | zero or one                |
| `(x)* @a` | `a: T[]`         | zero or more (scalar list) |
| `(x)+ @a` | `a: [T, ...T[]]` | one or more (scalar list)  |

Every declared field is **always present** in the output: an optional field is
`T | null` and materializes as `null` when it doesn't match (never an absent
key), and a missing list is `[]` (never `null`). The output shape is stable.

Node arrays work when the quantified pattern has **no internal captures**. For patterns with internal captures, use struct arrays:

| Pattern         | Output Type       | Meaning                                 |
| --------------- | ----------------- | --------------------------------------- |
| `{...}* @items` | `items: T[]`      | zero or more structs                    |
| `{...}+ @items` | `items: [T, ...]` | one or more structs                     |
| `{...}? @item`  | `item: T \| null` | nullable struct (bubbles if uncaptured) |

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

The `@func` capture on the sequence creates a nested scope. All captures inside (`@node`, `@name`, `@body`) become fields of that nested object.

### Type Annotations

`::` after a capture controls the output type:

| Annotation     | Effect                        |
| -------------- | ----------------------------- |
| `@x`           | Inferred (usually `Node`)     |
| `@x :: string` | Extract `node.text` as string |
| `@x :: T`      | Name the type `T` in codegen  |

Only `:: string` changes data; other `:: T` affect only generated type names.

### Suppressive Captures

Suppress captures from contributing to output with `@_` or `@_name`:

```
Expr = (binary_expression left: (number) @left right: (number) @right)

; Without suppression: @left, @right bubble up
Query = (statement (Expr) @expr)
; Output: { expr: Node, left: Node, right: Node }

; With suppression: inner captures are suppressed
Query = (statement { (Expr) @_ } @expr)
; Output: { expr: Node }
```

Use cases:

- **Match structurally, don't extract**: Use a definition's pattern but discard its captures
- **Wrap and isolate**: `{ inner @_ } @outer` captures the outer node while suppressing inner captures

Rules:

- `@_` and `@_name` match like regular captures but produce no output
- Named suppressive captures (`@_foo`) are equivalent to `@_` — the name is documentation only
- Type annotations are not allowed on suppressive captures
- Nesting works: `@_outer` containing `@_inner` correctly suppresses both

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

| Pattern                 | Output                                |
| ----------------------- | ------------------------------------- |
| `@name`                 | Field in current scope                |
| `(x)? @a`               | Optional field                        |
| `(x)* @a`               | Node array (no internal captures)     |
| `{...}* @items`         | Struct array (with internal captures) |
| `{...} @x` / `[...] @x` | Nested object (new scope)             |
| `@x :: string`          | String value                          |
| `@x :: T`               | Custom type name                      |

---

## Nodes

### Named Nodes

Match named nodes (non-terminals and named terminals) by type:

```
(function_declaration)
(binary_expression (identifier) (number))
```

Children can be partial — this matches any `binary_expression` with at least one `string_literal` child:

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

### Predicates

Filter nodes by their text content with inline predicates:

```
(identifier == "foo")         ; text equals "foo"
(identifier != "bar")         ; text does not equal "bar"
(identifier ^= "get")         ; text starts with "get"
(identifier $= "_id")         ; text ends with "_id"
(identifier *= "test")        ; text contains "test"
(identifier =~ /^[A-Z]/)      ; text matches regex
(identifier !~ /^_/)          ; text does not match regex
```

| Operator | Meaning        |
| -------- | -------------- |
| `==`     | equals         |
| `!=`     | not equals     |
| `^=`     | starts with    |
| `$=`     | ends with      |
| `*=`     | contains       |
| `=~`     | matches regex  |
| `!~`     | does not match |

**Regex patterns** use `/pattern/` syntax. Full Unicode is supported. Patterns match anywhere in the text (use `^` and `$` anchors for full-match semantics).

```
(identifier =~ /^test_/)      ; starts with "test_"
(identifier =~ /Handler$/)    ; ends with "Handler"
(identifier =~ /^[A-Z][a-z]+(?:[A-Z][a-z]+)*$/)  ; PascalCase
```

**Unsupported regex features** (compile-time error):

- Backreferences (`\1`, `\2`)
- Lookahead/lookbehind (`(?=...)`, `(?!...)`, `(?<=...)`, `(?<!...)`)
- Named captures (`(?P<name>...)`)

Predicates don't affect output types — they're structural constraints like anchors.

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

```
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

Query abstract node types directly. A `#subtype` refinement is accepted syntactically but
not yet enforced — the node currently matches on its supertype alone, so `(expression)` and
`(expression#binary_expression)` behave identically for now:

```
(expression) @expr
(expression#binary_expression) @binary
(expression#"()") @empty_parens
```

The separator is tight-binding — no whitespace around `#`. The tree-sitter spelling
`expression/binary_expression` is also accepted but deprecated in favor of `#`.

---

## Fields

Constrain children to named fields. A field value must be a node pattern, an alternation, or a quantifier applied to one of these. Sequences `{...}` are not allowed as direct field values.

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

### Quantifiers and Captures on Fields

Quantifiers and captures after a field value apply to the entire field constraint, not just the value:

```
decorator: (decorator)* @decorators   ; repeats the whole field
value: [A: (x) B: (y)] @kind          ; captures the field (containing the alternation)
```

This allows repeating fields (useful for things like decorators in JavaScript). The capture still correctly produces the value's type — for alternations, you get the tagged union, not a raw node.

### Negated Fields

Assert a field is absent with `-`:

```
(function_declaration
  name: (identifier) @name
  -type_parameters)
```

Negated fields don't affect the output type — they're purely structural constraints:

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
{ decorator: Node | null }
{ decorators: Node[] }
{ decorators: [Node, ...Node[]] }
```

The `+` quantifier always produces non-empty arrays — no opt-out.

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

```
[
  (identifier)
  (string_literal)
] @value
```

### Merge Style (Unlabeled)

Captures merge: present in all branches → required; some branches → nullable (`T | null`, always present). Same-name captures must have compatible types.

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
{ left: Node | null, func: Node | null }  // each appears in one branch only
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
{ x: Node, y: Node | null }  // x in all branches (required), y in one (nullable)
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

```
[
  Assign: (assignment_expression left: (identifier) @left)
  Call: (call_expression function: (identifier) @func)
] @stmt :: Stmt
```

```
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
  fn: Node | null;
  method: Node | null;
}

{
  target: Target;
}
```

---

## Anchors

Anchors constrain sibling positions. They don't affect types — they're structural constraints.

### Anchor Strictness

`.` is soft adjacency: it skips extras and disallows other named nodes between operands. When both sides are named, it also skips anonymous tokens. `.!` is exact adjacency: it allows nothing between operands.

| Pattern      | Extras Between | Anonymous Nodes Between | Named Nodes Between |
| ------------ | -------------- | ----------------------- | ------------------- |
| `(a) . (b)`  | Allowed        | Allowed                 | Disallowed          |
| `"x" . (b)`  | Allowed        | Disallowed              | Disallowed          |
| `(a) . "x"`  | Allowed        | Disallowed              | Disallowed          |
| `"x" . "y"`  | Allowed        | Disallowed              | Disallowed          |
| `(a) .! (b)` | Disallowed     | Disallowed              | Disallowed          |

Extras are nodes tree-sitter marks with the per-node `is_extra` bit; there is no bytecode extras or trivia table.

Explicit patterns always win over skipping. For example, this matches a comment node and then softly anchors the function after it:

```
{(comment) @doc . (function_declaration) @fn}
```

### Position Anchors

First child:

```
(array . (identifier) @first)
```

Last child:

```
(block (_) @last .)
```

### Adjacency Anchors

```
(dotted_name (identifier) @a . (identifier) @b)
```

Without the anchor, `@a` and `@b` would match non-adjacent pairs too. With the anchor, only consecutive identifiers match. Comments and punctuation between them are tolerated because both sides are named.

With an anonymous operand, comments are still tolerated but other anonymous tokens are not:

```
(array "," . (string) @next)
```

For strict token-level adjacency:

```
(call_expression (identifier) @fn .! "(")
```

Here, no trivia is allowed between the function name and the opening parenthesis because the anchor is explicit strict adjacency.

### Anchors After Optional Items

When the item before an anchor is optional (`?` or `*`), the anchor's meaning depends on whether that item matched:

```
(program {(lexical_declaration)? @a . (debugger_statement) @b})
```

- **When `@a` matches**, the anchor is enforced between the two siblings: `@b` must be the adjacent sibling, so `let x; debugger;` matches but `let x; foo; debugger;` does not.
- **When `@a` is skipped**, the anchor degrades to a leading anchor relative to the parent — as if the query were `(program . (debugger_statement) @b)`. `@b` must be the first child (trivia aside): `debugger;` and `/* c */ debugger;` match, but `foo; debugger;` does not.

Strictness carries through both paths: with `.!`, no trivia is tolerated on either the adjacency or the leading interpretation.

The anchor pins where a **quantified follower** begins, not just a single node:

```
(program {(lexical_declaration)? @a . (debugger_statement)* @b . (expression_statement) @c})
```

The anchor before `(debugger_statement)*` fixes its starting position the same way — adjacent to `@a` when present, the first child when `@a` is skipped. The repeated matches are then **back-to-back**: the quantifier consumes consecutive matching siblings (trivia aside) and stops at the first gap. So with `@a` skipped, `debugger; debugger; foo;` collects both debuggers, but `bar; debugger; foo;` does not match — `@b` may not skip past `bar;` to start later.

A **trailing anchor** combines with the leading interpretation rather than overriding it:

```
(program {(lexical_declaration)? @a . (debugger_statement) @b .})
```

When `@a` is skipped, `@b` must be both the first child (leading anchor) and the last child (trailing anchor) — so only `debugger;` alone matches, while `foo; debugger;` (not first) and `debugger; foo;` (not last) do not.

### Output Types

Anchors are structural constraints only — they don't affect output types:

```typescript
{ first: Node }
{ last: Node }
{ a: Node, b: Node }
```

Anchors are not values and do not appear in output types.

### Anchor Placement Rules

Anchors require parent node context to be meaningful:

**Valid positions:**

```
(parent . (first))         ; first child anchor
(parent (last) .)          ; last child anchor
(parent (a) . (b))         ; soft adjacent siblings
(parent (a) .! (b))        ; exact adjacent siblings
(parent {. (a) (b) .})     ; anchors in sequence inside node
{(a) . (b)}                ; interior anchor (between items)
```

**Invalid positions:**

```
Q = . (a)                  ; definition level (no parent node)
Q = {. (a)}                ; sequence boundary without parent
Q = {(a) .}                ; sequence boundary without parent
Q = [(a) . (b)]            ; directly in alternation
```

To anchor within alternation branches, wrap in a sequence:

```
Q = [{(a) . (b)} (c)]      ; valid: anchor inside sequence branch
```

The rules:

- **Boundary anchors** (at start/end of sequence) need a parent named node to provide first/last child or adjacent sibling semantics
- **Interior anchors** (between items in a sequence) are always valid because both sides are explicitly defined
- **Alternations** cannot contain anchors directly — anchors must be inside a branch expression

---

## Named Expressions

Define reusable patterns:

```
BinaryOp =
  (binary_expression
    left: (_) @left
    operator: _ @op
    right: (_) @right)
```

Use as node types:

```
(return_statement (BinaryOp) @expr)
```

**Encapsulation**: `(Name)` matches but extracts nothing. You must capture (`(Name) @x`) to access fields. This separates structural reuse from data extraction.

Named expressions define both pattern and type:

```
Expr = [(BinaryOp) (UnaryOp) (identifier) (number)]
```

---

## Recursion

Named expressions can self-reference:

```
NestedCall =
  (call_expression
    function: [(identifier) @name (NestedCall) @inner]
    arguments: (arguments))
```

Matches `a()`, `a()()`, `a()()()`, etc. → `{ name: Node | null, inner: NestedCall | null }`

Tagged recursive example:

```
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

Root = (program (Statement)+ @statements)
```

Output types:

```typescript
type Statement =
  | { $tag: "Assign"; $data: { target: string; value: Expression } }
  | { $tag: "Call"; $data: { func: string; args: Expression[] } }
  | { $tag: "Return"; $data: { value: Expression | null } };

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

| Feature              | Tree-sitter        | Plotnik                   |
| -------------------- | ------------------ | ------------------------- |
| Capture              | `@name`            | `@name` (snake_case only) |
| Suppressive capture  |                    | `@_` or `@_name`          |
| Type annotation      |                    | `@x :: T`                 |
| Text extraction      |                    | `@x :: string`            |
| Named node           | `(type)`           | `(type)`                  |
| Anonymous node       | `"text"`           | `"text"`                  |
| Any node             | `_`                | `_`                       |
| Any named node       | `(_)`              | `(_)`                     |
| Field constraint     | `field: pattern`   | `field: pattern`          |
| Negated field        | `!field`           | `-field`                  |
| Quantifiers          | `?` `*` `+`        | `?` `*` `+`               |
| Non-greedy           |                    | `??` `*?` `+?`            |
| Sequence             | `((a) (b))`        | `{(a) (b)}`               |
| Alternation          | `[a b]`            | `[a b]`                   |
| Tagged alternation   |                    | `[A: (a) B: (b)]`         |
| Anchor               | `.`                | `.` soft, `.!` exact      |
| Predicate            | `(#eq? @x "foo")`  | `(node == "foo")`         |
| Regex predicate      | `(#match? @x "p")` | `(node =~ /p/)`           |
| Named expression     |                    | `Name = pattern`          |
| Use named expression |                    | `(Name)`                  |

---

## Diagnostics

Priority-based suppression: when diagnostics overlap, lower-priority ones are hidden. You see the root cause, not cascading symptoms.
