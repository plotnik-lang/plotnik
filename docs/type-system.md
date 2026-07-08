# Plotnik Type System

Plotnik infers static types from query structure. Types — including their
names — are computed at compile time and stored in the bytecode; typegen and
the JSON materializer are pure renderers of that information.

## The Output Model

**Output exists where output syntax is written.** Definitions name whatever
result their body produces, but they do not implicitly capture the matched
root node. Four constructs produce or name output:

| Syntax        | Output                                       |
| ------------- | -------------------------------------------- |
| `@name`       | A field in the enclosing scope               |
| `Def = ...`   | A named type for the body's result           |
| `Label:`      | An enum variant (when the value is consumed) |
| `:: TypeName` | A name for the type at that position         |

Everything else — nested node patterns, sequences, references, anchors,
predicates — is structural unless one of those output positions consumes it.
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
  If the definition is void, the capture is rejected because there is no value
  to bind.
- This is uniform for recursive and non-recursive definitions, so extracting a
  pattern into a definition never silently changes your output shape — you
  always say `@x` where you want the value.

### Capture-Less Definitions

A definition whose body produces no output is void:

- A single node root — named, anonymous, wildcard, with fields, predicates, or
  anchors — matches structurally and returns no data.
- A plain union of capture-less node branches also returns no data.
- A sequence root is void because no output syntax consumes it.
- A labeled alternation is unchanged: branch labels are explicit output
  syntax, and tag-only variants remain tag-only.
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

A definition is **void** when its body produces no output:

```
Id = (identifier) @id
Foo = (function_declaration name: (Id)) ; bare ref only → Foo is void
```

```typescript
export interface Id { id: Node; }
export type Foo = undefined;              ; matches or not — no data
```

There is no pure type aliasing: `Foo = (Id)` does not re-export `Id`'s type.

### Quantifier-Rooted Definitions

The definition name is a consuming position — exactly as it is for labeled
alternations. A quantifier standing as the whole body collects into the
definition's own output, which is the container itself:

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
captures, `::` annotations, and variant tags. So the element must already be
a nameable type: a plain node, or a reference. An anonymous element shape is
rejected; name it in its own definition:

```
Bad = {(key) @k (value) @v}*      ; ERROR: the element row has no type name
Row = (pair key: (_) @k value: (_) @v)
Good = (Row)*                     ; Good = Row[]
```

Two consequences:

- A definition whose root is `*` or `?` can match zero nodes, but a repeat
  iteration must consume input — so repeating a reference to it
  (`(MaybeId)*`) is rejected: the wrapper's empty case could never occur
  under the repeat, and the intent is clearer with the quantifier in one
  place.
- A quantifier-rooted definition used as an entrypoint is a **value
  entrypoint**: `run` outputs a top-level JSON array (or `null`), not an
  object. A `*`-rooted entrypoint that matches zero times prints `[]` and
  exits 0 — the zero-iteration match is a successful match.

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

- **Captured sequences** `{...} @x` → nested struct
- **Captured alternations** `[...] @x` → union struct or enum
- **Definitions** — references are opaque (see above)
- **Suppression** `@_` — discards the whole subtree's output

```
{
  (expression_statement) @s
} @info
→ { info: { s: Node } }           ; @info creates a nested scope
```

A captured sequence _without_ internal captures is only meaningful when it
matches exactly one node — the capture takes that node (`{(a)} @x` ≡
`(a) @x`). See the multi-node rule below.

## Strict Dimensionality

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

**2. A capture needs exactly one node or a value.** A void pattern under a
capture must match exactly one node — several, or possibly none, and there is
no single node to bind. Both directions fail for the same reason:

```
{(a) (b)} @x                      ; ERROR: two nodes — which one is x?
{(a)+} @x                        ; ERROR: a variable run of nodes
{(a)?} @x                        ; ERROR: possibly no node at all
[(a)+ (b)] @x                    ; ERROR: one branch is a run
(SeqDef) @x                       ; ERROR when SeqDef is void
```

Greediness never matters here: `?` and `??` (and `*`/`*?`, `+`/`+?`) are
identical to the type system — they differ only in which alternative the
runtime tries first.

The fix is always to say what you mean: capture individual nodes inside the
group, capture the quantifier directly (`(a)+ @xs` → a list), or capture
nothing.

References are opaque, so quantifying one is dimensionally simple — the
definition's _type_ is the element, no matter how many captures it contains:

```
Item = (pair key: (_) @k value: (_) @v)
(Item)* @items                    ; OK: items: Item[]
(Item)*                           ; OK: structural repeat, no output
```

### Scalar Lists vs Row Lists

```
(identifier)* @ids                ; scalar list: ids: Node[]
{(a) @a (b) @b}* @rows            ; row list:   rows: { a: Node, b: Node }[]
(Item)+ @items                    ; ref list:   items: [Item, ...Item[]]
```

### Optional Rows

A captured optional group is one nullable row — the `?` counterpart of a `*`
row list:

```
{(modifier) @mod (decorator) @dec}? @attrs
→ { attrs: { mod: Node, dec: Node } | null }
```

The fields keep their true modality — if the row matched, both are present —
and a skip yields `attrs: null`, never a hollow `{ mod: null, dec: null }`.
A quantified named node collects the same way: `(pair (key) @k)? @p` gives
`p: { k: Node } | null`, mirroring `(pair (key) @k)* @ps` rows.

There is no uncaptured fallback (dimensionality rule 1): a bare
`{(mod) @mod (dec) @dec}?` would scatter correlated nulls into the enclosing
scope as independently-optional fields — a type that permits states the match
can never produce. For a single optional node with no wrapper, put the capture
on the quantifier: `(decorator)? @dec` → `dec: Node | null`. To match
structurally and drop the captures, suppress: `{...}? @_`.

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
row struct for captured groups, the enum for labeled alternations.

