# Plotnik Type System

Plotnik infers static types from query structure. Types — including their
names — are retained in the target-neutral compiled query. Rust and TypeScript
type emission project those same facts directly; bytecode is only constructed
when the VM target is explicitly selected.

## The Output Model

**Output exists where output syntax is written.** Definitions name whatever
result their body produces, but they do not implicitly capture the matched
root node. Four constructs produce or name output:

| Syntax      | Output                                               |
| ----------- | ---------------------------------------------------- |
| `@name`     | A field in the enclosing scope                       |
| `Def = ...` | A named type for the body's result                   |
| `Label:`    | A variant case when the alternation produces a value |
| `:: type`   | A built-in or custom capture type                    |

Everything else — nested node patterns, sequences, references, anchors,
predicates — is structural unless one of those output positions materializes it.
To return the root node of a definition, capture it explicitly.

## Definitions Are Types

A definition is both a reusable pattern and a named type. References to it are
**opaque**: fields never leak through a reference boundary.

```
Item = (expression_statement (identifier) @id)

(program (Item))              ; matches structurally, no output
(program (Item) @item)        ; { item: Item }
(program (Item)* @items)      ; { items: Item[] }
(program (Item)? @item)       ; { item: Item | null }
```

```typescript
export interface Item {
  id: Node;
}
```

- A **bare reference** `(Item)` matches the definition's pattern and discards
  its output — silently, by design. Use it for purely structural constraints.
- A **captured reference** `(Item) @x` produces the definition's result type.
  If the definition is match-only, the capture is rejected because there is no value
  to bind.
- This is uniform for recursive and non-recursive definitions, so extracting a
  pattern into a definition never silently changes your output shape — you
  always say `@x` where you want the value.

### Match-Only Definitions

A definition whose body produces no output is match-only:

- A single node root — named, anonymous, wildcard, with fields, predicates, or
  anchors — matches structurally and returns no data.
- An unlabeled alternation of match-only node alternatives also returns no data.
- A sequence root is match-only because no output syntax materializes it.
- A labeled alternation used as a definition body produces a variant value;
  no-payload cases remain no-payload.
- A `?`, `*`, or `+` root returns the optional/list container described below.

Captures define the result; there is no hybrid `{ $node, ... }` output.

```
Program = (program)                         ; Program = undefined
ProgramNode = (program) @root               ; ProgramNode = { root: Node }
MaybeProgram = (program)?                   ; MaybeProgram = Node | null
Expr = [(identifier) (number)]              ; Expr = undefined
Pair = {(identifier) (number)}              ; Pair = undefined
Named = (program (identifier) @id)          ; Named = { id: Node }
```

A definition is **match-only** when its body produces no output:

```
Id = (identifier) @id
Foo = (function_declaration name: (Id)) ; bare ref only → Foo is match-only
```

```typescript
export interface Id { id: Node; }
export type Foo = undefined;              ; TypeScript representation of match-only output
```

There is no pure type aliasing: `Foo = (Id)` does not re-export `Id`'s type.

### Quantifier-Rooted Definitions

A definition body is a result boundary. A quantifier standing as the whole
body therefore becomes the definition's result container:

```
Ids = (identifier)*          ; Ids = Node[]
MaybeId = (identifier)?      ; MaybeId = Node | null
Row = (pair key: (_) @k value: (_) @v)
Rows = (Row)*                ; Rows = Row[]
First = (Row)?               ; First = Row | null
```

```typescript
export type Ids = Node[];
export type Rows = Row[];
export type First = Row | null;
```

References stay opaque at call sites: `(Rows) @rows` → `rows: Rows`, bare
`(Rows)` is structural, `(Rows)* @xss` → `xss: Rows[]`. An optional-rooted
definition under a call-site `?` nests: `(MaybeId)? @x` → `x: MaybeId | null`
(both nulls print the same in JSON).

Containers never mint type names — names come only from definitions,
captures, custom capture types, and variant cases. So the element must already be
a nameable type: a plain node, or a reference. An anonymous element shape is
rejected; name it in its own definition:

```
Bad = {(key) @k (value) @v}*      ; ERROR: the element row has no type name
Row = (pair key: (_) @k value: (_) @v)
Good = (Row)*                     ; Good = Row[]
```

Two consequences:

- A definition whose root is `*` or `?` can match zero nodes, but a repeat
  iteration must consume a syntax-tree node — so repeating a reference to it
  (`(MaybeId)*`) is rejected: the wrapper's empty case could never occur
  under the repeat, and the intent is clearer with the quantifier in one
  place.
- A quantifier-rooted definition is a fragment, not an entry point. To run it,
  nest it under a one-node root and capture the collection:
  `Q = (program (identifier)* @items)`.

