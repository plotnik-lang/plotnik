# Plotnik Query Language Reference

Plotnik is a pattern-matching language for Tree-sitter syntax trees. It extends [Tree-sitter's query syntax](https://tree-sitter.github.io/tree-sitter/using-parsers/queries/1-syntax.html) with definitions, recursion, and static type inference.

**Pattern.** A _pattern_ is a query matcher over the source syntax tree. Patterns nest — every pattern is built from sub-patterns — so the query AST is a tree of `Pattern` nodes. A named-node pattern `(kind)` matches a named node; an anonymous-node pattern `"text"` matches a literal token; `_` is the node wildcard. Sequences, alternations, quantifiers, grammar-field constraints, and captures are also patterns.

Tree-sitter predicates (`#eq?`, `#match?`) and directives (`#set!`) are not supported. Plotnik has its own inline predicate syntax (see [Predicates](#predicates)).

---

## Execution Model

NFA-based cursor walk with backtracking.

### Key Properties

- **Starts at the root**: An entry point begins at the supplied syntax-tree root; node patterns remain open, so unmentioned descendants are unconstrained
- **Backtracking**: Continuation failure restores a checkpoint and tries the remaining alternatives
- **Source-order priority**: Alternatives are tried left-to-right, but later alternatives remain available when the continuation fails
- **Empty matches are last resort**: a pattern that can match zero nodes (a
  `?` or `*` quantifier, a group of nullable patterns, or a reference to such a definition)
  succeeds empty only after node-consuming outcomes are exhausted

### Sibling Navigation

Plotnik has three sibling-navigation tiers:

1. **Default navigation is permissive.** Without an anchor, sibling patterns advance until they find a match, skipping named nodes, anonymous tokens, and Tree-sitter `extra` nodes such as comments.
2. **`.` narrows navigation.** It always skips extras. When both sides are named, it also skips anonymous tokens such as punctuation. When either side is anonymous, it skips extras only.
3. **`.!` is exact.** It allows no intervening syntax-tree node.

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

Bare `_` can match either a named or anonymous node, so `(a) . _` uses the
extras-only policy. `(_)` matches only named nodes, so `(a) . (_)` may skip
anonymous or extra nodes.

An anchor next to an alternation applies per alternative on both sides. Before: `(a) . [(b) ","]` uses named-named soft adjacency for `(b)` and extras-only adjacency for `","`. After a named follower: `[(b) ","] . (a)` uses named-named soft adjacency for the `(b)` path and extras-only for the `","` path, so adding an anonymous alternative no longer makes the named alternatives stricter. When the follower is itself anonymous (`[(b) ","] . ","`), extras-only applies to every alternative, since both-sides-named never holds. (Some advanced forms still stay extras-only on every alternative — a reference or captured follower, a captured alternation, a quantified alternative, or an alternative that is a sequence containing punctuation; see [Tree Navigation](tree-navigation.md).)

Use `.!` for exact adjacency:

```
(call_expression (identifier) @fn .! "(")  ; no intervening syntax-tree node
```

### Partial Matching

Node patterns are open — unmentioned children are ignored:

```
(binary_expression left: (identifier) @left)
```

Matches any `binary_expression` with an `identifier` in `left`, regardless of `operator`, `right`, etc.

Sequences `{...}` advance through siblings in order, skipping non-matching nodes.

### Grammar-Field Constraints

`field: pattern` requires the child to occupy that grammar field and match the pattern:

```
(binary_expression
  left: (identifier) @x
  right: (number) @y
)
```

Grammar fields participate in sequential matching — they're not independent lookups.

---

## File Structure

A `.ptk` file contains definitions:

````
```
; Helper (can also be used as an entry point because it matches one node)
Expr = [(identifier) (number) (string)]

; Another definition
Stmt = (statement) @stmt
````

Definitions whose root matches exactly one node are entry points. Sequence- and
quantifier-rooted definitions are fragments: they can be referenced or captured
inside an entry point, but `--entry <Name>` cannot select them directly. With no
`--entry`, the last selectable definition runs by default.

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

### Language

Set with `-l/--lang` or a shebang (`#!/usr/bin/env -S plotnik run -l <language>`); an explicit `-l` must agree with the shebang.

### Execution

- One selectable definition: it is the default entry point.
- Multiple selectable definitions: the **last selectable definition** is the default entry point; pass `--entry <Name>` to run a different one.
- Fragment definitions cannot be selected as entry points; nest or reference them from a selectable definition.

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
AllIdentifiers = (program (DeepSearch)* @found)
```

The capture on the reference is what produces the result field — `found: DeepSearch[]`. A bare `(DeepSearch)*` would match the same structure and return nothing.

---

## Naming Conventions

| Kind                       | Case         | Examples                             |
| -------------------------- | ------------ | ------------------------------------ |
| Definitions, labels, types | `PascalCase` | `Expr`, `Statement`, `BinaryOp`      |
| Node kinds                 | `snake_case` | `function_declaration`, `identifier` |
| Captures, grammar fields   | `snake_case` | `@name`, `func_body:`                |

Tree-sitter allows `@function.name`; Plotnik requires `@function_name` because captures map to result fields.

---

## Data Model

Plotnik infers result types from your query. See [Type System](type-system.md) for full details.

### Flat by Default

Query nesting does NOT create result nesting. Within a definition, all captures bubble up to the nearest scope boundary:

```
(function_declaration
  name: (identifier) @name
  body: (block
    (return_statement (expression) @retval)))
```

Result type:

```typescript
{ name: Node, retval: Node }  // flat, not nested
```

The pattern is 4 levels deep, but the result is flat. You're extracting specific pieces from a syntax tree, not reconstructing its shape.

Definitions are the exception: references are **opaque**. A bare `(Item)` matches structurally and produces nothing; `(Item) @item` produces the definition's type. Result fields never leak through a reference boundary. See [Type System: Definitions Are Types](type-system.md#definitions-are-types).

### Match-Only Definitions

A definition with no result-producing syntax is match-only. To return the matched root
node, capture it explicitly:

```
Program = (program)            ; Program is match-only
ProgramNode = (program) @root  ; ProgramNode is { root: Node }
MaybeProgram = (program)?      ; MaybeProgram is Node | null
Expr = [(identifier) (number)] ; Expr is match-only
Pair = {(identifier) (number)} ; Pair is match-only
```

If the body contains any capture, the captures define the result instead:

```
Q = (program (identifier) @id) ; Q is { id: Node }, not { $node, id }
```

Match-only definitions still match structurally, but they cannot be captured as
values. Add captures inside the definition, or capture a node pattern directly.

### Repeated Captures Need an Item Boundary

**A quantified pattern with inner captures needs a capture around each repeated
item.**

```
// ERROR: no boundary groups each iteration's result fields
(method_definition name: (identifier) @name)*

// OK: @method groups one iteration; @methods collects the list
{ (method_definition name: (identifier) @name) @method }* @methods
→ { methods: { method: Node, name: Node }[] }
```

This preserves record identity: each iteration produces one record instead of
parallel lists that lose the association between result fields.

Because references are opaque, repeating one is dimensionally simple — the definition's type is the element:

```
Item = (pair key: (_) @k value: (_) @v)
(program (Item)* @items)
→ { items: Item[] }
```

See [Type System: Repeated captures](type-system.md#repeated-captures-need-an-item-boundary).

### The Node Type

Default capture type — a reference to a Tree-sitter node:

```
interface Node {
  kind: string;           // e.g. "identifier"
  text: string;           // source text
  span: [number, number]; // half-open UTF-8 byte offsets
}
```

`infer --include-points` adds `startPoint` and `endPoint`. Each point has a
zero-based `row` and a zero-based, byte-based `column`.

### Cardinality: Quantifiers → Lists

Quantifiers determine whether a result field is a single value, an option, or a list:

| Pattern   | Result Type      | Meaning                  |
| --------- | ---------------- | ------------------------ |
| `(x) @a`  | `a: T`           | exactly one              |
| `(x)? @a` | `a: T \| null`   | zero or one              |
| `(x)* @a` | `a: T[]`         | zero or more (node list) |
| `(x)+ @a` | `a: [T, ...T[]]` | one or more (node list)  |

Every declared result field is **always present** in the result: an option-typed
field is `T | null` and materializes as `null` when it doesn't match (never an
absent key), and a list fallback is `[]` (never `null`). The result shape is stable.

Node lists work when the quantified pattern has **no internal captures**. For patterns with internal captures, collect records:

| Pattern         | Result Type       | Meaning              |
| --------------- | ----------------- | -------------------- |
| `{...}* @items` | `items: T[]`      | zero or more records |
| `{...}+ @items` | `items: [T, ...]` | one or more records  |
| `{...}? @item`  | `item: T \| null` | option of a record   |

The capture on the quantifier is required whenever the pattern has internal
captures — for `?` just like `*`/`+` (use `@_` to match structurally and
discard them).

### Creating Nested Records

Capture a sequence `{...}` or alternation `[...]` to create a new scope. Braces alone don't introduce nesting:

```
{
  (function_declaration
    name: (identifier) @name
    body: (_) @body
  ) @node
} @func
```

Result type:

```typescript
{ func: { node: Node, name: Node, body: Node } }
```

The `@func` capture on the sequence creates a nested scope. All captures inside (`@node`, `@name`, `@body`) become result fields of that nested object.

### Capture Types

`::` after a regular capture selects its capture type:

| Syntax       | Effect                                                |
| ------------ | ----------------------------------------------------- |
| `@x`         | Inferred (usually `Node`)                             |
| `@x :: str`  | Source text for the captured value                    |
| `@x :: bool` | Observable presence; an absent option becomes `false` |
| `@x :: Name` | Custom nominal name for the inferred type             |

`str` and `bool` are the complete lowercase built-in set. Any other lowercase
name is an error. Custom names must be `PascalCase`; `Str` is a custom name,
not the built-in `str`. The common spellings `string` and `boolean` are
diagnosed with fixes to `str` and `bool`.

A built-in capture type is applied only after the ordinary capture has been
validated, so it cannot legalize an invalid multi-node or no-value capture.
`str` recursively preserves option and list dimensions: `Node?` becomes
`string | null`, and `Node[]` becomes `string[]`. Every list item owns its
own document byte range. A composite value becomes the source slice from its first
matched node through its last; a valid zero-node value becomes `null`.

`bool` means presence, not truthiness. A present option becomes `true` and its
absent path becomes `false`; a required value is rejected unless the same
result field is omitted by an alternative. That alternative supplies `false`.

Replacing composite data with `str` or `bool` emits a warning.

Every composite type has a compiler-generated name already
(`{Parent}{Field}` along the capture path), so custom capture types are
optional. A custom name overrides the generated name and resets the chain —
nested composites derive from the new name. Names are nominal: the same name
on identical shapes denotes one shared type; on different shapes it is a
compile error. `Node` and definition names are reserved. See
[Type System: Capture Types](type-system.md#capture-types) and
[Type Naming](type-system.md#type-naming).

### Discards

`@_` (or the documenting form `@_name`) discards a pattern's result — the
subtree still matches structurally:

```
; Structure required, no result value
Q = (program
  (expression_statement (identifier) @x) @_
  (debugger_statement) @d
)
; Result: { d: Node }
```

One use is intentionally discarding a labeled alternation's case identity. If a
labeled alternation does not produce a value, its labels have no output effect
and the compiler warns. `@_` explicitly discards the result and silences that
warning:

```
(program [A: (expression_statement) B: (debugger_statement)] @_)
; matches, JSON result is null, no warning
```

Rules:

- `@_` and `@_name` match structurally but discard the captured result
- Named discards (`@_foo`) are equivalent to `@_` — the name is documentation only
- Captures inside a discarded subtree are inert; they never collide with same-named captures outside it
- Capture types are not allowed on discards
- Nesting works: `@_outer` containing `@_inner` correctly suppresses both

### Summary

| Pattern                 | Result                                  |
| ----------------------- | --------------------------------------- |
| `@name`                 | Result field in current scope           |
| `(x)? @a`               | Result field with option type           |
| `(x)* @a`               | Node list (no internal captures)        |
| `{...}* @items`         | Record list (with internal captures)    |
| `{...} @x` / `[...] @x` | Nested object (new scope)               |
| `(Def)`                 | Structural match, no result value       |
| `(Def) @x`              | Definition type, or error if match-only |
| `(Def)* @xs`            | List of the definition's type           |
| `[...] @_`              | Match and discard                       |
| `@x :: str`             | Source text, preserving `?`/`*`/`+`     |
| `@x :: bool`            | Presence boolean                        |
| `@x :: T`               | Custom type name                        |

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

Result type:

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

**Regex patterns** use `/pattern/` syntax. Full Unicode is supported. Patterns match anywhere in the text (use `^` and `$` anchors for full-match semantics). `\b` and `\B` use ASCII word characters on every target. Case-insensitive mode (`(?i)`) is expanded at generation time against Plotnik's pinned Unicode tables rather than delegated to the host regex engine.

```
(identifier =~ /^test_/)      ; starts with "test_"
(identifier =~ /Handler$/)    ; ends with "Handler"
(identifier =~ /^[A-Z][a-z]+(?:[A-Z][a-z]+)*$/)  ; PascalCase
```

**Unsupported regex features** (compile-time error):

- Backreferences (`\1`, `\2`)
- Lookahead/lookbehind (`(?=...)`, `(?!...)`, `(?<=...)`, `(?<!...)`)
- Named captures (`(?P<name>...)`)
- Multiline and CRLF modes (`(?m)`, `(?R)`, including scoped forms)
- Word-boundary variants (`\<`, `\>`, `\b{start}`, `\b{end}`, and half-boundary forms)

Predicates don't affect result types — they're structural constraints like anchors.

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

Result type:

```typescript
{
  op: Node;
}
{
  keyword: Node;
}
```

### String Escapes

String literals — anonymous nodes and predicate values alike — support these escapes:

| Escape  | Meaning                                      |
| ------- | -------------------------------------------- |
| `\n`    | newline                                      |
| `\r`    | carriage return                              |
| `\t`    | tab                                          |
| `\\`    | backslash                                    |
| `\"`    | double quote                                 |
| `\'`    | single quote                                 |
| `\u{…}` | Unicode scalar, 1-6 hex digits (`\u{1F600}`) |

Any other `\` sequence is a compile-time error. Regex literals are unaffected — `/…/`
patterns follow regex escaping rules.

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
- `(MISSING)` — matches any node inserted by error recovery
- `(MISSING identifier)` — matches a specific missing token kind
- `(MISSING ";")` — matches a missing anonymous token

Both match as leaves: a missing node has an empty document byte range, so neither
`ERROR` nor `MISSING` accepts children. Because error recovery only ever inserts
tokens, a `(MISSING kind)` argument must name a leaf token — a kind with children
like `(MISSING binary_expression)` can never match and is rejected at compile time.

```
(ERROR) @syntax_error
(MISSING ";") @missing_semicolon
```

Result type:

```typescript
{
  syntax_error: Node;
}
{
  missing_semicolon: Node;
}
```

### Supertypes

Supertypes (abstract node kinds like `expression`) cannot be matched yet. Both the bare
form `(expression)` and the `#subtype` refinement `(expression#binary_expression)` are
rejected at compile time; the `#` syntax is reserved for a future release. Match the
concrete subtypes with an alternation instead:

```
[(binary_expression) (unary_expression)] @expr
```

The separator is tight-binding — no whitespace around `#`. The Tree-sitter spelling
`expression/binary_expression` is also accepted but deprecated in favor of `#`.

---

## Grammar Fields

Constrain children to named grammar fields. A grammar-field value must be a node pattern, an alternation, or a quantifier applied to one of these. Sequences `{...}` are not allowed as direct grammar-field values.

```
(assignment_expression
  left: (identifier) @target
  right: (call_expression) @value)
```

Result type:

```typescript
{ target: Node, value: Node }
```

### Quantifiers and Captures on Grammar Fields

Quantifiers and captures after a grammar-field value apply to the entire grammar-field constraint, not just the value:

```
decorator: (decorator)* @decorators   ; repeats the whole grammar field
value: [A: (x) B: (y)] @kind          ; captures the grammar-field value
```

This allows repeating grammar fields (useful for things like decorators in JavaScript).
The capture still produces the grammar-field value's inferred type. A labeled
alternation therefore produces its variant type, not a raw node.

### Negated Grammar Fields

Assert a grammar field is absent with `-`:

```
(function_declaration
  name: (identifier) @name
  -type_parameters)
```

Negated grammar fields don't affect the result type — they're purely structural constraints:

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

Result types:

```typescript
{ decorator: Node | null }
{ decorators: Node[] }
{ decorators: [Node, ...Node[]] }
```

The `+` quantifier always produces a non-empty list — no opt-out.

Plotnik also supports lazy forms: `*?`, `+?`, `??`

A repeat iteration must consume a syntax-tree node. When the element can itself match
zero nodes — a reference to a definition rooted at `?`, or an alternation
with a nullable alternative — only its node-consuming matches become elements:

```
A = (expression_statement (identifier) @id)? @x
Q = (program (A)* @xs)    ; xs collects one record per real match;
                          ; non-matching statements are skipped, not
                          ; collected as { x: null } records
```

`(A)+` likewise requires at least one real match; an empty outcome never
satisfies it.

---

## Sequences

Match sibling patterns in order with braces.

> **⚠️ Syntax Difference from Tree-sitter**
>
> Tree-sitter: `((a) (b))` — parentheses for sequences
> Plotnik: `{(a) (b)}` — braces for sequences
>
> This avoids ambiguity: `(foo)` is always a node, `{...}` is always a sequence.
> Using Tree-sitter's `((a) (b))` syntax in Plotnik is a parse error.

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

Result type:

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

Result type:

```typescript
interface Section {
  fn: Node;
}

{ sections: [Section, ...Section[]] }
```

---

## Alternations

Match alternatives with `[...]`:

- An **unlabeled alternation** merges result fields from its alternatives.
- A **labeled alternation** names the cases of a variant value.

An alternative that can match zero nodes (`[(a)? (b)]`) succeeds empty only
as a last resort: every alternative's node-consuming match, at any candidate
position, is preferred first. The empty outcome needs no candidate at all — it
matches even in an empty parent — and leaves the cursor in place for any
following pattern. In an unlabeled alternation it yields every merged result field at
its fallback (`null`, `[]`, or `false`); in a labeled alternation it selects a
case whose payload result fields use the same fallbacks.

```
[
  (identifier)
  (string_literal)
] @value
```

### Unlabeled Alternations

Captures merge: a result field produced by every alternative is required; a result field
produced by only some alternatives receives its fallback. Same-name captures
must have compatible types.

Alternatives must be type-compatible. A direct capture contributes its result
result field alongside result fields from structured alternatives.

```
(statement
  [
    (assignment_expression left: (identifier) @left)
    (call_expression function: (identifier) @func)
  ])
```

Result type:

```typescript
{ left: Node | null, func: Node | null }  // each appears in one alternative only
```

When the same capture appears in every alternative:

```
[
  (identifier) @name
  (string) @name
]
```

Result type:

```typescript
{
  name: Node;
} // required: present in every alternative with the same type
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

The second alternative `(identifier) @x` contributes the same `x` result field as the
first alternative.

Result type:

```typescript
{ x: Node, y: Node | null }  // x in every alternative; y in one
```

Type mismatch is an error:

```
[(identifier) @x :: Foo (number) @x :: Bar]  // ERROR: @x has different types
```

With a capture on the alternation itself, the type is not an option because one
alternative must match:

```
[
  (identifier)
  (number)
] @value
```

Result type:

```typescript
{
  value: Node;
}
```

### Labeled Alternations

Labels name cases of a variant type. TypeScript renders the variant as a
discriminated union; JSON uses `$tag` and, for payload-bearing cases, `$data`:

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

The cases materialize when the alternation produces a value: when it is
captured (`[...] @x`), collected (`[...]* @xs`), or used as a definition body
(`Expr = [...]`). If no surrounding construct materializes the alternation,
its captures merge into the enclosing record and the compiler warns that the
labels have no output effect.

An alternative with no captures becomes a no-payload case (`{ $tag: "..." }`,
with no `$data`). A variant containing only such cases is useful when the case
identity is the result. A bare reference as an alternative body is also
no-payload; capture it (`[Call: (Inner) @data]`) to carry the definition's
value.

### Alternation Type Names

A captured alternation that produces a record gets a generated path name like
any other composite; use a capture type to override it:

```
Q = (call_expression
  function: [
    (identifier) @fn
    (member_expression property: (property_identifier) @method)
  ] @target)
```

Result type:

```typescript
interface QTarget {
  fn: Node | null;
  method: Node | null;
}

interface Q {
  target: QTarget;
}
```

---

## Anchors

Anchors constrain sibling positions. They don't affect types — they're structural constraints.

### Anchor Modes

`.` is soft adjacency: it skips extras and disallows other named nodes between operands. When both sides are named, it also skips anonymous tokens. `.!` is exact: it allows no intervening syntax-tree node.

| Pattern      | Extras Between | Anonymous Nodes Between | Named Nodes Between |
| ------------ | -------------- | ----------------------- | ------------------- |
| `(a) . (b)`  | Allowed        | Allowed                 | Disallowed          |
| `"x" . (b)`  | Allowed        | Disallowed              | Disallowed          |
| `(a) . "x"`  | Allowed        | Disallowed              | Disallowed          |
| `"x" . "y"`  | Allowed        | Disallowed              | Disallowed          |
| `(a) .! (b)` | Disallowed     | Disallowed              | Disallowed          |

Extras are nodes Tree-sitter marks with the per-node `is_extra` bit; there is no
bytecode table for extras or the combined anonymous-or-extra navigation class.

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

For exact syntax-tree adjacency:

```
(call_expression (identifier) @fn .! "(")
```

Here, no syntax-tree node may occur between the function name and the opening parenthesis because `.!` requests exact adjacency.

### Anchors After Nullable Items

When the item before an anchor is nullable (`?` or `*`), the anchor's meaning depends on whether that item matched:

```
(program {(lexical_declaration)? @a . (debugger_statement) @b})
```

- **When `@a` matches**, the anchor is enforced between the two siblings: `@b` must be the adjacent sibling, so `let x; debugger;` matches but `let x; foo; debugger;` does not.
- **When `@a` is skipped**, the anchor degrades to a leading anchor relative to the parent — as if the query were `(program . (debugger_statement) @b)`. `@b` must be the first child aside from anonymous or extra nodes: `debugger;` and `/* c */ debugger;` match, but `foo; debugger;` does not.

The exact constraint carries through both paths: with `.!`, no intervening
syntax-tree node is tolerated on either the adjacency or the leading interpretation.

The anchor pins where a **quantified follower** begins, not just a single node:

```
(program {(lexical_declaration)? @a . (debugger_statement)* @b . (expression_statement) @c})
```

The anchor before `(debugger_statement)*` fixes its starting position the same way — adjacent to `@a` when present, the first child when `@a` is skipped. The repeated matches are then **back-to-back** under the anchor's skip policy and stop at the first disallowed gap. So with `@a` skipped, `debugger; debugger; foo;` collects both debuggers, but `bar; debugger; foo;` does not match — `@b` may not skip past `bar;` to start later.

A **trailing anchor** combines with the leading interpretation rather than overriding it:

```
(program {(lexical_declaration)? @a . (debugger_statement) @b .})
```

When `@a` is skipped, `@b` must be both the first child (leading anchor) and the last child (trailing anchor) — so only `debugger;` alone matches, while `foo; debugger;` (not first) and `debugger; foo;` (not last) do not.

### Anchors Over Empty Matches

When every item in a child list is nullable and none of them matches, there is no matched child for an anchor to bind to. A leading or trailing anchor then degrades to an assertion about the node itself: it has no children the anchor's skip policy would reject — none beyond the nodes admitted by `.` and none at all for `.!` (the policy is chosen the same way as between siblings). When both anchors are present, the stricter one applies.

```
(program {(debugger_statement)* @b .})
```

With zero repetitions this matches an empty program or one holding only comments (`b` is `[]`), but not `foo;` — an unskippable child remains.

The degenerate form is a body of anchors alone, which is the idiomatic emptiness check:

```
(statement_block .)     ; no statements (braces and comments aside)
(program .!)            ; a completely empty file
```

### Result Types

Anchors are structural constraints only — they don't affect result types:

```typescript
{ first: Node }
{ last: Node }
{ a: Node, b: Node }
```

Anchors are not values and do not appear in result types.

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

To anchor within alternatives, wrap the anchored patterns in a sequence:

```
Q = [{(a) . (b)} (c)]      ; valid: anchor inside a sequence alternative
```

The rules:

- **Boundary anchors** (at start/end of sequence) need a parent named node to provide first/last child or adjacent sibling semantics
- **Interior anchors** (between items in a sequence) are always valid because both sides are explicitly defined
- **Alternations** cannot contain anchors directly — anchors must be inside an alternative's sequence

---

## Definitions

Define reusable patterns:

```
BinaryOp =
  (binary_expression
    left: (_) @left
    operator: _ @op
    right: (_) @right)
```

Use as node kinds:

```
(return_statement (BinaryOp) @expr)
```

**Encapsulation**: `(Name)` matches but extracts nothing. Capture the reference to get the definition's typed result — `(BinaryOp) @expr` above produces `{ expr: BinaryOp }` where `BinaryOp` is `{ left: Node, op: Node, right: Node }`. This separates structural reuse from data extraction, and it means extracting a pattern into a definition never silently changes your result.

Definitions name both a reusable pattern and, when the body produces a value,
its result type. A match-only single-node definition has no result value; capture
the root node when the definition should carry a value:

```
Expr = [(identifier) (number)] ; match-only: structural only
ExprNode = [(identifier) (number)] @expr ; returns { expr: Node }
(statement (Expr))     ; matches any statement containing an Expr, no result value
```

---

## Recursion

Definitions can reference themselves:

```
NestedCall =
  (call_expression
    function: [(identifier) @name (NestedCall) @inner]
    arguments: (arguments))
```

Matches `a()`, `a()()`, `a()()()`, etc. → `{ name: Node | null, inner: NestedCall | null }`

Recursive variant example:

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
    left: (identifier) @target
    right: (Expression) @value)
  Call: (call_expression
    function: (identifier) @func
    arguments: (arguments (Expression)* @args))
  Return: (return_statement
    (Expression)? @value)
]

Expression = [
  Ident: (identifier) @name
  Num: (number) @value
  Str: (string) @value
]

Root = (program (Statement)+ @statements)
```

Result types:

```typescript
export type Statement =
  | { $tag: "Assign"; $data: { target: Node; value: Expression } }
  | { $tag: "Call"; $data: { args: Expression[]; func: Node } }
  | { $tag: "Return"; $data: { value: Expression | null } };

export interface Root {
  statements: [Statement, ...Statement[]];
}

export type Expression =
  | { $tag: "Ident"; $data: { name: Node } }
  | { $tag: "Num"; $data: { value: Node } }
  | { $tag: "Str"; $data: { value: Node } };
```

Variant types render as one multi-line TypeScript union with inline cases; case
payloads never get standalone declarations.

---

## Quick Reference

| Feature                  | Tree-sitter        | Plotnik                     |
| ------------------------ | ------------------ | --------------------------- |
| Capture                  | `@name`            | `@name` (snake_case only)   |
| Discard                  |                    | `@_` or `@_name`            |
| Capture type             |                    | `@x :: str`, `bool`, or `T` |
| Named node               | `(type)`           | `(type)`                    |
| Anonymous node           | `"text"`           | `"text"`                    |
| Any node                 | `_`                | `_`                         |
| Any named node           | `(_)`              | `(_)`                       |
| Grammar-field constraint | `field: pattern`   | `field: pattern`            |
| Negated grammar field    | `!field`           | `-field`                    |
| Quantifiers              | `?` `*` `+`        | `?` `*` `+`                 |
| Lazy                     |                    | `??` `*?` `+?`              |
| Sequence                 | `((a) (b))`        | `{(a) (b)}`                 |
| Alternation              | `[a b]`            | `[a b]`                     |
| Labeled alternation      |                    | `[A: (a) B: (b)]`           |
| Anchor                   | `.`                | `.` soft, `.!` exact        |
| Predicate                | `(#eq? @x "foo")`  | `(node == "foo")`           |
| Regex predicate          | `(#match? @x "p")` | `(node =~ /p/)`             |
| Definition               |                    | `Name = pattern`            |
| Definition reference     |                    | `(Name)`                    |

---

## Diagnostics

Priority-based suppression: when diagnostics overlap, lower-priority ones are hidden. You see the root cause, not cascading symptoms.
