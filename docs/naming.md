# Name the concept, not its current representation

Plotnik uses one stable vocabulary across the query language, compiler, runtime,
generated code, diagnostics, and serialized output. Consistency does not mean
using one word at every layer: the same feature may be an alternation in query
syntax, a variant in the result model, an enum in generated Rust, and a
discriminated union in generated TypeScript.

The governing rule is:

> Name query syntax by matcher behavior, results by their target-neutral shape,
> and target-language forms only at target boundaries.

Before naming something, identify the layer and the distinction the name must
preserve. If two nearby concepts could share a familiar word, qualify both
instead of leaving one ambiguous: `grammar_field` and `result_field`, or
`query_span` and `event_range`. Once a module or enclosing object establishes
the domain, prefer the shorter name.

## Vocabulary follows the layer

| Layer                | Names describe                          | Examples                                               |
| -------------------- | --------------------------------------- | ------------------------------------------------------ |
| Query syntax         | what the author writes                  | alternation, alternative, capture                      |
| Matching             | how a pattern explores a syntax tree    | candidate node, checkpoint, continuation               |
| Result model         | target-neutral value shape              | option, record, list, variant                          |
| Rust output          | the generated Rust representation       | `Option`, struct, `Vec`, enum                          |
| TypeScript output    | the generated TypeScript representation | nullable type, object type, array, discriminated union |
| JSON output          | the serialized representation           | object, array, `null`, tagged object                   |
| Compiler and runtime | how matching is implemented             | NFA transition, instruction, output event              |

Related words still have separate jobs:

- An **alternative** is an item inside `[...]`.
- A **successor arm** is one outgoing control-flow choice at a fork.
- An **execution path** is the dynamic sequence taken through one match attempt.
- A **case** is one possibility of a target-neutral variant; generated Rust
  renders it as an enum **variant**.

## Query-language terms

Use these names in language documentation, diagnostics, syntax metadata, and
parser code.

| Syntax or role                               | Canonical term                      |
| -------------------------------------------- | ----------------------------------- |
| `Name = pattern`                             | definition                          |
| right-hand side of a definition              | definition body                     |
| `(Name)`                                     | definition reference                |
| definition eligible for execution            | selectable definition               |
| selectable definition exposed by an artifact | exported entry point                |
| entry point chosen for one run               | selected entry point                |
| non-selectable reusable definition           | fragment definition                 |
| `(kind)`                                     | named-node pattern                  |
| `"text"` or `'text'`                         | anonymous-node pattern              |
| `(_)` / `_`                                  | named-node wildcard / node wildcard |
| `{...}`                                      | sequence                            |
| `[...]`                                      | alternation                         |
| item inside `[...]`                          | alternative                         |
| `A:` inside `[...]`                          | alternative label                   |
| `?`, `*`, `+`                                | quantifiers                         |
| `??`, `*?`, `+?`                             | lazy quantifiers                    |
| `.` / `.!`                                   | soft anchor / exact anchor          |
| `@name`                                      | capture                             |
| `@_name` / `@_`                              | named discard / discard             |
| `:: T`                                       | capture type                        |
| `field: pattern`                             | grammar-field constraint            |
| `-field`                                     | negated grammar-field constraint    |

Write **entry point** as two words in prose and `EntryPoint` or `entry_point` in
code. Entry point is an export or execution role, not a synonym for definition.
A definition is selectable only when its successful match has exactly one
top-level syntax-tree node; `RootExtent::{SingleNode, Other}` records this
distinction without pretending every other pattern has one common arity.

Use **query** for one complete Plotnik input and **query text** for the contents
of one input buffer. Use **module** only with a qualifier such as **bytecode
module** or **generated module**. Do not call definitions rules: Tree-sitter
already has grammar rules, and a Plotnik definition may be both referenced and
executed.

Use **greedy** and **lazy**, not greedy and non-greedy, as the primary
quantifier pair. Both preferences may backtrack when the continuation fails.
Use **capture type**, not annotation or modifier. A capture binds a pattern's
result, which may be a node, text, record, list, option, or variant; it does not
merely name a matched node. The built-ins `:: text` and `:: bool` change the
captured representation; a PascalCase capture type supplies a result type name
without changing the underlying matched shape.

### Alternations produce different result shapes

`[...]` is always an **alternation**. Labels affect its result, not its syntactic
category.