## Scope Model

Within a definition, captures bubble up through query nesting to the nearest
scope boundary:

```
(function_declaration
  name: (identifier) @name
  body: (statement_block
    (return_statement (_) @retval)))
→ { name: Node, retval: Node }    ; flat, not nested
```

Transparent (captures bubble through):

- Node patterns `(kind ...)`
- Uncaptured sequences `{...}`
- Uncaptured alternations `[...]`

Boundaries (a new scope starts):

- **Captured sequences** `{...} @x` → nested record
- **Captured alternations** `[...] @x` → merged record or variant value
- **Definitions** — references are opaque (see above)
- **Discard** `@_` — discards the whole subtree's output

```
{
  (expression_statement) @s
} @info
→ { info: { s: Node } }           ; @info creates a nested scope
```

A captured sequence _without_ internal captures is only meaningful when it
matches exactly one node — the capture takes that node (`{(a)} @x` ≡
`(a) @x`). See the multi-node rule below.

## Repeated Captures Need an Item Boundary

Two rules keep repetition and captures honest:

**1. A quantifier's internal captures must be collected by a capture on the
quantifier.** All quantifiers, uniformly — `*`/`+` collect a list of rows, `?`
collects one nullable row; `@_` discards:

```
{(key) @k (value) @v}*            ; ERROR: captures repeat, nothing collects them
{(key) @k (value) @v}* @entries   ; OK: entries: { k: Node, v: Node }[]
{(key) @k (value) @v}?            ; ERROR: captures skip together, nothing collects them
{(key) @k (value) @v}? @entry     ; OK: entry: { k: Node, v: Node } | null
(func (id) @name)*                ; ERROR: same rule through node patterns
(func (id) @name)? @fn            ; OK: fn: { name: Node } | null
```

**2. A capture needs exactly one node or a value.** A match-only pattern under a
capture must match exactly one node — several, or possibly none, and there is
no single node to bind. Both directions fail for the same reason:

```
{(a) (b)} @x                      ; ERROR: two nodes — which one is x?
{(a)+} @x                        ; ERROR: a variable run of nodes
{(a)?} @x                        ; ERROR: possibly no node at all
[(a)+ (b)] @x                    ; ERROR: one alternative is a run
(SeqDef) @x                       ; ERROR when SeqDef is match-only
```

Greedy versus lazy preference never matters here: `?` and `??` (and
`*`/`*?`, `+`/`+?`) are identical to the type system. At runtime, they differ
only in whether another repetition or the continuation is tried first.

The fix is always to say what you mean: capture individual nodes inside the
group, capture the quantifier directly (`(a)+ @xs` → a list), or capture
nothing.

References are opaque, so a quantified reference already has an item boundary:
the definition's _type_ is the element, no matter how many captures it contains:

```
Item = (pair key: (_) @k value: (_) @v)
(Item)* @items                    ; OK: items: Item[]
(Item)*                           ; OK: structural repeat, no output
```

### Node Lists vs Record Lists

```
(identifier)* @ids                ; node list:   ids: Node[]
{(a) @a (b) @b}* @rows            ; record list: rows: { a: Node, b: Node }[]
(Item)+ @items                    ; reference list: items: [Item, ...Item[]]
```

### Optional Rows

A captured optional group is one optional record — the `?` counterpart of a
record list:

```
{(modifier) @mod (decorator) @dec}? @attrs
→ { attrs: { mod: Node, dec: Node } | null }
```

The fields keep their true modality — if the row matched, both are present —
and a skip yields `attrs: null`, never a hollow `{ mod: null, dec: null }`.
A quantified named node collects the same way: `(pair (key) @k)? @p` gives
`p: { k: Node } | null`, mirroring `(pair (key) @k)* @ps` rows.

There is no uncaptured fallback (the item-boundary rule): a bare
`{(mod) @mod (dec) @dec}?` would scatter correlated nulls into the enclosing
scope as independently-optional fields — a type that permits states the match
can never produce. For a single optional node with no wrapper, put the capture
on the quantifier: `(decorator)? @dec` → `dec: Node | null`. To match
structurally and drop the captures, discard them: `{...}? @_`.

### Null, Not Absent

Every declared field is **always present** in the output. An optional field
renders as `T | null` and materializes as `null` when it doesn't match — never
as a missing key. Missing **lists** are the empty array `[]`, never `null`.
The output shape is stable; consumers never guard for `undefined`.

## Cardinality

| Pattern   | Output Type      | Meaning      |
| --------- | ---------------- | ------------ |
| `(A) @a`  | `a: T`           | exactly one  |
| `(A)? @a` | `a: T \| null`   | zero or one  |
| `(A)* @a` | `a: T[]`         | zero or more |
| `(A)+ @a` | `a: [T, ...T[]]` | one or more  |

