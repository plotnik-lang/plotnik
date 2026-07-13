# Plotnik terminology contract

Status: working contract for the terminology redesign. This document records the decisions that implementation should follow; it is not yet user-facing documentation.

## Goal

Plotnik needs one stable vocabulary across the language reference, type system, CLI, diagnostics, public APIs, compiler, generated code, runtime, and inspection formats. A familiar word is not precise if it means different things in those layers.

The governing rule is:

> Name query syntax by what the matcher does, name results by their abstract shape, and use target-language terms only at target boundaries.

This rule deliberately prevents a Rust representation such as `enum`, a TypeScript representation such as discriminated union, or a VM mechanism such as an effect log from naming the source-language construct that produced it.

## 1. Layer boundaries

The same construct may need a different term at each layer. That is not inconsistency when each term names a different concept.

| Layer             | What its names describe                 | Example                                   |
| ----------------- | --------------------------------------- | ----------------------------------------- |
| Query syntax      | What the author writes                  | labeled alternation                       |
| Matching          | How a pattern explores the tree         | source-order preference with backtracking |
| Result model      | The target-neutral value shape          | variant type                              |
| Rust output       | The generated Rust representation       | `enum`                                    |
| TypeScript output | The generated TypeScript representation | discriminated union                       |
| JSON output       | The serialized representation           | adjacently tagged representation          |
| Lowering/runtime  | How the matcher implements the behavior | successor, checkpoint, output event       |

Related words at different layers should have separate jobs:

- An **alternative** is a source-level item inside `[...]`.
- A **successor arm** is one outgoing control-flow choice at a fork.
- An **execution path** is the dynamic sequence of states or instructions taken through one match attempt.
- A **case** is one possibility of a result variant; a Rust renderer emits it as an enum **variant**.

## 2. Query-language vocabulary

### 2.1 Queries, definitions, and execution

| Canonical term        | Meaning                                                                 |
| --------------------- | ----------------------------------------------------------------------- |
| query language        | Plotnik itself                                                          |
| query text            | The contents of one Plotnik input buffer                                |
| query file            | One `.ptk` file                                                         |
| query                 | One complete Plotnik input compiled or executed together                |
| query source table    | The indexed query texts addressed by query-source IDs                   |
| definition namespace  | The definition names visible within a query                             |
| pattern               | An expression that describes matching and captures                      |
| definition            | `Name = pattern`                                                        |
| definition body       | The pattern on the right-hand side of a definition                      |
| definition reference  | `(Name)` in query syntax                                                |
| selectable definition | A single-root-node definition eligible to be selected as an entry point |
| fragment definition   | A definition that may be referenced but not selected as an entry point  |
| exported entry point  | A selectable definition exposed by a compiled artifact                  |
| selected entry point  | The exported entry point chosen for one execution                       |
| source language       | The language of the code being matched, such as Rust or JavaScript      |
| target language       | The language emitted by code generation, such as Rust or TypeScript     |
| emission target       | A selected artifact form, including bytecode or declarations            |
| bytecode module       | The validated VM artifact                                               |
| generated module      | Source code emitted for one target language                             |

Use **entry point** as two words in prose and `EntryPoint` / `entry_point` in Rust. Qualify exported versus selected when the distinction matters. Entry point is a runtime/export role, not a synonym for every definition. A definition is selectable when analysis proves its root extent is exactly one node; other definitions remain reusable fragments. A definition reference invokes either kind, so neither “executable” nor “callable” distinguishes entry-point eligibility.

In ordinary public prose, use **query** and **language** unless a contrast requires a qualifier. Internal compiler boundaries may use `QueryUnit` for one complete compilation input and `LanguageProfile` for the selected language plus its runtime and grammar metadata. Those internal type names do not make their expanded descriptions mandatory user vocabulary.

Use **module** only with a qualifier. A query, bytecode module, and generated module are different artifacts.

Do not use **rule** for a definition. Tree-sitter already has grammar rules, and other code-query tools use rule for a complete finding specification. Do not use **fragment** for every definition: a Plotnik definition may be both reused and executed. Fragment is reserved for a definition that cannot be selected as an entry point.

### 2.2 Patterns

| Syntax                 | Canonical term                   |
| ---------------------- | -------------------------------- |
| `(kind)`               | named-node pattern               |
| `"text"` / `'text'`    | anonymous-node pattern           |
| `(_)`                  | named-node wildcard              |
| `_`                    | node wildcard                    |
| `{...}`                | sequence                         |
| `[...]`                | alternation                      |
| item inside `[...]`    | alternative                      |
| `A:` inside `[...]`    | alternative label                |
| `?`, `*`, `+`          | quantifiers                      |
| `??`, `*?`, `+?`       | lazy quantifiers                 |
| `.`                    | soft anchor                      |
| `.!`                   | exact anchor                     |
| `@name`                | capture                          |
| `@_name`               | named discard                    |
| `@_`                   | discard                          |
| `:: T` after a capture | capture type                     |
| `field: pattern`       | grammar-field constraint         |
| `-field`               | negated grammar-field constraint |
| `(node == "x")`        | string predicate                 |
| `(node =~ /x/)`        | regex predicate                  |
| `/x/`                  | regex literal                    |
| `(ERROR)`              | `ERROR`-node pattern             |
| `(MISSING ...)`        | missing-node pattern             |
| `supertype#subtype`    | supertype constraint             |

Use **greedy** and **lazy** as the primary quantifier pair. Greedy initially prefers another repetition; lazy initially prefers the continuation. Either may backtrack if the rest of the pattern fails. “Non-greedy” may appear once as a recognition aid, but should not be the canonical term.

