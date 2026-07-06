# tree-sitter-plotnik

Tree-sitter grammar for the Plotnik query language (`.ptk` files).

Standalone for now: not wired into the Rust workspace, the CLI, or any
editor distribution. CI generates the parser from `grammar.js` and runs the
corpus, so the grammar stays correct even though `src/` is not committed.

## Development

```sh
tree-sitter generate   # grammar.js -> src/
tree-sitter test       # corpus + highlight assertions
tree-sitter parse q.ptk
```

## Acceptance contract

The source of truth is the reference parser in
`crates/plotnik-lib/src/compiler/parse/`. This grammar mirrors it at the
**syntax level**: anything the reference parser accepts without error-level
diagnostics parses cleanly here, with the deliberate exceptions below.
Later-stage rejections (analyze/link: unknown node kinds, empty `()`/`[]`/
`{}`, supertype refinements, dimensionality, anchor placement semantics)
are out of scope, as usual for editor grammars — files may parse here and
still fail `plotnik check`.

Deliberately rejected despite being warn-accepted (deprecated) upstream:

- `((a) (b))` tree-sitter parenthesized sequences — use `{(a) (b)}`
- `!field` negation — use `-field`

Deliberately rejected despite parsing upstream (they can never compile):

- predicate operator/value mismatches: `(a == /re/)`, `(a =~ "str")`
- anchors/negated fields as definition bodies (upstream parses them and
  rejects the definition at analysis with `NoEntrypoints`)

The reference parser rejects all other misplaced positional assertions
itself — suffixes on anchors/negated fields, anchors/negated fields as
field values or branches, negated fields in sequences — as well as any
children in `(ERROR)` and `(MISSING)`, and this grammar matches those
rejections.

Known micro-divergences on inputs that never compile anyway:

- a shebang after leading comments/blank lines parses (reference: garbage
  unless at offset 0)
- `=~ //` lexes as an empty predicate followed by a comment (reference:
  empty regex, then "empty regex pattern" at analyze)

## Layout

- `grammar.js` — the grammar; `src/` is generated and gitignored
- `test/corpus/` — parse expectations, including an `invalid.txt` suite
  asserting the rejections above
- `test/highlight/` + `queries/highlights.scm` — highlighting smoke tests