## Alternations

`[...]` matches one of several branches. Its output form depends on labels
_and consumption_.

### Unions (no labels)

Branch captures merge into one struct, one level deep:

- A capture present in **all** branches → required field.
- A capture present in **some** branches → `T | null`.
- A missing **list** in a branch → `[]`, not null.
- The same capture must have the same type in every branch; a mismatch is an
  error (`capture @x has incompatible types across branches`). Cardinality
  counts: a `+` list and a `*` list do not unify.
- A branch that is a bare node beside struct branches is fine — it simply
  contributes no fields (or its own capture, if any).
- A branch that is a bare reference is a structural alternative: it
  contributes nothing to the merged struct.

```
[
  (binary_expression left: (_) @x right: (_) @y)
  (identifier) @x
]
→ { x: Node, y: Node | null }
```

A capture on the alternation itself takes the branch's value; for all-scalar
branches that's the matched node:

```
[(identifier) (number)] @value    → { value: Node }
```

### Enums (labels + consumption)

Branch labels prepare a tagged enum. The tags materialize when the
alternation's value is **consumed** — captured, row-captured, or used as a
definition body:

```
Expr = [                          ; def body root: consumed
  Lit: (number) @value
  Neg: (unary_expression (Expr) @inner)
]
```

```typescript
export type Expr =
  | { $tag: "Lit"; $data: { value: Node } }
  | { $tag: "Neg"; $data: { inner: Expr } };
```

Variant payloads come from the branch's bubbling captures:

- Captures → `$data` is an anonymous struct (always inlined, never a named
  standalone type).
- No captures → the variant is tag-only and omits `$data` entirely. Tags-only
  enums are legitimate — sometimes which branch matched _is_ the data.
- A bare reference (or `@_`) as branch body → tag-only variant.
  `[Call: (Inner)]` tags the branch; `[Call: (Inner) @data]` also carries the
  value.

```
[Stmt: (expression_statement)  Decl: (lexical_declaration)] @kind
→ kind: { $tag: "Stmt" } | { $tag: "Decl" }
```

### Degradation

A labeled alternation that nothing consumes cannot tag anything — there is no
value to put the tag on. It **degrades to a plain union** (captures bubble as
optional fields) and the compiler warns:

```
(program [A: (expression_statement) @e  B: (debugger_statement) @d])
```

```
warning: branch labels have no effect without capture
help: capture the alternation (`[...] @name`) to make the labels enum
      variants, or remove them
→ { e: Node | null, d: Node | null }
```

Mixing labeled and unlabeled branches in one `[...]` is an error.

## Suppression: `@_`

`@_` (or `@_name` — the name is documentation) consumes a pattern's output and
discards all of it. The subtree matches structurally; captures inside it are
inert, labels stay meaningful but produce nothing, and no degradation warning
fires — you said "discard all of it":

```
(program [A: (expression_statement) B: (debugger_statement)] @_)
→ matches, output is null; no warning
```

Suppressed captures never collide with real ones: a `@x` inside a suppressed
subtree does not touch a `@x` outside it. Quantifiers under `@_` carry no
dimensionality demands — nothing is collected, so nothing can be lost.

## Recursion

Definitions can reference themselves (or each other) when every cycle both
**escapes** and **consumes**:

1. **Escape**: some branch must terminate without recursing.
2. **Consumption**: each pass around the cycle must descend into a child.

```
Loop = (Loop)
; ERROR: infinite recursion: no escape path

A = [X: (identifier) @i  Y: (B) @b]
B = (A)
; ERROR: infinite recursion: cycle consumes no input

A = (parenthesized_expression (B))
B = (array (A))
; ERROR: no escape path — descending is not enough, some branch must terminate

MemberChain = [
  Base: (identifier) @name
  Access: (member_expression
    object: (MemberChain) @object
    property: (property_identifier) @property)
]
; OK: Base escapes, Access descends
```

```typescript
export type MemberChain =
  | { $tag: "Base"; $data: { name: Node } }
  | { $tag: "Access"; $data: { object: MemberChain; property: Node } };
```

A recursive reference in a union works the same way:

```
NestedCall = (call_expression
  function: [(identifier) @name (NestedCall) @inner]
  arguments: (arguments))
→ { name: Node | null, inner: NestedCall | null }
```

## Type Naming

Every structured type gets its name at compile time. Names are deterministic
and complete in the bytecode.

### Path Names

A definition's result is named after the definition. Composite types created
by captures are named `{ParentTypeName}{PascalCase(field)}`, following the
capture path; arrays and optionals are transparent (the name lands on the
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

Enum variant payloads are anonymous (inlined), so they take no name; a
composite _inside_ a payload field is named
`{EnumName}{VerbatimLabel}{PascalCase(field)}`:

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

### `::` Annotations

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

- The same annotation name on **structurally identical** types denotes one
  type — annotate two identical shapes `:: Info` and both fields share
  `Info`, declared once.
- The same name on **different** shapes is a compile error, reported with
  both spans (`type name X is already used for a different type`).
- Definition names and the builtin `Node` are reserved; `Node = ...` is an
  error.
- An annotation that matches the name the compiler would generate anyway is a
  warning (`omit it`), as is an annotation on a reference (a definition's type
  cannot be renamed at a use site).
- There is no numeric suffixing — a name conflict is always an error, never a
  silent rename.

## TypeScript Rendering

- Structs render as named `interface`s.
- Enums render as one multi-line union literal with inline variants — variant
  payloads never get standalone declarations.
- Void queries render as `export type Q = undefined;` — the query matches or
  not, and carries no data.
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