A sequence matches its pattern items against sibling positions in syntax-tree order. Anchors constrain the gaps and occupy no sibling position themselves. Without an anchor, the next pattern item searches forward among siblings; a sequence does not imply adjacency.

Quantifiers count successful pattern occurrences, not syntax-tree nodes. Every repetition iteration must consume at least one node. An empty outcome of a nullable element cannot become an iteration and cannot satisfy `+`. Surrounding navigation may search for the first occurrence; subsequent iterations are back-to-back under the active skip/anchor policy, and a gap ends the repetition.

A capture binds a pattern's result to a named result field. The result may be a node, record, list, option value, or variant value; describing captures as only naming nodes is too narrow. `@_` and `@_name` are discards: the pattern still matches structurally, but its result is suppressed. The `_name` spelling documents what was discarded and does not produce a result field.

**Capture type** names both this syntax and the resulting type. Analysis distinguishes how the forms determine that type:

- `:: str` and `:: bool` are built-in capture projections that change the captured representation;
- `:: Name` supplies an explicit result type name without changing the underlying matched shape.

A **structured capture** establishes a record materialization boundary around bubbling result fields. A **quantifier capture** is the umbrella for a **repetition capture** on `*`/`+` and an **optional capture** on `?`. These captures establish the element/value boundary for a list or option result. Repeated inner captures must have a repetition-capture boundary or be discarded; avoid the opaque public label “strict dimensionality.”

Qualify **query anchor** versus **regex anchor** whenever both the Plotnik pattern and an embedded regex are discussed.

### 2.3 Alternations

`[...]` is always an **alternation** at the syntax layer. Labels do not turn it into a different kind of syntax node. Source-order preference with backtracking is part of Plotnik's matching semantics, not part of the construct's name.

The matching priority has two regimes:

1. Across both regimes, all node-consuming outcomes precede all empty outcomes; empty outcomes then follow alternative source order.
2. In an unanchored resumable search, candidate positions are visited in navigation order. At each position, alternatives follow source order and are exhausted before the search advances.
3. Under bounded or anchored navigation, alternatives follow source order and each applies its own navigation policy. A higher-priority alternative may therefore match a later sibling than a lower-priority alternative would.
4. Continuation failure backtracks through the checkpoints created by the applicable regime.

This distinction matters when alternative-dependent namedness changes a soft anchor's skip policy. The alternatives do not necessarily share one candidate order.

Do not call this **ordered choice**. In parsing-expression grammars, ordered or prioritized choice normally commits to the first locally successful choice. Plotnik retains later alternatives for backtracking when the continuation fails. “First match wins” is also insufficient because a local match is not necessarily a successful complete match path.

There are two source forms:

- An **unlabeled alternation** has no alternative labels and adds no case identity. Result fields produced by its alternatives merge in the enclosing record. When a structured capture scopes those fields, its value is a **merged record**, not a union type. An unlabeled alternation with no inner result fields may be match-only; an outer node-valued capture may instead capture the matched node.
- A **labeled alternation** labels every alternative. When its value is produced, the labels name cases of a **variant type**. Each case is either no-payload or payload-bearing; the payload value may itself have option type. Plotnik currently permits an anonymous record payload or no payload. A no-payload case is tag-only in JSON.

Captures with the same name contribute to one result field across alternatives; their types must be compatible. When an alternative does not produce that field, Plotnik applies its fallback: normally an option value represented by `null`, `[]` for a required list, or `false` for a presence boolean. Presence in every alternative adds no field fallback but preserves any option type already present in the field.

Canonical examples:

```plotnik
[
  (identifier) @name
  (number) @number
]
```

This is an unlabeled alternation. Its fields merge into one record: `{ name: Option<Node>, number: Option<Node> }`.

```plotnik
Value = [
  Name: (identifier) @name
  Other: (number)
]
```

This is a labeled alternation producing a variant with one payload-bearing case and one no-payload case. The current JSON representation is either `{ "$tag": "Name", "$data": { "name": ... } }` or `{ "$tag": "Other" }`.

```plotnik
(program [Name: (identifier) @name Number: (number) @number])
```

Here no surrounding construct materializes the alternation as a value. The labels have no output effect; `name` and `number` contribute to the enclosing record. An absent ordinary value falls back to absence (`null` in JSON), a required list to `[]`, and a presence boolean to `false`.

An alternation cannot mix labeled and unlabeled alternatives. If labels occur where no value is produced, say that **the labels have no output effect** and that captures merge as for an unlabeled alternation. Do not say that an enum “degrades” to a union.

Reserve **node-consuming outcome** for a successful outcome whose matched extent contains at least one syntax-tree node. Cursor navigation is separate: the cursor may move across skipped nodes that the pattern does not consume, while a root match consumes a node without moving to another one. In public explanations, say directly that labels produce cases when an alternation is captured, collected, or used as a definition body. Internal analysis uses the three-state `OutputContext` defined below. Do not use bare “consumed” or “consumption” for both concepts. The consuming-before-empty priority above is specific to alternations; a lazy quantifier may prefer its empty path.

### 2.4 Matching terms

- A **cursor position** is the runtime location from which navigation proceeds.
- A **candidate node** is the syntax-tree node currently being considered for a node-consuming pattern.
- A **continuation** is the remaining matcher that must succeed after a subpattern outcome.
- A **checkpoint** stores the cursor position, a journal watermark, and enough frame/continuation state to restore the attempt before trying another successor arm.
- An **execution path** is one dynamic sequence of matcher states or bytecode instructions through an attempt.
- A **match outcome** is consuming or empty success. **No match** is failure, not a third successful result value.
- A **match-only definition** answers whether it matched but has no captured result value. In the CLI, success exits `0` and renders JSON `null`; no match exits `1`.