`T` is `Node` for plain patterns, the definition's type for references, the
record for captured groups, or the variant type for labeled alternations.

## Alternations

`[...]` matches one of several alternatives. Labels affect the result only
when the alternation produces a value.

### Unlabeled Alternations

Captures from the alternatives merge into one record, one level deep:

- A capture present in **every** alternative → required field.
- A capture present in **some** alternatives → its fallback (`null`, `[]`, or `false`).
- A missing **list** in an alternative → `[]`, not `null`.
- The same capture must have the same type in every alternative; a mismatch is an
  error (`capture @x has incompatible types across alternatives`). Cardinality
  counts: a `+` list and a `*` list do not unify.
- A bare node beside record-producing alternatives is fine — it simply
  contributes no fields (or its own capture, if any).
- A bare reference is a structural alternative: it
  contributes nothing to the merged record.

```
[
  (binary_expression left: (_) @x right: (_) @y)
  (identifier) @x
]
→ { x: Node, y: Node | null }
```

A capture on the alternation itself takes the selected alternative's value;
when every alternative is a single match-only node pattern, that value is the
matched node:

```
[(identifier) (number)] @value    → { value: Node }
```

### Labeled Alternations

Alternative labels name cases of a variant type. The cases materialize when
the alternation produces a value — when it is captured, collected, or used as
a definition body:

```
Expr = [                          ; definition body produces a value
  Lit: (number) @value
  Neg: (unary_expression (Expr) @inner)
]
```

```typescript
export type Expr =
  | { $tag: "Lit"; $data: { value: Node } }
  | { $tag: "Neg"; $data: { inner: Expr } };
```

Case payloads come from the selected alternative's bubbling captures:

- Captures → `$data` is an anonymous record (always inlined, never a named
  standalone type).
- No captures → the case has no payload and omits `$data` entirely. A variant
  may contain only no-payload cases when the case identity is the result.
- A bare reference (or `@_`) as an alternative body → no-payload case.
  `[Call: (Inner)]` tags the case; `[Call: (Inner) @data]` also carries the
  value.

```
[Stmt: (expression_statement)  Decl: (lexical_declaration)] @kind
→ kind: { $tag: "Stmt" } | { $tag: "Decl" }
```

### Labels Without a Result Value

A labeled alternation that no surrounding construct materializes has no value
to tag. Its captures merge into the enclosing record as they do for an
unlabeled alternation, and the compiler warns:

```
(program [A: (expression_statement) @e  B: (debugger_statement) @d])
```

```
warning: alternative labels have no output effect here
help: capture the alternation (`[...] @name`) to make its labels produce
      variant cases, or remove them
→ { e: Node | null, d: Node | null }
```

Mixing labeled and unlabeled alternatives in one `[...]` is an error.

## Discards: `@_`

`@_` (or `@_name` — the name is documentation) discards a pattern's result.
The subtree matches structurally; captures inside it are inert, labels remain
valid but produce nothing, and no unused-label warning fires:

```
(program [A: (expression_statement) B: (debugger_statement)] @_)
→ matches, output is null; no warning
```

Discarded captures never collide with returned ones: a `@x` inside a discarded
subtree does not touch a `@x` outside it. Quantifiers under `@_` carry no
item-boundary requirement — nothing is collected, so nothing can be lost.

## Recursion

Definitions can reference themselves (or each other) when every cycle both
**escapes** and **makes progress**:

1. **Escape**: some alternative must terminate without recursing.
2. **Progress**: each pass around the cycle must match at least one syntax-tree
   node before recursing.

```
Loop = (Loop)
; ERROR: infinite recursion: no escape path

A = [X: (identifier) @i  Y: (B) @b]
B = (A)
; ERROR: infinite recursion: cycle makes no progress

A = (parenthesized_expression (B))
B = (array (A))
; ERROR: no escape path — progress is not enough; some alternative must terminate

MemberChain = [
  Base: (identifier) @name
  Access: (member_expression
    object: (MemberChain) @object
    property: (property_identifier) @property)
]
; OK: Base escapes, Access matches a member_expression before recursing
```

```typescript
export type MemberChain =
  | { $tag: "Base"; $data: { name: Node } }
  | { $tag: "Access"; $data: { object: MemberChain; property: Node } };
```

A recursive reference in an unlabeled alternation works the same way:

```
NestedCall = (call_expression
  function: [(identifier) @name (NestedCall) @inner]
  arguments: (arguments))
→ { name: Node | null, inner: NestedCall | null }
```

## Capture Types

Analysis first infers and validates the ordinary capture, then applies the
written capture type. Removing `:: str` or `:: bool` therefore always leaves
an independently valid capture.