- An **unlabeled alternation** merges fields from its alternatives into the
  enclosing record. If a capture materializes those fields, the result is a
  merged record—not a union type.
- A **labeled alternation** produces a target-neutral **variant** when the
  surrounding context materializes its value. Its labels name **cases**; a case
  may carry a record payload or no payload.
- If no surrounding construct materializes the labeled alternation, its labels
  have no output effect and its captures merge as for an unlabeled alternation.

Do not call Plotnik alternation **ordered choice**. That term commonly implies
commitment to the first locally successful choice, while Plotnik can backtrack
to a later alternative after continuation failure. Describe the behavior as
**source-order preference with backtracking** when the matching policy matters.

## Result terms

Use this target-neutral vocabulary above code-generation and serialization
boundaries:

| Concept           | Meaning                                            |
| ----------------- | -------------------------------------------------- |
| node              | a matched Tree-sitter node                         |
| text              | borrowed source text                               |
| boolean           | `true` or `false`                                  |
| option            | zero or one semantic value                         |
| record            | named result fields, each with one value           |
| list              | an ordered sequence of one element type            |
| variant           | one selected named case, with or without a payload |
| type declaration  | a name attached to a structural type body          |
| type reference    | a reference to a type declaration                  |
| match-only output | successful matching without result data            |

`TypeShape` therefore uses `Node`, `Text`, `Bool`, `Option`, `Record`, `List`,
`Variant`, and `Ref`. Rust structs, enums, and vectors and TypeScript object
types, discriminated unions, and arrays belong only in their renderers and
target-specific documentation.

The current JSON renderer uses an **adjacently tagged representation**: `$tag`
stores the case name, `$data` stores its record payload, and a no-payload case
has no `$data`. These are encoding keys, not names for the semantic construct.

A result field always exists in the record schema. Its value may have option
type and render as `null`; that does not make it an optional TypeScript property
or an omitted JSON key. Plotnik has one observable absence state, so nested
options normalize to one option layer. Likewise, say **list of records**, not
row, row struct, or row wrapper.

A **grammar field** relates a parent syntax node to a child. A **result field**
belongs to a target-neutral record; generated Rust renders it as a struct field
and TypeScript as an object property. A variant contains cases, while the Rust
enum that represents it contains variants.

Keep these states distinct:

| Term                    | Meaning                                             |
| ----------------------- | --------------------------------------------------- |
| nullable pattern        | can match without consuming a syntax-tree node      |
| empty match             | runtime success that consumes no syntax-tree node   |
| missing node            | Tree-sitter recovery node with an empty source span |
| option value            | semantic value that may be absent                   |
| nullable representation | target encoding that uses `null`                    |
| empty list              | present list with no elements                       |
| no-value flow           | internal success without result data                |
| match-only output       | public entry-point result with no data              |
| no-payload case         | variant case with no payload                        |

An epsilon transition is also not an empty match: it is one implementation edge
that consumes no node, while the execution path containing it may still consume
nodes.

## Tree-sitter terms

Use Tree-sitter's vocabulary literally for source documents:

- **named node** and **anonymous node** describe the node's named flag;
- an **extra node** is admitted by the grammar's extras behavior;
- a **missing node** is an inserted recovery node;
- an **`ERROR` node** contains source text the parser could not incorporate;
- **node kind** and **grammar field** come from the grammar;
- **syntax tree** or **concrete syntax tree** names the parser output.

Reserve **AST** for Plotnik's own abstract query representation. The parsed
source document remains a syntax tree even when a display hides anonymous
nodes. Avoid **trivia** in public prose: anonymous nodes include meaningful
punctuation and operators. Low-level navigation code may define a local class
such as `AnonymousOrExtra`, but must not attribute that grouping to Tree-sitter.

The language being matched is the **source language**. A language emitted by a
code generator is the **target language**. A Tree-sitter `Language` object, a
grammar artifact, a parser, and Plotnik's internal language profile are separate
objects; use the expanded name where they meet.

## Compiler and runtime terms

Name pipeline stages by the artifact they produce:

1. **Parse** query text into syntax and an AST.
2. **Analyze** names, recursion, result shapes, and other grammar-independent
   semantics.
3. **Bind** node kinds and grammar fields to one Tree-sitter grammar.
4. **Lower** the bound query into an optimized matcher NFA.
5. **Emit** bytecode, generated matchers, or declarations.