An epsilon transition is an implementation edge that consumes no syntax-tree node by itself. It is not an empty match: an execution path may contain many epsilon transitions and still produce a node-consuming outcome.

### 2.5 Anchors and root matching

Keep **anchor**, including for leading and trailing positions where “adjacency” would be inaccurate.

- A soft anchor always admits extra nodes. When both adjacent match paths are definitely named, it also admits anonymous nodes; because namedness can vary by alternative, this behavior can be path-specific.
- An exact anchor permits no intervening child node.
- A leading or trailing anchor constrains the gap to the corresponding parent boundary rather than the gap between two siblings.
- Neither anchor promises that source byte ranges touch. Whitespace omitted from the tree is not an intervening node.

Entry-point matching begins at the supplied syntax-tree root. This does not mean exhaustive whole-tree matching: node patterns remain open and unmentioned descendants are unconstrained. Use “root-anchored” only as secondary shorthand when it cannot be confused with the `.` and `.!` anchor operators.

### 2.6 Empty, missing, and absent states

These terms name different states and must not substitute for one another:

| Canonical term                    | Meaning                                                         |
| --------------------------------- | --------------------------------------------------------------- |
| nullable pattern                  | A pattern that can succeed without consuming a syntax-tree node |
| empty match                       | A runtime success that consumes zero syntax-tree nodes          |
| missing node                      | A Tree-sitter recovery node with an empty source range          |
| option value                      | A semantic result that may be absent                            |
| nullable representation           | A TypeScript or JSON encoding of an option using `null`         |
| empty list                        | A present list containing no elements                           |
| no-value flow / match-only output | Successful matching without result data                         |
| no-payload case                   | A variant case that carries no payload                          |

A missing node occupies a syntax-tree position even though its source range is empty. It is therefore not an empty pattern match. A result record key is always present; its value may have option type and use a nullable representation in TypeScript or JSON. The key is not an optional property.

## 3. Result vocabulary

The result model has three layers: output flow during analysis, the result schema, and runtime result values. A pattern may succeed without producing data, bubble a field set, or produce one value:

```text
OutputFlow =
  NoValue
  | Fields(FieldSetId)
  | Value(TypeId)

Type =
  Node
  | Text
  | Bool
  | Option(Type)
  | List(TypeId, ListMinimum)
  | Record(FieldSetId)
  | Variant(CaseSetId)
  | Reference(TypeDeclId)

TypeDeclaration = {
  id: TypeDeclId,
  name: Name,
  body: TypeId,
}

EntryResult = MatchOnly | Value(TypeId)

ResultSchema = {
  types,
  field_sets,
  case_sets,
  declarations,
  entry_results,
}

ListMinimum = Zero | One

Value =
  NodeValue
  | TextValue
  | BoolValue
  | Absent
  | ListValue(Values)
  | RecordValue(Fields)
  | VariantValue(CaseId, CasePayload)

CasePayload = NoPayload | RecordValue(Fields)
```

Definitions:

- **No-value flow** means matching succeeds without producing output data. At a public entry point this is **match-only output**, not a unit value in the result algebra.
- **Fields flow** bubbles a field set into an enclosing result scope without first packaging it as a record value. A materialization boundary may turn the field set into `Record(FieldSetId)`.
- A **node**, **text value**, and **boolean** are distinct leaf types. Reserve scalar for runtime scalar framing, not as a public type constructor.
- A **type declaration** gives a structural body a name; naming is orthogonal to shape. A **type reference** points to a declaration, including a recursive definition. A custom capture type over a node creates a declaration whose body is `Node`, not a special node shape.
- An **option type** represents zero or one semantic value. It is target-neutral; nullable is a property of its TypeScript/JSON representation. Plotnik has one observable absence state, so option is idempotent: `Option<Option<T>> = Option<T>`.
- A **record** is a set of named result fields with one value per field.
- A **list** is an ordered sequence of values of one element type. Its semantic minimum is zero for `*` and one for `+`.
- A **variant type** is a tagged sum type with named cases. A value selects exactly one case. Each case either has no payload or carries one anonymous record payload; that payload may contain option-typed fields.

### 3.1 Target mappings

| Semantic concept  | Rust                                | TypeScript                                          | JSON                             |
| ----------------- | ----------------------------------- | --------------------------------------------------- | -------------------------------- |
| node              | `tree_sitter::Node<'t>` handle      | target-configured generated `Node` representation   | `{ kind, text, span }`           |
| text              | `&str`                              | `string`                                            | string                           |
| boolean           | `bool`                              | `boolean`                                           | boolean                          |
| option type       | `Option<T>`                         | `T \| null`                                         | value or `null`                  |
| record            | struct                              | object type or interface                            | object                           |
| zero-or-more list | `Vec<T>`                            | `T[]`                                               | array                            |
| non-empty list    | `Vec<T>`                            | `[T, ...T[]]`                                       | array                            |
| variant type      | enum                                | discriminated union                                 | adjacently tagged representation |
| match-only output | marker type exposing match-only API | generated alias configured as `undefined` or `null` | fixed `null` in CLI output       |

Use **option**, **record**, **list**, and **variant** in target-neutral compiler and language documentation. Use nullable, struct, array, `Vec`, enum, and discriminated union only where the named target makes them exact.

Targets do not preserve every semantic distinction:

| Property                            | Rust                           | TypeScript                     | JSON                                     |
| ----------------------------------- | ------------------------------ | ------------------------------ | ---------------------------------------- |
| Option nesting                      | Normalized to one option layer | Normalized to one `\| null`    | One `null` state                         |
| Non-empty-list guarantee            | Not expressed by `Vec<T>`      | Expressed by a rest tuple      | Not expressed                            |
| Record/variant declaration identity | Nominal                        | Structural                     | Not expressed                            |
| Transparent alias identity          | Not distinct from aliased type | Structural                     | Not expressed                            |
| Variant case                        | Preserved by enum              | Preserved by discriminant      | Preserved in `$tag`; not schema-enforced |
| Match-only vs absent option         | API-distinct                   | Depends on configured sentinel | May both be `null`                       |

These are representation properties, not alternate semantic type systems. Generated Rust must not expose nested option states that the matcher result protocol cannot produce.

The generated TypeScript `Node` describes serialized node data, not a live Tree-sitter binding handle. When a target configuration includes points, the TypeScript declaration names them `startPoint` and `endPoint`.

The existing JSON form is an **adjacently tagged representation**:

- an alternative label derives a semantic **case name**; `$tag` stores that case name as its **tag value**;
- `$data` contains the **case payload** and is absent for a no-payload case;
- in TypeScript-specific prose, `$tag` is the discriminant property;
- do not call `$tag` a Rust discriminant, which is logically an integer value.

Whether payload fields should instead be spliced beside `$tag` is a result-representation design decision, not a terminology decision, and is outside this contract. `$tag` and `$data` are reserved encoding keys; capture names cannot collide with them.

### 3.2 Fields, properties, and members

Use qualifiers whenever grammar and output concepts share a page:

- A **grammar field** relates a parent syntax node to a child.
- A **result field** belongs to a target-neutral record.
- A Rust renderer emits a struct **field**.
- A TypeScript renderer emits an object **property**.
- A TypeScript union contains union **members**.
- A variant type contains **cases**; a Rust enum contains **variants**.

`member` is acceptable as internal storage vocabulary for a table that contains both record fields and variant cases. It should not erase the more precise terms in public documentation.

### 3.3 Field completion

Field production and final record schema are separate. Analysis first records what one alternative produced, then determines how to complete missing contributions, then derives the final field type:

```rust
struct ProducedField { value_type: TypeId }

enum FieldCompletion {
    AlwaysPresent,
    Absent,
    EmptyList,
    False,
}

struct RecordField { final_type: TypeId }
```

`AlwaysPresent` means every alternative produces the field. `Absent` makes the final field type `Option<T>`, normalized according to the single-absence rule, and renders as `null` in TypeScript/JSON. `EmptyList` changes the final list minimum to zero. `False` yields a required boolean field. Completion belongs to analysis/lowering; it is not a permanent omission flag on the final record field. Public prose should describe the fallback value rather than saying the result key itself is omitted or optional.

### 3.4 Lists, not rows

Do not use **row**, **row struct**, or **row wrapper** in user-facing language. A repeated structured capture produces a **list of records**; an optional structured capture produces an **option of a record**, commonly rendered as a nullable record in TypeScript/JSON. In implementation code, prefer `element`, `element_shape`, or `element_record` when naming the scope of one collected value.

## 4. Tree-sitter vocabulary

Use Tree-sitter's terms literally:

- **named node**: a node whose Tree-sitter named flag is true, ordinarily from a named grammar rule or alias;
- **anonymous node**: a node whose Tree-sitter named flag is false, ordinarily from a grammar string literal;
- **extra node**: a node admitted by the grammar's extras behavior, such as a comment, that may appear between surrounding grammar symbols;
- **missing node**: an inserted recovery node with an empty source range;
- **`ERROR` node**: source text that could not be incorporated normally;
- **node kind**: the node's grammar-visible kind;
- **grammar field**: the named parent-child relation;
- **syntax tree** or **concrete syntax tree**: the parser output.

Use **AST** only for Plotnik's own abstract query representation. The tree of a matched source document is a syntax tree/CST even when a display hides anonymous nodes.

Avoid **trivia** in introductory or source-language documentation. Anonymous nodes include meaningful punctuation and operators, so the ordinary-language meaning of trivia is misleading. Prefer **anonymous-or-extra nodes** or describe the exact skip policy.

If **trivia** remains in navigation internals or low-level documentation, define it explicitly as a Plotnik navigation class:

```text
trivia node = anonymous node or extra node
```

Do not attribute that grouping to Tree-sitter, imply that every member is semantically insignificant, or imply that extra and anonymous are mutually exclusive properties. `AnonymousOrExtra` is preferable when an internal type must name that union. Soft-anchor skippability remains path-dependent and is not a fixed node class.

Keep the following nouns distinct:

- A **grammar** defines node kinds, fields, and productions.
- A **grammar artifact** is the exact `grammar.json` snapshot.
- A Tree-sitter **Language object** is the runtime parsing description.
- A **parser** is configured with a Language object and produces a syntax tree.
- An internal **language profile** groups the selected language, Tree-sitter Language object, grammar metadata, aliases, and registry identity used by the CLI and compiler. Public CLI prose normally calls the selection simply a **language**.

### 4.1 Same ancestry, Plotnik semantics

Tree-sitter familiarity helps, but shared terms do not imply identical syntax or behavior:

| Concept               | Tree-sitter queries                       | Plotnik                                                                                           |
| --------------------- | ----------------------------------------- | ------------------------------------------------------------------------------------------------- |
| capture               | associates a name with matched nodes      | binds a pattern result to a result field and may materialize records, lists, options, or variants |
| `.` anchor            | applies Tree-sitter's anonymous-node rule | soft anchor with extra-node and path-dependent namedness behavior                                 |
| exact anchor          | no corresponding `.!` form                | `.!` permits no intervening child node                                                            |
| sibling sequence      | parenthesized query patterns              | `{...}`                                                                                           |
| negated grammar field | `!field`                                  | `-field`                                                                                          |
| predicate             | predicate forms such as `#eq?`            | inline string and regex predicates                                                                |

State these divergences near introductory syntax rather than asking users to infer them from advanced lowering documentation.

## 5. Compiler and runtime vocabulary

### 5.1 Pipeline

Use the stage verbs according to their outputs:

1. **Parse**: query text to syntax/AST.
2. **Analyze**: query-only semantic validation, name resolution, recursion checks, result-shape inference, and other grammar-independent facts.
3. **Bind**: bind node kinds and grammar fields to one Tree-sitter grammar and validate grammar-dependent facts.
4. **Lower**: produce and optimize the target-neutral matcher NFA.
5. **Emit**: produce one target artifact from the matcher NFA, result schema, or both. Matcher emission consumes the NFA and schema; type-only emission primarily consumes the schema.

Inside emission, use **bytecode packing**, **Rust matcher emission**, **Rust type emission**, and **TypeScript declaration emission** where the distinction matters. Do not call a bytecode-packed representation merely “lowered” when a target-neutral matcher NFA already exists.

### 5.2 NFA, bytecode, and metering

Keep the representation boundary explicit:

```text
pattern -> matcher NFA -> bytecode instruction stream -> VM dispatches
                      \-> generated matcher states -> generated dispatches
```

- An NFA contains **states** connected by **transitions**.
- An epsilon transition consumes no syntax-tree node.
- Bytecode contains **instructions** selected by opcodes.
- A **code address** or **instruction address** identifies the first bytecode word of an instruction.
- A **bytecode word** is the fixed 8-byte allocation and address unit. One instruction may occupy several bytecode words.
- An **operand slot** is one 16-bit position in an instruction payload.
- A **successor address** is an encoded non-terminal control-flow target.
- A **matcher dispatch** is one executor-loop iteration: one bytecode instruction in the VM or one generated matcher state in emitted code.

Reserve **step** for debugger actions such as single-stepping. Do not use it simultaneously for an instruction address, storage slot, dispatch count, and trace record. A dispatch is an internal execution event. **Fuel** is the public cross-backend work budget; each matcher dispatch currently consumes one fuel unit. Fuel usage is a safety limit, not a stable cross-version performance benchmark. This vocabulary remains valid if operation weights change later.

### 5.3 Result construction and tracing

These artifacts are different:

- A **match journal** is the rollbackable sequence recorded during matching. Its current prefix is speculative and may be truncated on backtracking; after acceptance, the surviving prefix is committed.
- An **output event** constructs result data. An **inspection event** records source/result provenance. `JournalEvent` is the umbrella when both event kinds share one physical journal.
- **Output events** are the logical result-construction view of the committed journal. `OutputEvents` excludes inspection events.
- An **execution trace** records what the VM did for debugging.
- A **result provenance map** connects query spans, matched document bounding ranges, result bindings, and event ranges for inspection.

Use **trace** only for execution tracing. Use **output event** or `OutputEvents` for result-construction data and `ResultDecoder` for the consumer that constructs a typed result. `ResultDecodePlan` is the target-neutral schema-derived plan used by such a decoder. An effect remains a valid compiler term for an operation that appends to or otherwise mutates the match journal; the emitted event is not itself an effect.

At the target-neutral runtime and bytecode boundaries, the output-event variants of `JournalEvent` should follow semantic shapes: `ListOpen`, `RecordOpen`, `VariantOpen`, and so on, with corresponding opcodes. Compiler operations that append those events may continue to be called effects; array/struct/enum should not survive merely because they were historical wire names.

Use **materialize** for constructing the generic dynamic result. Use **decode** for constructing generated typed output from the output-event stream. Reserve **replay** for reproducing a recorded execution; typed result construction is not replay.

### 5.4 Source coordinates

Always qualify the coordinate domain:

- A **query span** combines query identity, query-source identity, and a query byte range.
- **document source**, **document byte range**, and `DocumentByteRange` refer to the code being matched.
- A **document bounding range** is the smallest document byte range bounding possibly discontiguous contributing nodes; it is not exact coverage.
- A **generated-artifact range** is qualified by artifact, such as `TypeScriptRange`; it is never merely a source range.
- A **bytecode-word address** identifies storage in the instruction stream.
- A **journal-event range** indexes the match journal.
- An **execution-trace record index** indexes debugger records.
- A **JSON Pointer** names a value location in JSON inspection output. Use **result path** only for a genuinely representation-neutral path model.

Avoid unqualified `source`, `span`, and `range` when both domains are present.

## 6. Internal Rust naming conventions

Precision should come from the type system and module context, not stacks of nouns. Shared boundary types should remain searchable; short names are most appropriate after a module has established the domain.

The parser should model the one syntactic construct directly:

```rust
Pattern::Alternation(AlternationPattern)

enum Labeling {
    Unlabeled,
    Labeled,
    Mixed, // invalid input retained for recovery
}

struct Alternative(/* ... */);
```

The analysis model should keep output flow separate from value types. The following is a vocabulary sketch, not a demand to collapse existing IDs and interning into recursive Rust values:

```rust
enum OutputFlow {
    NoValue,
    Fields(FieldSetId),
    Value(TypeId),
}

enum RootExtent {
    SingleNode,
    Other,
}

enum TypeShape {
    Node,
    Text,
    Bool,
    Option(TypeId),
    List { element: TypeId, minimum: ListMinimum },
    Record(FieldSetId),
    Variant(CaseSetId),
    Ref(TypeDeclId),
}

struct TypeDeclaration {
    id: TypeDeclId,
    name: Symbol,
    body: TypeId,
}
```

`RootExtent` is the static top-level extent used to decide whether a definition is selectable as an entry point. `Other` covers zero, multiple, or variable top-level nodes without claiming they have one common arity. Root extent is distinct from nullability, list minimum, node consumption, and the number of nodes visited internally. Do not call it general arity.

Use concise operations such as `compile_alt`, `merge_fields`, `FieldCompletion`, `open_variant`, and `decode_result`. Field merging and per-field completion are different operations; do not call the completion table a merge plan. Avoid names such as `UnlabeledAlternationPatternDescriptor`; `Pattern` or `Descriptor` adds no distinction there. Within `alternation.rs`, `alt`, `alts`, and `compile_alt` are appropriate even though the shared AST uses the searchable full names.

Target-specific names belong inside target-specific modules:

- `rust::emit_enum` is exact;
- `typescript::emit_discriminated_union` is exact;
- target-neutral analysis should use `Variant`, not `Enum`;
- target-neutral matching should use `Alternation`, not `Union`.

Reserve **node consumption** for matching progress. Output handling has three contexts:

```rust
enum OutputContext {
    Fields,
    Value,
    Discard,
}
```

`Fields` bubbles result fields into the enclosing scope and does not expose an opaque value. `Value` makes an eligible pending value observable; the pattern and capture kind still determine whether child fields are packaged as a record. `Discard` suppresses all result construction. This replaces the overloaded `Consumption` vocabulary and distinguishes field bubbling from explicit discard.

Names should describe roles rather than containers or implementation accidents:

- `GrammarBinding`, not a generic `Context`, when it contains bound grammar identities;
- `CodeAddr(u16)` for any bytecode-word address, including zero;
- `SuccessorAddr(NonZeroU16)` for an encoded non-terminal successor, preserving the type-level zero/sentinel distinction currently hidden by step vocabulary;
- `MatchJournal` for the rollbackable/committed physical sequence;
- `OutputEvents` for its logical result-construction view;
- `ResultDecoder`, not `TraceReader`, when it decodes output events;
- `ResultDecodePlan`, not `ReplayPlan`, for the schema-derived typed construction plan;
- `TraceNode` for an inline node snapshot in execution-trace data and `GrammarNodeRef` for a reference to a grammar-visible node kind, not two unrelated `NodeRef` types.

Do not repeat a module's noun in every child identifier. Within `alternation.rs`, `compile`, `style`, and `alternatives` may be clearer than `compile_alternation`, `alternation_style`, and `alternation_branches`.

## 7. Legacy-to-canonical replacements

| Legacy or overloaded wording     | Canonical replacement                                            |
| -------------------------------- | ---------------------------------------------------------------- |
| union alternation / Union Style  | unlabeled alternation; merged-record output                      |
| enum alternation / Enum Style    | labeled alternation; variant output                              |
| enum branch / union branch       | alternative in syntax; successor arm in control flow             |
| ordered choice                   | alternation with source-order preference and backtracking        |
| first match wins                 | hierarchical candidate/alternative priority with backtracking    |
| labels degrade to a union        | labels have no output effect; captures merge                     |
| struct                           | record, except in Rust-specific output                           |
| array                            | list semantically; array only for JSON/TypeScript representation |
| row / row struct                 | record element / list of records                                 |
| optional field                   | result field whose value has option type                         |
| zero-width pattern               | nullable pattern                                                 |
| zero-width success               | empty match                                                      |
| void result                      | match-only output / no-value flow                                |
| void variant                     | no-payload case / tag-only JSON object                           |
| annotation / capture type syntax | capture type                                                     |
| suppressive capture              | discard                                                          |
| AST of document code             | syntax tree / CST                                                |
| target language for matched code | source language                                                  |
| bytecode transition              | instruction; transition only in the NFA                          |
| wire step                        | bytecode word                                                    |
| VM step count                    | public fuel usage; internal matcher dispatch count               |
| capture trace                    | match journal or output events, according to event content       |
| typed replay                     | typed result decoding                                            |
| inspection provenance            | result provenance map                                            |

## 8. Public diagnostics

Diagnostics should teach the canonical model. Representative rewrites:

| Current idea                                         | Canonical wording                                                                     |
| ---------------------------------------------------- | ------------------------------------------------------------------------------------- |
| cannot mix enum and union branches                   | an alternation cannot mix labeled and unlabeled alternatives                          |
| branch labels become enum variants                   | alternative labels name variant cases when the alternation produces a value           |
| use an enum if captures are mutually exclusive       | use labeled alternatives to preserve which alternative matched                        |
| capture the alternation to make labels enum variants | capture the alternation to make its labels produce variant cases                      |
| labels degrade to a plain union                      | the labels have no output effect here; captures merge as for an unlabeled alternation |
| strict anchor means byte adjacency                   | an exact anchor permits no intervening syntax-tree node                               |
| matching covers the entire tree                      | matching begins at the syntax-tree root; unmentioned descendants remain unconstrained |

Prefer a direct repair over taxonomy-heavy wording. For example, “either label every alternative or remove all labels” may be more useful than naming both forms when the names do not help the correction.

## 9. Canonical public surfaces

Backward compatibility and migration cost are outside this contract. Public and internal surfaces should use the canonical end-state names directly:

- **Execution budget:** the public resource is **fuel**. Use `--fuel`, `fuel_used`, and `fuel_limit`; `OutOfFuel` is the fuel-exhaustion case of the generated API's `LimitExceeded` error. The umbrella remains necessary because execution may also exceed memory or typed-decode depth limits. One matcher dispatch currently consumes one fuel unit. Debug execution data may separately expose `matcher_dispatches`.
- **Generated query API:** use the familiar `Parse` / `parse` and `Matches` / `matches` traits and helpers. Here parsing means applying a generated query to an existing syntax tree and producing its typed value; Tree-sitter's `Parser::parse` separately produces that syntax tree from source text. The argument and return types make the two layers clear without encoding the input container into the method name.
- **Tree display:** the command and WASM function are `tree`. Use `--query-view ast|cst` for a Plotnik query and `--include-anonymous` / `include_anonymous` for a source-language syntax tree. When both are shown, label them “Query AST” or “Query CST” and “Source syntax tree.” JSON uses `query_tree` and `source_tree`, never bare `source`; web types and filenames use `Tree`, not `Ast`, for the source-tree pane.
- **Node output:** JSON uses the idiomatic `{ kind, text, span: [start, end] }`; the end is exclusive and both offsets are UTF-8 bytes. TypeScript `--include-points` adds `startPoint` and `endPoint`; their `row` and byte-based `column` are zero-based. Other target code generators may choose a different idiomatic node representation through target-specific configuration; target-neutral analysis owns only the document byte-range semantics.
- **Inspection spans:** use `SpanKind::Alternation(Labeling)` and `SpanKind::Alternative`. Inspection JSON represents both alternation forms as `kind: "alternation"` with `labeling: "unlabeled"` or `"labeled"`; an alternative uses `kind: "alternative"`.
- **Binary and runtime vocabulary:** type kinds, output events, opcodes, dumps, and format documentation use list/record/variant and bytecode-word/dispatch terminology. Historical wire names do not form a naming boundary.

Target-specific names that are exact remain target-specific: a Rust emitter still emits structs and enums, while TypeScript emits object types and discriminated unions.

### 9.1 Inspection JSON

The inspection bundle qualifies names at boundaries where several domains meet. Within a nested object, the object supplies its domain and child names stay short:

```text
version
query_spans
query_tokens
diagnostics
typescript_declarations
typescript_bindings
entry_points
result
result_provenance
run_stats
execution_trace
error
```

A query-span entry within `query_spans` uses `id`, `source_id`, `kind`, `labeling`, `span`, and an optional `binding`. A binding names `type_id` and an optional `member_id`; the latter is the shared identity used to join this table to target bindings, not a claim that every target renders the member as a field or case.

A result-provenance entry uses `query_span_id`, `parent`, `source_span`, `bindings`, and `range`. `parent` is an index into the same `entries` array. Each binding uses a JSON-Pointer-style `path` relative to the output-builder frames represented by its entry and an `event_index`; consumers form an absolute result path by joining paths down the parent chain. `query_span_id` remains qualified because it is a foreign key into a different top-level collection.

The corresponding Rust field is `event_range`: unlike the nested JSON object, the struct simultaneously contains source and journal coordinates and therefore needs the qualifier.

Every span uses a tuple representing the half-open interval `[start, end)`. Query spans, query tokens, TypeScript bindings, node values, and result provenance keep that representation; their containing fields distinguish the coordinate domain. `source_span` may bound discontiguous contributions, but it is still the smallest source span containing them, not a new range object. A provenance entry's `range` is likewise half-open and can be used directly to slice the match journal. A TypeScript binding also names `type_id` and an optional `member_id`. Run statistics use `fuel_used` and `peak_live_heap_bytes`. Do not expose ambiguous names such as bare top-level `spans`, `source`, `inspection`, `hull`, `effect_range`, or `trace`; short names such as `id`, `kind`, `span`, `range`, and `path` are appropriate once their containing object establishes the domain.

### 9.2 Command verbs

- `check` validates and compiles a query for a selected language without executing it.
- `infer` emits result type declarations for a selected target language.
- `run` executes a selected entry point and materializes its result.
- `generate` emits a target artifact.
- `inspect` produces the compiler/runtime inspection bundle.
- `trace` records an execution trace.
- `tree` displays a query representation, source syntax tree, or both.
- `dump` renders a bytecode module for compiler/runtime debugging.
- `lang list` lists available languages and aliases.
- `lang dump` renders the selected language's grammar metadata.
- `completions` emits shell-completion definitions.

## 10. Acceptance criteria

The terminology implementation is coherent when:

- every `[...]` is called an alternation in syntax documentation and syntax metadata;
- no unlabeled alternation output is described as a union type;
- no labeled alternation is called enum syntax;
- output flow, result schema, and runtime result values are distinct;
- node, text, boolean, option, record, list, variant, type declaration, and type reference are the target-neutral type vocabulary;
- Rust, TypeScript, and JSON terms appear only with their target made clear;
- nullable patterns, empty matches, missing nodes, option values, nullable representations, empty lists, and match-only outputs remain distinct;
- target documentation states option normalization and where non-empty lists or named identity lose precision;
- public text qualifies grammar fields versus result fields and query spans versus document ranges where those domains meet;
- trace means execution trace, the rollbackable physical stream is a match journal, and result construction consumes output events;
- bytecode uses instruction/address/word/dispatch vocabulary, NFA code uses state/transition vocabulary, and public work limits use fuel;
- generated APIs use the familiar `Parse` / `parse` and `Matches` / `matches` vocabulary for typed query application;
- tree display options, node coordinates, and inspection JSON use the contextual canonical names from section 9;
- internal Rust names are concise within their modules and do not encode a target representation above the target boundary.

