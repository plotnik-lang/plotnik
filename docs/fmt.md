# Plotnik Query Formatting

Canonical formatting rules for `.ptk` queries — the specification for the
formatter. The rules are informed by the hand-formatted query text of the
snapshot corpus (`crates/plotnik-tests/tests/`). Exact agreement numbers
must be recomputed from the implementation before they are documented; the
working draft no longer claims results from an unavailable reference tool.

Terminology follows the parser's CST (`compiler/parse/cst.rs`): `Def`,
`NamedNode`, `Sequence` (`{...}`), `Alternation` (`[...]`) with `Alternative`s,
`Field`, `NegatedField`, `Anchor`, `NodePredicate`, `Capture`,
`CaptureType`, `Quantifier`, `Str` (string literal), and `Wildcard` (`_`).

## Contract

- **Input must parse cleanly.** The formatter formats the CST, not text. A
  file with parse errors is left untouched (recovery CSTs are not formattable).
  Parse- or analyze-level _warnings_ (e.g. Tree-sitter-style `((a) (b))`) do
  not block formatting. A parse failure returns its diagnostics together with
  the matching source map, so callers can render it without rebuilding context.
- **Semantics-preserving.** Output differs from input only in whitespace,
  plus the explicit [normalizations](#normalizations). Comments are preserved.
- **Idempotent and canonical.** Formatting is a pure function of the
  significant CST plus the authored placement class of comments. Outside
  comments, authored line breaks carry no information. A comment's
  own-line/inline/trailing role is the sole layout information retained from
  source whitespace.

## Layout model

Every pattern is rendered in one of two modes:

- **inline** — the whole pattern on one line, single spaces between parts;
- **broken** — the pattern's items fan out, one item per line, indented.

The decision is bottom-up, per group (`NamedNode`, `Sequence`, `Alternation`):

```
broken(group) :=
     structural_break(group)             // the rules below
  or inline form has 4+ semantic landmarks // density rule
  or any child group is broken           // breaks propagate upward
  or a boundary comment requires lines
  or inline rendering would overflow the line width
```

There are only two shapes. No half-broken forms: no hugging the opening
delimiter of a broken child onto the parent line, no stacking closers
(`))))`), no aligning several items in columns, and never more than one item
per line in a broken group.

### Constants

| Constant               | Value              |
| ---------------------- | ------------------ |
| Indent                 | 2 spaces/level     |
| Line width             | 80 Unicode scalars |
| Inline landmark budget | 3                  |

Formatter-generated whitespace never contains tabs; tabs inside preserved
comments remain verbatim. Width uses `chars().count()` over the entire line,
indent included. An atomic token, comment line, itemless group, or group head
that cannot be usefully split may exceed the limit.

### Structural breaks

These force the broken mode even when the inline form would fit:

| Construct     | Breaks when                                           |
| ------------- | ----------------------------------------------------- |
| `Alternation` | it has 2+ alternatives, or any alternative is labeled |
| `Sequence`    | it has 2+ items, **anchors included**                 |
| `NamedNode`   | it has 2+ items, **anchors excluded**                 |

"Item" means a child pattern, a `Field`, or a `NegatedField`. The node kind,
a `NodePredicate`, and trailing suffixes are part of the head/closer, never
items. Anchors are items for layout (they get their own line when broken) but
only count toward the sequence threshold, not the node threshold — positional
anchors around a single child are part of how that child is addressed:

```
Q = (array . (element) .)        // stays inline: one real item
Q = (block .! (statement))       // stays inline
Q = (program .)                  // the idiomatic emptiness check, inline
```

whereas a bare sequence exists only to list siblings, so any second item —
anchor or not — makes it a list:

```
Q = {
  (a)
  .
  (b)
}
```

### Semantic density

An inline candidate may contain at most three semantic landmarks. Four or more
forces the nearest group with a breakable item into broken mode. The budget
measures how many distinct matching, navigation, and output decisions a reader
must decode on one line; it is independent of byte length and nesting depth.

Each of these CST constructs counts as one landmark:

- pattern: named node, sequence, alternation, definition reference, string,
  wildcard;
- navigation and structure: field, **labeled** alternative, anchor, negated field;
- pattern detail: node predicate, quantifier, capture, capture type.

Unlabeled alternative wrappers do not count. Definitions, comments, punctuation,
literal contents, regex internals, and category-refinement tokens do not count.
Field and alternative-label prefixes and quantifier, capture, and capture-type suffixes
belong to the complete inline candidate even when the renderer flattens those
wrappers around a group.

Three nested match landmarks remain a concise path:

```
Q = (foo (bar (baz)))
```

A fourth nested match breaks the nearest useful container:

```
Q = (foo
  (bar (baz (qux)))
)
```

The same budget applies across syntax categories:

```
Q = (pair key: (identifier))  // node + field + node: inline
Q = (identifier == "x") @id   // node + predicate + capture: inline

Q = (program                 // ancestor makes four: broken
  (identifier == "x") @id
)
```

Captures are landmarks, so the former capture-only rule is a consequence of
the general budget: valid syntax that would put two capture suffixes in one
candidate necessarily has at least four landmarks. Canonical output therefore
still does not normally put two capture suffixes on one line:

```
Q = {
  (identifier) @id
} @inner

Q = (program
  (expression_statement
    (identifier) @id
  ) @stmt
)
```

If a group has no breakable item, density is unavoidable just like width: the
formatter keeps the atomic or itemless head inline rather than inventing an
unsupported half-broken shape. Long simple tokens likewise remain one
landmark; only the width rule applies to their physical length.

Consequences of the table, spelled out:

- An alternation of two or more alternatives is always vertical, even `[(a) (b)]`.
- A labeled alternation is always vertical, even with one alternative — labels
  read as case declarations. (Exception-free: `[A: (x)] @e` still breaks.)
- A single-alternative unlabeled alternation, a single-item sequence, and a
  single-child node stay inline while they fit: `[(identifier)] @arg`,
  `{(identifier) @id}`, `(program (E) @e)`.
- Chains of single-child nodes stay inline while the complete candidate has at
  most three landmarks and fits within 80 scalars. Longer paths break in
  readable chunks of at most three landmarks:

  ```
  Q = (program
    (expression_statement
      (binary_expression (identifier == "b"))
    )
  )
  ```

- A node acquires a second item and immediately fans out, however short:

  ```
  Q = (binary_expression
    left: (_) @left
    right: (_) @right
  )
  ```

### Broken shape

When a group breaks:

1. The opening delimiter stays where the group began — after `Name = `, after
   `field: `, after `Label: `, and the node kind stays glued to its paren
   (`(function_declaration`). A `NodePredicate` stays on the head line.
2. Each item goes on its own line, one indent level deeper. Anchors, negated
   fields, and comments occupy lines of their own.
3. The closing delimiter goes on its own line, at the indent of the line that
   opened the group, with the whole suffix chain attached:
   `)`, `)* @funcs`, `]? @kind :: Kind`, `}+ @records`.
4. An item that is itself a group repeats the procedure at its indent. A
   broken field value keeps `field: ` and the opener on the field's line:

```
Q = (call_expression
  function: [
    (identifier) @fn
    (member_expression) @m
  ]
)
```

The canonical composite example:

```
Expr = [
  Lit: (number) @value
  Rec: (call_expression
    function: (identifier) @fn
    arguments: (Expr) @inner
  )
]
```

## Spacing

Inline, exactly one space separates sibling parts. The full token-pair table:

| Position                     | Spacing                                        |
| ---------------------------- | ---------------------------------------------- |
| `Name = pattern`             | one space each side of `=`                     |
| after `(kind`, between items | one space: `(call (a) (b))`                    |
| inside empty delimiters      | none: `()`, `{}`, `[]`                         |
| delimiter to content         | none: `(foo)`, `{(a)}`, `[(a)]`                |
| `field:` / `Label:`          | no space before `:`, one after                 |
| quantifier                   | glued to its pattern: `(x)*`, `","+`, `(x)*?`  |
| capture                      | one space before `@`: `(x) @name`              |
| capture type                 | spaced `::` both sides: `@x :: T`              |
| predicate                    | spaced: `(identifier == "foo")`, `(x =~ /re/)` |
| negated field                | glued `-`: `-value`                            |
| anchors inline               | spaced like items: `(a) . (b)`, `. (x)`        |
| category refinement          | glued `#`: `(expression#call_expression)`      |
| `MISSING` / `ERROR` argument | one space: `(MISSING ";")`                     |

Suffixes attach in fixed grammar order — quantifier, capture, capture type —
and never migrate to their own line:

```
}* @entries :: Entry
)+? @items
] @stmt :: Stmt
```

## File layout

A file is a list of definitions (plus comments and an optional shebang).

- Every definition starts at column 0; nothing else shares its line
  (`A = (a) B = (b)` is split into two lines).
- **Blank lines between definitions**: none between consecutive single-line
  definitions; exactly one on each side of a multi-line definition. Two or
  more blank lines collapse to the rule above.

  ```
  Name = (identifier) @name
  Value = (number) @value

  Both = (pair
    (property_identifier) @key
    (number) @val
  )
  ```

- No blank lines inside a definition body.
- No leading blank lines in a nonempty file; exactly one trailing newline at
  EOF.
- A shebang stays verbatim on line 1 and is treated as a single-line neighbor
  for the blank-line rule.
- Empty or whitespace-only input formats to `"\n"`. A comment-only file keeps
  its comments and receives exactly one final newline.

## Comments

All three forms — `// line`, `; line`, `/* block */` — are preserved as
written; the formatter never converts between them. Trailing ASCII whitespace
on a line comment and conversion of CRLF or lone CR to LF inside a block comment are the
only comment-text normalizations.

Comment placement has three classes:

- **own-line** — no code precedes the comment on its starting source line;
- **inline** — a one-line block comment has code on both sides on that line;
- **trailing** — code precedes it and no code follows it on that line.

A line comment can only be own-line or trailing. A block comment containing a
newline is never an inline-width fragment.

- An own-line comment stays on its own line, indented to the current nesting
  level. Between definitions it attaches to the definition that follows it
  (blank-line policy treats comment + definition as one unit).
- A trailing comment starts one space after the completed code unit.
- A one-line inline block comment stays in its source-order gap with one space
  on each side: `(call /* c */ (arguments))`. Inside otherwise-empty
  delimiters it keeps one space on each side: `[ /* none */ ]`.
- A comment between atomic parts such as a definition separator, field prefix,
  predicate, or suffix cannot force those tokens into an invalid half-broken
  shape. Keep a one-line inline block comment in that gap; hoist an own-line
  comment before the smallest complete definition/alternative/pattern unit, and
  attach a trailing comment after that unit.
- A line comment inside a group forces a line boundary because nothing can
  follow it on the line.
- A multiline block comment preserves its internal line structure, indentation,
  and tabs, while normalizing CRLF pairs and lone CR characters to LF. Formatter indentation applies
  only before its first line. Any syntax after its closing line begins on a new
  formatted line.

Comments retain source order and must each be emitted exactly once. Canonical
placement may change a comment's CST owner, so idempotence is defined by the
formatted fixed point, not by preserving original trivia ownership.

## Normalizations

Beyond whitespace, the formatter rewrites these equivalent spellings:

| Input                 | Output                  | Why                                                                      |
| --------------------- | ----------------------- | ------------------------------------------------------------------------ |
| `'text'`              | `"text"`                | double quotes are canonical; keep `'` only when the content contains `"` |
| `@x::T`, `@x ::T`     | `@x :: T`               | capture type is always spaced                                            |
| `(identifier=="foo")` | `(identifier == "foo")` | predicates are always spaced                                             |
| `(x) *`               | `(x)*`                  | quantifiers bind tight                                                   |
| `(kind/sub)`          | `(kind#sub)`            | `/` is the deprecated spelling                                           |

String _contents_ (escapes) and regex literals are emitted verbatim —
formatting never re-escapes.

## Rejected styles

Each of these appears somewhere in the corpus; they are minority idioms and
are deliberately not part of the canonical style. The formatter rewrites them.

- **Hugged containers** — `(program [` … `] @x)`, attaching a broken child's
  delimiters to the parent's lines (scattered through `04-emit/types/`).
  Canonical style fans out. One shape for "broken" keeps diffs and the mental
  model trivial.
- **Stacked closers** — `value: (template_string … @frag))))`. They may appear
  in authored snapshots; canonical output puts each broken group's closer on
  its own line.
- **Grid/column alignment** — `labeled_alternation_30_alternative_cascade.txt`
  lays alternatives out
  in an aligned grid. Never produced: one item per line, single spaces.
- **Compact labeled alternations** — `Entry = [Id: (identifier) @id  Num: (number) @num]`
  (note the double space). Labeled alternations always break; multi-space
  separators never survive.
- **Emphasis fan-out** — snapshots often break a construct that fits inline
  because it is the construct under test (`(d\n  (C)\n  (A)\n)` at 11 chars,
  captured wrapper chains in `04-emit/bytecode/captures/`). A formatter
  cannot see emphasis; authored breaks are not preserved. This is the main
  source of corpus divergence and is accepted.

## Corpus rollout

The structural rules intentionally disagree with some authored snapshot layout:
hugged containers, stacked closers, compact labeled alternations, emphasis-only fan-out, and
capture-dense lines are all reflowed. Once the formatter exists, its dry-run
report is the source of truth for rollout size. Do not carry old agreement
percentages forward without a reproducible corpus test tied to the current
rules and snapshot revision.

## Algorithm sketch

For the implementation. `width = 80` Unicode scalars, `indent = 2`. The
production path performs one parse, one normalization/analysis pass, and one
emission pass. Reformatting the result is a test invariant, not a convergence
mechanism in the public API.

```text
fmt_pattern(pattern, col, level, pending_affixes):
    model = normalize(pattern)              # typed layout IR + lossless token handles
    inline = inline_summary(model, pending_affixes)

    must_break =
        construct_threshold(model)
        or inline.landmark_count > 3
        or child_is_broken(model)
        or boundary_comment_requires_lines(model)

    if not must_break and inline.width + col <= width:
        emit(inline)
        return

    if not model.has_breakable_units:
        emit(inline)                        # unavoidable overflow
        return

    emit(model.head)
    for item in model.layout_items:
        newline(level + 1)
        render_item(item)                   # item owns field/label prefix
    newline(level)
    emit(model.closer, pending_affixes)
```

Comment classification and attachment happen before measurement, including
newlines inside block-comment tokens. One ordered CST-token traversal records
the first and last code token on each source line and classifies every comment
from those line facts; there is no per-comment source or token scan.
Structural facts are compositional and bottom-up; summaries store width,
landmark count, hardline state, and boundary token roles rather than flattened
subtree strings. Normalization records definition bodies, group items, comment
boundaries, closers, and prefix/suffix fragments explicitly, so rendering does
not rediscover structure or compare node identity. Width is decided at emission
because the starting column and structured pending affix chain are contextual.

Emission writes directly into one source-sized `String`; no document or line
tree is built and copied afterward. Deterministic work accounting covers token
classification, normalization, measurement, layout traversal, atom traversal,
and output bytes. Geometric tests enforce both a relative scaling bound and an
absolute input-plus-output work budget for comment-heavy, broad-group, and
deep-prefix queries.

Idempotence is not assumed from CST identity: comment relocation and explicit
token normalizations may change trivia ownership or token spelling. It is a
required fixed-point property, verified by formatting every successful output
again.