Keep each representation's control-flow vocabulary within its boundary:

| Representation    | Canonical terms                                                |
| ----------------- | -------------------------------------------------------------- |
| matcher NFA       | state, transition, epsilon transition                          |
| bytecode          | instruction, opcode, operand slot, code address, bytecode word |
| generated matcher | matcher state, dispatch                                        |
| resource limit    | fuel, fuel limit, fuel used                                    |
| debugger          | step, execution trace, trace record                            |

A matcher **dispatch** is one executor-loop iteration. One dispatch currently
consumes one unit of public **fuel**, but fuel is a safety budget, not a stable
performance metric. Reserve **step** for debugger actions such as
single-stepping.

Result construction and debugging also have separate artifacts:

- The **match journal** is rollbackable during matching and committed on
  acceptance.
- An **output event** constructs result data; an **inspection event** records
  provenance. `JournalEvent` is their shared physical representation.
- `OutputEvents` is the result-construction view of the committed journal.
- `ResultDecoder` builds typed output from those events; it does not replay an
  execution.
- An **execution trace** records matcher execution for debugging.
- A **result provenance map** connects query spans, document spans, result
  bindings, and journal-event ranges.

Event names should expose the subject when the verb alone is ambiguous:
`ArrayPush` and `RecordSet`, not bare `Push` and `Set`. Shape names remain
target-neutral where the event opens or closes a semantic value, such as
`ListOpen`, `RecordOpen`, and `VariantOpen`.

## Coordinates name their domain

Every interval is half-open, `[start, end)`, but half-openness alone does not
determine its name. Use the surrounding domain and operation:

| Coordinate                   | Use                                                              |
| ---------------------------- | ---------------------------------------------------------------- |
| query span                   | location in Plotnik query text                                   |
| document byte range          | UTF-8 byte interval in matched source code                       |
| document bounding range      | smallest interval enclosing possibly discontiguous contributions |
| generated-artifact range     | interval in named generated output, such as TypeScript           |
| bytecode-word address        | location in the instruction stream                               |
| journal-event range          | interval in the match journal                                    |
| execution-trace record index | one debugger record                                              |
| JSON Pointer                 | location in serialized result data                               |

Short contextual names remain idiomatic. A serialized node is
`{ kind, text, span: [start, end] }`, while a Rust provenance struct containing
several coordinate domains uses `event_range`. Avoid bare `source`, `span`, or
`range` where the containing type does not establish the domain.

## Public names should be familiar and exact

Precision does not require mechanically descriptive APIs. Generated queries
use `Parse` / `parse` for typed query application and `Matches` / `matches` for
match-only queries. Tree-sitter's `Parser::parse` separately turns source text
into a syntax tree; the receiver and argument types make the boundary clear.

The tree display command is `tree`. When both trees appear, call them the
**Query AST** or **Query CST** and the **Source syntax tree**. Inspection syntax
uses `SpanKind::Alternation(Labeling)` and `SpanKind::Alternative`; serialized
alternations use `kind: "alternation"` plus `labeling: "unlabeled"` or
`"labeled"`.

Diagnostics should teach the model through the repair the user can make. Prefer
“either label every alternative or remove all labels” to a taxonomy lesson, and
“this capture already has type `Bla`; naming it `Foo` won't have an effect” to
an explanation of compiler history.

## Keep names precise without making them heavy

Shared boundary types should be searchable. Inside a focused module, let the
module supply context: `compile`, `style`, and `alternatives` are often clearer
inside `alternation.rs` than repeating `alternation` in every identifier.
Qualify a name when adjacent domains would otherwise collide, not merely to make
it longer.

Names should describe roles and invariants rather than containers, chronology,
or implementation accidents. Prefer `GrammarBinding` to `Context`,
`ResultDecodePlan` to `ReplayPlan`, and `TraceNode` to an overloaded `NodeRef`.
Use `OutputContext::{Fields, Value, Discard}` for result handling and reserve
consumption for matching progress.

A rename is complete only when the concept has the same name everywhere a
contributor will search for it: types, variants, functions, modules, filenames,
fixtures, diagnostics, CLI flags, protocol keys, generated APIs, and current
documentation. Review each occurrence in context before changing it; a shared
spelling does not prove shared meaning.