## 11. Implementation inventory

This appendix is non-normative. It prevents the redesign from stopping at the most visible prose while leaving contradictory names on adjacent surfaces.

| Canonical concept               | Legacy names/surfaces to search                                                       | Expected treatment                                                                                           |
| ------------------------------- | ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| alternation syntax              | `Pattern::Union`, `Pattern::Enum`, `UnionPattern`, `EnumPattern`, `AltKind`, `Branch` | One shared `AlternationPattern`, `Alternative`, and labeling classification                                  |
| variant result                  | `TypeShape::Enum`, `TypeKind::Enum`, `EnumOpen/EnumClose`, Rust and TS emitters       | `Variant` outside the Rust renderer; Rust output remains `enum`                                              |
| merged fields                   | `compile_union`, union diagnostics/comments                                           | Merge vocabulary; keep alternative matching separate from field unification                                  |
| field completion                | `UnionFlowPlan`, `FieldInfo::optional`, `FieldFallback`                               | `FieldCompletion::{AlwaysPresent, Absent, EmptyList, False}`; derive final `RecordField` afterward           |
| output context                  | `Consumption::Consumed`, binary consumed/ignored flags, suppressive special cases     | `OutputContext::{Fields, Value, Discard}`; reserve consume for node progress                                 |
| root extent                     | `Arity::{One, Many}` used for entry eligibility                                       | `RootExtent::{SingleNode, Other}`                                                                            |
| option type                     | `TypeShape::Optional`, `FieldInfo::optional`, nullable/optional prose                 | One flat semantic option state; separate it from field completion and nullable rendering                     |
| type declarations               | `TypeShape::Custom`, `type_names`, definition refs, generated aliases                 | One `TypeDeclaration`/`TypeDeclId` graph orthogonal to structural `TypeShape`                                |
| list and minimum                | `TypeShape::List { minimum }`, bytecode list kinds, generated `Vec`/TS arrays         | List semantically; record target precision                                                                   |
| match-only flow                 | `TypeShape::NoValue`, `PatternFlow::NoValue`, match-only CLI/type settings            | Separate flow from value type; keep public output described as match-only                                    |
| syntax inspection               | `SpanKind::Union`, `SpanKind::Enum`, `SpanKind::Branch`                               | `SpanKind::Alternation(Labeling)` and `SpanKind::Alternative`; JSON shape specified above                    |
| match journal and output events | `EffectLog`, `RuntimeEffect`, bytecode `EffectKind`, capture trace wording            | `MatchJournal`, umbrella `JournalEvent`, filtered `OutputEvents`; effect only for compiler append operations |
| typed construction              | `TraceReader`, `ReplayPlan`, replay-depth wording                                     | `OutputEvents`, `ResultDecoder`, `ResultDecodePlan`, and decode depth                                        |
| grammar-dependent stage         | `link`, linked/bound query names                                                      | `bind`; use bound for the resulting grammar-bound artifact                                                   |
| VM code coordinates             | `StepId`, raw step indices, `STEP_SIZE`, payload slots                                | Code/instruction address, bytecode word, operand slot; distinguish successor targets                         |
| VM metering                     | `--max-steps`, `steps_used`, step-limit diagnostics, macro `steps`                    | Public fuel vocabulary; internal matcher dispatches; `LimitExceeded::OutOfFuel` for fuel exhaustion          |
| generated query API             | `MatchTree`, `match_tree`, `IsMatch`, `is_match`                                      | Restore `Parse`, `parse`, `Matches`, and `matches`                                                           |
| document tree display           | `ast` command and `--raw` meanings                                                    | Replace with `tree`; use precise query/source labels                                                         |
| node output                     | TypeScript `Node`, VM/generated serializers, `--verbose-nodes`                        | JSON `{ kind, text, span: [start, end] }`; target-configured generated representation; `--include-points`    |
| inspection bundle               | `spans`, `source`, `type`, `member`, `inspection`, `hull`, `effect_range`, `trace`    | Qualified top-level collections and context-shortened nested names from section 9.1                          |
| leaf-pattern AST                | `TokenPattern` combines anonymous-node literals and `_`; broad `NodePattern` variants | Split precise wrappers or retain a neutral leaf wrapper with an explicit kind; do not call `_` a token       |
| source coordinates              | unqualified `source`, `span`, `range`, `NodeHandle` wording                           | Query span versus document byte range or document bounding range                                             |
| overloaded node reference       | both public `NodeRef` types                                                           | `TraceNode` versus `GrammarNodeRef`                                                                          |

Implementation should expand this inventory with exact symbols and tests before each coherent change set. It is a coverage aid; scope should follow semantic boundaries rather than compatibility concerns.

## Research basis

The main external precedents are:

- Tree-sitter query syntax: patterns, captures, alternations, anchors, named and anonymous nodes, missing nodes.
- Rust `regex`: alternation preference, greedy and lazy quantifiers, assertions.
- Parsing expression grammars: the commit semantics that make “ordered choice” a poor name for Plotnik's backtracking behavior.
- WIT: target-neutral records, lists, variants, and payload-bearing or no-payload cases.
- Rust Reference: structs, enums, variants, discriminants, unions, arrays, and unit-like forms.
- TypeScript Handbook: object types, properties, union members, discriminated unions, arrays, tuples, and optional properties.
- Serde: externally, internally, adjacently, and untagged enum representations.
- Zod and schema-validation APIs: `parse` as validated interpretation into a typed value.
- Tree-sitter parser documentation: grammar, Language object, parser, and concrete syntax tree.