### Built-in `str`

`str` replaces each terminal value with the source range owned by that value
and preserves its existing containers:

| Ordinary type     | `:: str` result     |
| ----------------- | ------------------- |
| `Node`            | `string`            |
| `Node \| null`    | `string \| null`    |
| `Node[]`          | `string[]`          |
| nonempty `Node[]` | nonempty `string[]` |
| record/variant    | `string` (warning)  |

List items keep distinct ranges; trivia between items belongs to neither.
A composite value uses the smallest source span containing its contributing
nodes. An admitted empty value is `null`, while a real zero-byte node is the
present empty string `""`.

A capture on a non-leaf node replaces only that capture's `Node` value. Child
captures that bubble independently into the enclosing scope remain ordinary
fields. By contrast, converting a captured sequence or labeled alternation
replaces the composite data it owns and warns once at the written capture type.

### Built-in `bool`

`bool` exposes observable absence; it never reads or interprets captured text:

```text
absent  -> false
present -> true
```

An optional non-boolean value becomes `boolean`. Nested optionals collapse to
one boolean. A required node, list, record, or variant is rejected because the
result would always be `true`, unless an alternative omits that exact field. In
that case the capture observes alternative presence and the omitted alternative
receives `false`, not `null`. `bool` never means `any()` and does not map list
elements.

### Custom capture types

PascalCase capture types assign nominal names without changing the underlying
value. `str` and `bool` are the only lowercase built-ins; `:: Str` and
`:: Bool` are custom names. A custom name may name a captured node or a
composite shape; it cannot name the result of a built-in capture type because
each capture has only one capture-type position.

## Type Naming

Every named result type gets its name at compile time. Names are deterministic
and complete in the bytecode.

### Path Names

A definition's result is named after the definition. Composite types created
by captures are named `{ParentTypeName}{PascalCase(field)}`, following the
capture path; lists and options are transparent (the name lands on the
element):

```
Foo = (function_declaration
  body: (statement_block {
    (expression_statement {(identifier) @v} @inner)
  } @items)
)
```

```typescript
export interface FooItemsInner {
  v: Node;
}
export interface FooItems {
  inner: FooItemsInner;
}
export interface Foo {
  items: FooItems;
}
```

Variant case payloads are anonymous (inlined), so they take no name; a
composite _inside_ a payload field is named
`{VariantName}{VerbatimLabel}{PascalCase(field)}`:

```
Q = (program [
  Stmt: (expression_statement {(identifier) @name} @info)
  DECL: (lexical_declaration) @node
] @item)
```

```typescript
export type QItem =
  | { $tag: "Stmt"; $data: { info: QItemStmtInfo } }
  | { $tag: "DECL"; $data: { node: Node } };
```

Labels are used **verbatim** (`DECL` stays `DECL`), so two labels can never
collide after case conversion.

### Custom `:: Name` Capture Types

`@x :: Name` overrides the generated name **and resets the chain** — children
derive from the new name:

```
Foo = (function_declaration
  body: (statement_block {
    (expression_statement {(identifier) @v} @inner)
  } @outer :: Bar)
)
```

```typescript
export interface BarInner {
  v: Node;
}
export interface Bar {
  inner: BarInner;
}
export interface Foo {
  outer: Bar;
}
```

On a plain node capture, `:: Name` declares a named alias:

```
(identifier) @x :: MyName    →  export type MyName = Node;
                                 { x: MyName }
```

### Names Are Nominal

- The same custom capture type name on **structurally identical** types denotes one
  type — annotate two identical shapes `:: Info` and both fields share
  `Info`, declared once.
- The same name on **different** shapes is a compile error, reported with
  both spans (`type name X is already used for a different type`).
- Definition names and the builtin `Node` are reserved; `Node = ...` is an
  error.
- A custom capture type that matches the name the compiler would generate anyway is a
  warning (`omit it`), as is one on a reference (a definition's type
  cannot be renamed at a use site).
- There is no numeric suffixing — a name conflict is always an error, never a
  silent rename.

## TypeScript Rendering

- Records render as named `interface`s.
- Variant types render as one multi-line union literal with inline cases; case
  payloads never get standalone declarations.
- Match-only queries render as `export type Q = undefined;` — the query matches
  or not, and carries no data.
- Optional fields are `T | null` (always present), non-empty lists are
  `[T, ...T[]]`.

```typescript
export type Statement =
  | { $tag: "Assign"; $data: { target: Node; value: Expression } }
  | { $tag: "Call"; $data: { args: Expression[]; func: Node } }
  | { $tag: "Return"; $data: { value: Expression | null } };

export interface Root {
  statements: [Statement, ...Statement[]];
}
```
