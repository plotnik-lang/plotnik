# Plotnik reference

Plotnik matches Tree-sitter syntax trees and builds typed results from the parts you capture. A query describes both the structure you require and the data you want back.

- [A query from tree to result](#a-query-from-tree-to-result)
- [Match nodes and children](#match-nodes-and-children)
  - [Node kinds and wildcards](#node-kinds-and-wildcards)
  - [Children and sibling sequences](#children-and-sibling-sequences)
  - [Grammar fields](#grammar-fields)
  - [Syntax errors and missing tokens](#syntax-errors-and-missing-tokens)
- [Filter by text](#filter-by-text)
  - [Regular expressions](#regular-expressions)
- [Shape the result](#shape-the-result)
  - [Node values](#node-values)
  - [Flat captures and nested records](#flat-captures-and-nested-records)
  - [Discard a result](#discard-a-result)
- [Repeat or skip a pattern](#repeat-or-skip-a-pattern)
  - [Collect nodes or records](#collect-nodes-or-records)
  - [Greedy and lazy matching](#greedy-and-lazy-matching)
  - [Empty matches](#empty-matches)
- [Choose an alternative](#choose-an-alternative)
  - [Merge result fields](#merge-result-fields)
  - [Keep the case name](#keep-the-case-name)
  - [Alternatives that can match nothing](#alternatives-that-can-match-nothing)
- [Reuse patterns](#reuse-patterns)
  - [Capture a definition's result](#capture-a-definitions-result)
  - [Entry points and fragments](#entry-points-and-fragments)
  - [Definitions that return lists or options](#definitions-that-return-lists-or-options)
  - [Recursion](#recursion)
- [Constrain sibling positions](#constrain-sibling-positions)
  - [Soft and exact adjacency](#soft-and-exact-adjacency)
  - [First, last, and no children](#first-last-and-no-children)
  - [Anchors with optional or repeated patterns](#anchors-with-optional-or-repeated-patterns)
  - [Anchors in groups and definitions](#anchors-in-groups-and-definitions)
  - [Anchors beside alternatives](#anchors-beside-alternatives)
- [Choose capture types](#choose-capture-types)
  - [Source text](#source-text)
  - [Presence booleans](#presence-booleans)
  - [Type names](#type-names)
- [Files and spelling](#files-and-spelling)
  - [Names](#names)
  - [Suffix order](#suffix-order)
  - [Strings and comments](#strings-and-comments)
  - [Compile a file or directory](#compile-a-file-or-directory)
- [A complete example](#a-complete-example)

## A query from tree to result

Start with a JavaScript declaration:

```javascript
const answer = 42;
```

Tree-sitter produces this tree:

```text
program
|
`-- lexical_declaration
    |
    +-- kind: "const"
    |
    +-- variable_declarator
    |   |
    |   +-- name: identifier       "answer"
    |   |
    |   +-- "="
    |   |
    |   `-- value: number          "42"
    |
    `-- ";"
```

This query follows the tree to the variable name:

```ptk
Q = (program
  (lexical_declaration
    (variable_declarator
      name: (identifier) @name :: text
    )
  )
)
```

`Q =` names the query. Parentheses match a node and describe its children. `name:` requires the identifier to occupy the grammar's `name` field. `@name` creates a result field, and `:: text` asks for its source text:

```json
{ "name": "answer" }
```

Matching starts at the supplied tree's root. Here that node is `program`, so `Q = (identifier) @name` would not find `answer`. The path to it belongs in the query. Plotnik does not quietly insert a search through every descendant.

One execution returns the first complete match. If a later part fails, Plotnik backtracks and tries another way to satisfy the query. To collect several matches in one result, use repetition.

The examples below use the JavaScript grammar. A block without `Name =` is a pattern fragment to place at the appropriate level of the tree. Node kinds and grammar fields come from the selected source language's grammar.

## Match nodes and children

### Node kinds and wildcards

Tree-sitter distinguishes named nodes, such as `identifier`, from anonymous tokens, such as `"+"`. Plotnik keeps that distinction:

| Pattern        | Matches                     |
| -------------- | --------------------------- |
| `(identifier)` | A named identifier node     |
| `"+"` or `'+'` | The anonymous `+` token     |
| `(_)`          | Any named node              |
| `_`            | Any named or anonymous node |

A quoted token matches its grammar kind. To require a named identifier's text to be `answer`, use `(identifier == "answer")`.

`(_ ...)` can constrain the children of any named node. Bare `_` matches one node and has no child list.

Plotnik rejects abstract grammar supertypes, such as JavaScript's `expression`, including refinements such as `(expression#binary_expression)`. To choose between concrete kinds, use an [alternation](#choose-an-alternative).

### Children and sibling sequences

Nested patterns match direct children. They do not search grandchildren. Within a child list, patterns match siblings in order and can skip unrelated siblings:

```ptk
(binary_expression
  (identifier) @left
  (number) @right
)
```

For `answer + 1`, the identifier matches, the `+` token is skipped, and the number matches. Unmentioned children are unconstrained. `(binary_expression (identifier))` accepts either `answer + 1` or `1 + answer`.

Braces group a sequence of siblings without adding a level to the tree:

```ptk
{
  (comment) @doc
  (function_declaration) @function
}
```

```text
parent
|
+-- comment                 matches @doc
|
+-- unrelated sibling       can be skipped
|
`-- function_declaration    matches @function
```

The comment must precede the function. If a candidate function fails its child constraints, matching can continue to a later function. Failure farther along can also send the search back to an earlier choice, with that attempt's captures discarded.

Use [anchors](#constrain-sibling-positions) when a gap must be restricted. Without one, a gap can contain named nodes, punctuation, or comments.

### Grammar fields

Grammar fields identify a child's role. Result fields identify the data you return. Their names need not agree:

```ptk
(binary_expression
  left: (identifier) @target
  right: (number) @amount
)
```

Here `left` and `right` belong to the grammar, while `target` and `amount` belong to the result. Field constraints still follow sibling order. Writing `right:` before `left:` does not turn them into independent lookups.

Use `-field` to require that a node has no child in that field:

```ptk
(variable_declarator
  name: (identifier) @name
  -value
)
```

This matches `let answer`'s declarator, but not `let answer = 42`'s. The position of `-value` among the children does not move through the tree. It tests the enclosing node.

A field value describes one child node. A sequence is not a field value, even if its braces contain just one pattern.

Negated fields belong directly inside a node pattern. They cannot be captured, repeated, or used as an alternative by themselves.

### Syntax errors and missing tokens

Tree-sitter can return a tree even when the source is incomplete or malformed:

| Pattern                | Matches                        |
| ---------------------- | ------------------------------ |
| `(ERROR)`              | A parser error node            |
| `(MISSING)`            | Any token inserted by recovery |
| `(MISSING identifier)` | An inserted identifier token   |
| `(MISSING ";")`        | An inserted semicolon token    |

These patterns accept no child constraints. An `ERROR` node can cover source text. A `MISSING` node has a zero-width source range. The kind in `(MISSING kind)` must be a leaf token, so `(MISSING binary_expression)` is rejected.

## Filter by text

A predicate inside a leaf node pattern tests that node's source text:

| Pattern                     | Text requirement       |
| --------------------------- | ---------------------- |
| `(identifier == "answer")`  | Equals `answer`        |
| `(identifier != "ignored")` | Differs from `ignored` |
| `(identifier ^= "get")`     | Starts with `get`      |
| `(identifier $= "_id")`     | Ends with `_id`        |
| `(identifier *= "test")`    | Contains `test`        |
| `(identifier =~ /^test_/)`  | Matches the regex      |
| `(identifier !~ /^_/)`      | Does not match it      |

A failed predicate rejects that candidate node. It does not stop the search for a later candidate. Predicates add matching constraints without changing the result type.

There is one predicate position, immediately after the node kind. The kind must be a leaf in the grammar. A composite kind such as `expression_statement` cannot take a predicate, even if one particular node has simple text. Wildcards and definition references cannot take predicates either.

Plotnik does not support Tree-sitter's separate predicates or directives, such as `(#eq? @name "answer")` and `#set!`.

### Regular expressions

`/pattern/` searches anywhere in the node's text. Use `^` and `$` to constrain the beginning and end of that text:

```ptk
(identifier =~ /^[A-Z][a-z]+(?:[A-Z][a-z]+)*$/)
```

Regex matching supports Unicode characters and properties, character classes, groups, alternation, and repetition. Put flags inside the expression, as in `/(?i)answer/` or `/(?i:answer)/`. A slash inside the pattern is escaped as `\/`.

The details that affect matches are:

- `^` and `$` refer to the node's text boundaries. Multiline mode is unavailable.
- `.` excludes newline by default. `(?s)` lets it match newline too.
- `\b` and `\B` use ASCII word characters, even though the rest of the expression supports Unicode.
- Case folding and Unicode properties are resolved by Plotnik's compiler, so they do not depend on a host application's regex engine.
- Parenthesized groups affect regex matching only. They do not create Plotnik result fields.

Backreferences, lookahead, lookbehind, named groups, multiline mode `(?m)`, CRLF mode `(?R)`, and directional or half word boundaries such as `\<` and `\b{start}` are rejected. An empty regex literal `//` is also rejected. A nonempty expression that can match an empty string, such as `/a*/`, is valid.

## Shape the result

`@name` adds a result field. A query that only checks structure can succeed without returning data. Capturing a parent and capturing its children are separate choices.

### Node values

The default capture of a node is a `Node` value. Its JSON form contains the kind, source text, and half-open UTF-8 byte range:

```json
{
  "kind": "identifier",
  "text": "answer",
  "span": [6, 12]
}
```

In the introductory example, removing `:: text` changes `name` from `"answer"` to this object. A byte offset is not a character index. If point information is requested when generating node types, `startPoint` and `endPoint` use zero-based rows and byte columns.

### Flat captures and nested records

Nesting node patterns does not nest the result:

```ptk
(function_declaration
  name: (identifier) @name
  body: (statement_block
    (return_statement (_) @value)
  )
) @node
```

The result has three sibling fields:

```text
result
|
+-- name: Node
|
+-- value: Node
|
`-- node: Node
```

`@node` captures the function node itself. It does not gather `@name` and `@value` into an object. To make an object, capture a sequence containing those captures:

```ptk
{
  (function_declaration
    name: (identifier) @name
    body: (statement_block) @body
  ) @node
} @function
```

```text
result
|
`-- function
    |
    +-- name: Node
    |
    +-- body: Node
    |
    `-- node: Node
```

Braces alone only group matching. The capture on the braces creates the nested record.

A capture needs either a single node or a value assembled by the pattern. `{(identifier)} @x` can capture that one node. `{(identifier) (number)} @x` cannot choose a node to represent the pair. Capture inside the group to give it a record value, such as `{(identifier) @name (number) @value} @pair`. Adding `:: text` does not make an otherwise invalid capture valid.

Two captures cannot use the same name in the same record. They can reuse a name in separate nested records or in different alternatives, where the [merge rules](#merge-result-fields) apply.

### Discard a result

`@_` keeps a pattern's matching constraints and discards its entire result:

```ptk
Q = (program
  (expression_statement (identifier) @unused) @_
  (debugger_statement) @stop
)
```

Only `stop` appears in the result. Captures inside a discarded pattern do not collide with names outside it. Discards can nest.

`@_reason` does the same as `@_`. The suffix documents why the pattern is there. Discards cannot have a capture type.

## Repeat or skip a pattern

A quantifier follows the pattern it modifies. A capture after the quantifier collects the result:

| Pattern             | Matches      | Captured result |
| ------------------- | ------------ | --------------- |
| `(identifier) @x`   | Once         | One node        |
| `(identifier)? @x`  | Zero or once | Node or `null`  |
| `(identifier)* @xs` | Zero or more | List            |
| `(identifier)+ @xs` | One or more  | Nonempty list   |

The lazy forms `??`, `*?`, and `+?` have the same result types.

After a field value, a quantifier repeats or skips the whole field constraint. `value: (number)? @initial` optionally matches a number in `value`. Skipping it does not assert that `value` is absent or prevent it from holding another kind of node. Use `-value` to require absence.

### Collect nodes or records

Repeating a node pattern without inner captures collects the matched nodes:

```ptk
Q = (program (expression_statement)* @statements)
```

With captures inside the node pattern, it collects one record per match:

```ptk
Q = (program
  (lexical_declaration
    (variable_declarator
      name: (identifier) @name :: text
      value: (number) @value :: text
    )
  )* @bindings
)
```

For `const a = 1; const b = 2;`:

```json
{
  "bindings": [
    { "name": "a", "value": "1" },
    { "name": "b", "value": "2" }
  ]
}
```

The capture on `*` keeps each name with its value. Without `@bindings`, those inner captures have nowhere to put each iteration's record, so the query is rejected. Use a collecting capture or discard the repeated result with `@_`.

The same rule applies to `?`. In `(expression_statement (identifier) @id)? @statement`, the result is either one record `{ id: Node }` or `null`. It is not a record containing `id: null`. An uncaptured `?` with inner captures is rejected too.

For one iteration to include both a parent node and its child captures, put the captured node in a sequence and repeat that sequence:

```ptk
Q = (program
  {
    (expression_statement (identifier) @id) @node
  }* @statements
)
```

Each element has `id: Node` and `node: Node`. A captured repetition of a sequence without captures is invalid unless each iteration matches exactly one node.

Result fields are always present in their record. An absent optional value is `null`, and a zero-length list is `[]`. In TypeScript, an option is `T | null`, a list is `T[]`, and a nonempty list is `[T, ...T[]]`.

### Greedy and lazy matching

Greedy quantifiers try to match before continuing. Lazy quantifiers try the continuation as soon as their minimum is satisfied. Both can backtrack:

```ptk
Q = (program
  (expression_statement)* @prefix :: text
  (expression_statement) @last :: text
)
```

For `a; b; c;`, the greedy `*` first takes all three statements. The required final statement then fails, so the repeat gives one back:

```text
greedy *     a;  b; | c;
             prefix  last

lazy *?            | a;        b; c; need not match
             prefix  last
```

With `*`, the result is `{ "prefix": ["a;", "b;"], "last": "c;" }`. Replacing it with `*?` gives `{ "prefix": [], "last": "a;" }`. The name `last` is just a capture name. Add a trailing anchor to require the last child.

An unanchored repeat can skip nonmatching siblings between iterations. It does not mean "a contiguous run". [Anchoring the repeat](#anchors-with-optional-or-repeated-patterns) restricts those gaps.

### Empty matches

A pattern is _nullable_ if it can succeed without matching a node. `?` and `*` are nullable. A sequence is nullable when all its items are, and an alternation is nullable when any alternative is.

An empty match leaves the next sibling available for the following pattern. It is different from a `MISSING` token: the latter is an actual tree node with an empty source range.

Each successful repetition must match a tree node. Empty outcomes do not become list elements or satisfy `+`. For example, repeating a record-producing pattern with only optional contents collects records only for iterations that actually match a node. This also prevents a repeat from succeeding forever without moving.

References preserve their definitions' greedy or lazy preference. There is one separate restriction for collecting repeats of [nullable list- or option-valued definitions](#definitions-that-return-lists-or-options).

## Choose an alternative

Square brackets choose one pattern:

```ptk
[(identifier) (number)] @value
```

This captures one `Node`. The alternatives are not a list of results.

At a candidate sibling, alternatives are tried in source order. If none can complete the rest of the query, matching can try a later sibling. A later alternative at an earlier sibling wins over an earlier alternative found farther ahead. An alternative that initially matches can still be abandoned if a later requirement fails.

### Merge result fields

Without labels, captures from alternatives merge into one record:

```ptk
Q = (program
  (expression_statement
    [
      (identifier) @name :: text
      (number) @number :: text
    ]
  )
)
```

```text
input       result

answer;     { name: "answer", number: null }
42;         { name: null, number: "42" }
```

A field captured by every alternative keeps its type. A field omitted by an alternative gets a fallback there:

| Field being merged             | Fallback when omitted |
| ------------------------------ | --------------------- |
| Node, text, record, or variant | `null`                |
| Directly collected list        | `[]`                  |
| Presence boolean               | `false`               |

A list that can be omitted is no longer guaranteed nonempty. A definition reference retains its declared value type, so an omitted reference capture can be an option around that value, including a list value.

Captures with the same name must have compatible types. `[(identifier) @x (number) @x]` gives one required `x: Node`. Options can admit absence and list types can relax their minimum, but Plotnik does not merge different nested record shapes field by field. Distinct named record or variant types remain distinct even if their fields look identical. Use labeled alternatives when the possible values have different shapes.

The compiler checks these ordinary capture types before applying `:: text` or `:: bool`. Converting two incompatible values to the same primitive does not make their merge valid.

Capturing the whole alternation moves its merged fields into a nested record. With no inner captures, a capture can instead take the matched node, provided every alternative matches exactly one node.

### Keep the case name

Labels make the chosen alternative part of the result:

```ptk
Q = (program
  [
    Value: (expression_statement (identifier) @name :: text)
    Stop: (debugger_statement)
  ] @statement
)
```

For `answer;` and `debugger;`, respectively:

```json
{ "statement": { "$tag": "Value", "$data": { "name": "answer" } } }
```

```json
{ "statement": { "$tag": "Stop" } }
```

This is a variant: one named case with that case's data. A case without captures has a tag and no `$data`. Labels must be unique within the alternation, and an alternation cannot mix labeled and unlabeled cases.

The variant exists when the alternation supplies a value: it is captured, collected by a captured quantifier, or used as a definition's body. Merely placing labels in a node's child list does not preserve the case identity. Without a surrounding value boundary, the inner captures merge into the enclosing record and the compiler warns that the labels have no output effect. `@_` explicitly discards the result and silences that warning.

A bare definition reference as a case body contributes no payload. Capture it inside the case to include its result.

### Alternatives that can match nothing

An alternation tries node-consuming outcomes before empty ones, across all candidates. This applies even when a nullable alternative appears first:

```ptk
Q = (program
  [
    Comment: (comment)? @comment :: text
    Stop: (debugger_statement)
  ] @item
)
```

For `debugger;`, the result selects `Stop`. For an empty program, it selects `Comment` with `{ "comment": null }` as its data. The empty path needs no candidate node and leaves any following pattern's position unchanged.

An empty outcome still produces the value described by its chosen pattern. For example, a captured record containing optional fields remains a record with `null` fields. It does not automatically become `null` itself. Fields belonging only to other alternatives receive their fallbacks.

## Reuse patterns

`Name = pattern` defines a reusable pattern. `(Name)` uses it at the current position:

```ptk
Binding = (variable_declarator
  name: (identifier) @name :: text
  value: (number) @value :: text
)

Q = (program
  (lexical_declaration (Binding) @binding)
)
```

For `const answer = 42;`, `Q` returns:

```json
{ "binding": { "name": "answer", "value": "42" } }
```

Definitions can refer to definitions written later. A reference has no child list or predicate of its own. Add those constraints to the definition's body.

A reference can be a grammar-field value if it matches exactly one node. An alternation in a field must also match one node on every path. Capturing either keeps its result type. A reference to a nullable or multi-node fragment cannot be used as a field value.

### Capture a definition's result

A reference hides the definition's internal captures. The caller chooses whether to keep its result:

| Use                 | Result in the caller    |
| ------------------- | ----------------------- |
| `(Binding)`         | No result field         |
| `(Binding) @item`   | `item: Binding`         |
| `(Binding)* @items` | `items: Binding[]`      |
| `(Binding)? @item`  | `item: Binding \| null` |

That boundary prevents helper captures from leaking into every caller. Moving inline captures into a definition changes how the caller receives them: capture the reference where you want the helper's value.

A definition without result-producing syntax is match-only:

```ptk
Identifier = (identifier)
IdentifierValue = (identifier) @node
```

`(Identifier)` is a useful constraint, but `(Identifier) @x` is an error because there is no value to capture. `(IdentifierValue) @x` returns `x: { node: Node }`. A bare alias such as `Alias = (IdentifierValue)` also discards the referenced result. It does not create a type alias.

A successful match-only query renders as `null` in debug JSON and as `undefined` in generated TypeScript. This is still a successful match. Failure to match is a separate outcome.

### Entry points and fragments

An entry point is a definition that can start execution on the tree's root. Its body must match exactly one top-level node and need no surrounding anchor context. Captures do not affect that eligibility. An alternation or reference qualifies when its possible matches meet the same condition. A sequence containing just one such pattern can qualify too.

A definition matching several siblings, an optional node, or a repetition is a fragment. Use it inside another pattern:

```ptk
Documented = {
  (comment) @doc :: text
  (function_declaration) @function
}

Q = (program (Documented)* @functions)
```

`Documented` cannot run at the root by itself. `Q` can, because it matches one `program` node. Entry-point selection belongs to the caller. Definition order does not imply an automatic traversal or root wrapper.

### Definitions that return lists or options

When a quantifier is the whole definition body, the definition itself collects its value:

```ptk
Identifiers = (identifier)*
MaybeIdentifier = (identifier)?

Item = (expression_statement (identifier) @name)
Items = (Item)+
```

`Identifiers` is a list of nodes, `MaybeIdentifier` is a node or `null`, and `Items` is a nonempty list of `Item` records. All three are fragments. A captured use such as `(Items) @items` keeps that list as one value.

If the element has a record or variant shape, give it its own definition, as with `Item`. Writing `(expression_statement (identifier) @name)*` as an entire definition body leaves the element type unnamed and is rejected. A capture on the quantifier is another way to name the result's element through a result field.

A collecting `*` or `+` cannot repeat a reference to a nullable definition whose result is itself an option or list. For example, `(MaybeIdentifier)* @items` and `(Identifiers)* @groups` are rejected. A structural repeat or an explicit discard is allowed, and a nonempty list-valued definition such as `Items` can be repeated to collect lists of lists.

This restriction concerns option- and list-valued definitions. A nullable definition returning a record can be collected, with only node-consuming outcomes becoming elements. Nested options do not create distinguishable kinds of `null`.

### Recursion

Definitions can call themselves or each other. Every recursive cycle needs a way to finish, and each trip around the cycle must match a tree node before returning to the same definition:

```ptk
MemberChain = [
  Base: (identifier) @name :: text
  Access: (member_expression
    object: (MemberChain) @object
    property: (property_identifier) @property :: text
  )
]

Q = (program
  (expression_statement (MemberChain) @chain)
)
```

For `a.b.c`, recursion follows the nested `object` fields:

```text
Access                         property: "c"
|
`-- object: Access             property: "b"
    |
    `-- object: Base           name: "a"
```

`Base` lets the recursion finish. `Access` matches a `member_expression` before descending to another `MemberChain`.

`Loop = (Loop)` has no way to finish. `Loop = [(Loop) (identifier)]` has a base case but recurses without making progress. Both are rejected. The same checks apply to cycles across several definitions.

The progress check needs a node match before the recursive call, as shown by `member_expression` above. A preceding call to a separate helper does not establish progress for this check, even if that helper always matches a node. Keep the node pattern that guarantees progress in the recursive body.

## Constrain sibling positions

Anchors restrict the gaps that ordinary matching can skip. They constrain syntax-tree nodes, not source characters. Whitespace that Tree-sitter leaves out of the tree cannot break an anchor.

Tree-sitter marks some nodes as _extras_, commonly comments. Extras can be named or anonymous. An anchor's treatment of extras is separate from whether a pattern can explicitly match them.

### Soft and exact adjacency

`.` is soft adjacency. Between named-node patterns it skips punctuation and extras, but no other named node. With an anonymous token or bare `_` on either side, it skips extras only. `.!` allows no intervening tree node:

| Gap                | Nodes allowed in the gap   |
| ------------------ | -------------------------- |
| No anchor          | Any                        |
| `(a) . (b)`        | Anonymous nodes and extras |
| `(a) . "token"`    | Extras only                |
| `"token" . (b)`    | Extras only                |
| `(a) . _`          | Extras only                |
| Any pair with `.!` | None                       |

Here `a` and `b` stand for named node kinds. `(_)` uses the named-node rule, while `_` must allow for an anonymous match.

For example:

```ptk
(array (identifier) @first . (identifier) @second)
```

```text
[a, /* note */ b]       matches: comma and comment can be skipped
[a, 42, b]              fails: number is a named node
```

Replacing `.` with `.!` makes this query impossible: JavaScript arrays separate identifiers with comma nodes. The compiler rejects that adjacency using the grammar. Exact adjacency is quite literal about punctuation.

An explicit `(comment)` pattern still gets to match a comment. Skipping rules apply to the gap, not to the node requested by the pattern.

### First, last, and no children

A leading anchor restricts the gap before the first matched child. A trailing anchor restricts the gap after the last one:

```ptk
(array . (identifier) @first)
```

```ptk
(array (identifier) @last .)
```

The first pattern requires the first named non-extra child to be an identifier. The second requires the last such child to be an identifier. Anonymous brackets and commas can be skipped. For an anonymous operand, only extras can be skipped at that boundary. With `.!`, the matched node must be the actual first or last child, including punctuation.

Anchors alone check emptiness:

```ptk
(statement_block .)
```

This accepts a block containing only braces and comments. `(program .!)` requires a program with no children at all.

### Anchors with optional or repeated patterns

An empty match does not erase an anchor:

```ptk
Q = (program
  (lexical_declaration)? @declaration
  .
  (debugger_statement) @stop
)
```

If the declaration matches, `stop` must follow it under the soft adjacency rule. If the declaration is skipped, the anchor applies at the beginning of the child list. `debugger;` and `/* note */ debugger;` match that path, but `call(); debugger;` does not.

An anchor before a quantified pattern constrains both where the repeat begins and the gaps between its iterations:

```ptk
Q = (program
  . (debugger_statement)* @stops
  . (expression_statement) @next
)
```

For `debugger; debugger; work();`, `stops` contains both debuggers. It cannot skip `other();` to find another debugger farther ahead. If the repeat takes zero nodes, the next anchored pattern must still begin at the parent's allowed boundary. `.!` keeps the exact rule on both the consuming and empty paths.

If an entire anchored child list matches nothing, its anchors check that the parent contains no disallowed children. `(program (debugger_statement)* @stops .)` can match an empty or comment-only program with `stops: []`, but not a program containing an expression statement. When both leading and trailing constraints remain, the stricter one applies.

### Anchors in groups and definitions

An interior anchor can join patterns inside a sequence. A boundary anchor needs a sibling or an enclosing node supplied by its surroundings. Braces do not create a parent node.

A definition can leave that boundary for its caller:

```ptk
Tail = {(identifier) @name .}

Q = (program
  (expression_statement
    (array (Tail) @tail)
  )
)
```

`Tail` is a fragment. Its trailing anchor makes the captured identifier the array's last named non-extra child. A chain of references that never supplies the required context is rejected.

Anchors cannot be captured or quantified. They also cannot stand directly between alternatives. To anchor a sequence within an alternative, put that sequence in braces.

### Anchors beside alternatives

With a soft anchor before an alternation, the alternative being tried determines whether anonymous nodes can be skipped:

```ptk
(array (identifier) . [(identifier) ","])
```

The identifier alternative can skip punctuation. The comma alternative can skip only extras. After an alternation, the path that matched determines the rule too, including before a trailing anchor. Captures, quantified alternatives, and definition references preserve the named and anonymous paths. Bare `_` still uses the extras-only rule even when it happens to match a named node.

One conservative case remains in the grammar check: a sequence containing an anonymous token is classified as potentially anonymous as a whole. Inside an array, `{"[" (identifier)} . (identifier)` is rejected because the gap needs a comma. Write `"[" {(identifier) . (identifier)}` to put the soft anchor directly between the named patterns.

## Choose capture types

`::` changes how an already valid capture is represented. `text` and `bool` are the only lowercase built-ins. A PascalCase name gives the inferred type a name, so `Text` is a custom name rather than another spelling of `text`.

### Source text

`@name :: text` returns source text instead of the captured node or structured value. Options and lists keep their shape:

| Ordinary value     | With `:: text`       |
| ------------------ | -------------------- |
| Node               | String               |
| Optional node      | String or `null`     |
| List of nodes      | List of strings      |
| Nonempty node list | Nonempty string list |
| Record or variant  | Its matched text     |

Each list item owns its own source range. Converting a list does not join the items or include text between them.

A record or variant uses the range from its first contributing matched node through its last, including intervening text. For example:

```ptk
Q = (program
  {
    (comment) @comment
    (expression_statement (identifier) @name)
  } @chunk :: text
)
```

For `// note` followed by a newline and `answer;`, `chunk` is `"// note\nanswer;"`. The conversion replaces the group's record. The compiler warns because its internal captures no longer appear as data.

By contrast, `(expression_statement (identifier) @id) @source :: text` still returns both `id: Node` and `source: string`. That capture owns the parent node, while `id` is a separate field in the enclosing record.

An absent option becomes `null`. A structured match with no contributing node also becomes `null`. A real zero-width node becomes the present empty string `""`.

### Presence booleans

`bool` reports whether a value is present:

```ptk
Q = (program
  (debugger_statement)? @has_debugger :: bool
)
```

The result contains `has_debugger: true` when the optional pattern matches and `false` when it is skipped. It never tests the captured text, numeric value, or byte length.

The value must be able to be absent. A required node, record, variant, or list would always produce `true`, so `bool` is rejected there unless another alternative omits that exact result field. In that case it reports whether the chosen alternative supplies the field.

`bool` does not map list elements or test whether a list is nonempty. A list present in a chosen alternative can yield `true` even when the list is empty. Nested option layers collapse to one boolean. Converting a composite value suppresses that value's data and produces a warning.

### Type names

Plotnik names the types inferred from captures. A definition supplies the root name, and nested records or variants extend it with the capture name in PascalCase:

```text
Q
|
`-- functions[]             QFunctions
    |
    `-- details             QFunctionsDetails
```

Lists and options do not add a name component. Inside a variant case, the case label is kept verbatim: a `details` record in the `Call` case of `QStatement` is `QStatementCallDetails`. The case's immediate payload stays inline.

Use `:: Name` to choose a name without changing the value:

```ptk
Q = (program
  (lexical_declaration
    (variable_declarator name: (identifier) @name)
  )* @bindings :: Binding
)
```

The list elements are `Binding` records instead of `QBindings`. Any nested type names then start with `Binding`. On a plain node capture, `:: Name` creates a named alias for `Node`.

The same custom name can be shared by identical shapes. Reusing it for different shapes is an error. `Node` and definition names are reserved. Different named definitions remain distinct types, and a use-site annotation cannot rename a definition's result. Redundant names produce warnings rather than new types.

## Files and spelling

A `.ptk` file contains named definitions. Each definition has one pattern body. There are no statement separators: a newline has the same syntactic role as other whitespace. Bare top-level patterns are rejected, including when query text is passed inline.

### Names

| Name                          | Form         | Example          |
| ----------------------------- | ------------ | ---------------- |
| Definition, case, custom type | PascalCase   | `MemberChain`    |
| Capture                       | snake_case   | `@function_name` |
| Node kind, grammar field      | Grammar name | `identifier`     |

Names are case-sensitive. Definitions and custom types start with an ASCII uppercase letter and use letters or digits. Capture names start with an ASCII lowercase letter and use lowercase letters, digits, or underscores. A leading underscore after `@` makes a discard. Capture names cannot use Tree-sitter's dotted form, such as `@function.name`.

An uppercase name in parentheses is a definition reference. Node and field names follow the grammar's spellings, normally snake_case. Definitions must be unique across all files compiled together.

### Suffix order

The order is pattern, quantifier, capture, capture type:

```ptk
(identifier)+ @names :: text
```

There is one quantifier, one capture, and one capture-type position at each level. Use braces to group a larger pattern before applying another suffix. `*?`, `+?`, `??`, `.!`, and `::` are single tokens, so their characters cannot be separated by whitespace.

Empty `()`, `{}`, and `[]` are errors. Use an optional pattern for a match that can take nothing, or an anchor-only child list to test emptiness.

Write sibling groups as `{...}`. The parser still recognizes Tree-sitter's `((...)(...))` grouping and `!field` negation with warnings directing you to braces and `-field`. Supertype refinements are unsupported in either `#` or `/` spelling.

### Strings and comments

Single- and double-quoted strings have the same meaning. They are used for anonymous tokens and string predicate values. Supported escapes are:

| Escape    | Character                  |
| --------- | -------------------------- |
| `\n`      | Newline                    |
| `\r`      | Carriage return            |
| `\t`      | Tab                        |
| `\\`      | Backslash                  |
| `\"`      | Double quote               |
| `\'`      | Single quote               |
| `\u{...}` | Unicode scalar, 1-6 digits |

Unicode escape digits are hexadecimal, as in `\u{1F600}`. Other string escapes are errors. Regex literals use regex escaping instead.

`//` and `;` begin line comments. `/* ... */` encloses a block comment. A `#!` line is accepted only at the very beginning of the file. The compiler ignores it rather than using it to select a grammar or entry point.

### Compile a file or directory

The compiler needs the source language's `grammar.json` to check node kinds, field names, and possible tree shapes:

```sh
plotnik check query.ptk --grammar path/to/grammar.json
plotnik infer query.ptk --grammar path/to/grammar.json
```

`check` validates the query. `infer` shows its result types. A directory in place of `query.ptk` loads its `.ptk` files in filename order into one namespace. No imports are needed between them, and subdirectories are not loaded recursively.

Passing `-q 'Q = ...'` supplies query text instead of a file. It uses the same language rules and does not add a root pattern. The CLI's execution commands are not implemented, so these compiler commands do not run the query on a source file.

## A complete example

This query collects simple JavaScript variable bindings and identifier calls, preserving which kind of statement matched:

```ptk
Expression = [
  Name: (identifier) @text :: text
  Number: (number) @text :: text
]

Statement = [
  Binding: (lexical_declaration
    (variable_declarator
      name: (identifier) @name :: text
      value: (Expression) @value
    )
  )
  Call: (expression_statement
    (call_expression
      function: (identifier) @function :: text
      arguments: (arguments (Expression)* @arguments)
    )
  )
]

Q = (program (Statement)* @statements)
```

For:

```javascript
const answer = 42;
print(answer);
```

The result is:

```json
{
  "statements": [
    {
      "$tag": "Binding",
      "$data": {
        "name": "answer",
        "value": { "$tag": "Number", "$data": { "text": "42" } }
      }
    },
    {
      "$tag": "Call",
      "$data": {
        "function": "print",
        "arguments": [{ "$tag": "Name", "$data": { "text": "answer" } }]
      }
    }
  ]
}
```

The outer `*` can skip statements that do not match `Statement`, and the argument repeat can skip arguments that do not match `Expression`. These are extraction patterns, so unmentioned syntax remains unconstrained. To require complete coverage of a child list, use the boundary and adjacency anchors described above.
