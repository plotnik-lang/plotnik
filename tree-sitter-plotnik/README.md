# tree-sitter-plotnik

Tree-sitter grammar for the Plotnik query language (`.ptk` files).

## Development

Use the same CLI version pinned in `.github/workflows/grammar.yml`:

```sh
cargo install tree-sitter-cli --version 0.26.10 --locked
tree-sitter generate   # grammar.js -> src/
tree-sitter test       # corpus + highlight assertions
tree-sitter parse q.ptk
```

CI generates the parser from `grammar.js` and runs the corpus. Generated files in `src/` are not committed.

## Acceptance contract

Follow the Rust parser in `crates/compiler/src/compiler/parse/` when changing this grammar. Queries it parses without errors should parse here too, with the exceptions below.

The compiler checks names and child patterns against the selected source grammar. It also checks the results built by captures and where anchors can be used.

The Rust parser accepts these forms with a deprecation warning. This grammar rejects them:

- `((a) (b))`: use `{(a) (b)}` for sibling sequences.
- `!field`: use `-field` for negated fields.

These forms parse in Rust but fail compiler checks. This grammar rejects them during parsing:

- Predicates with the wrong value form: `(a == /re/)`, `(a =~ "str")`.
- Definitions whose entire body is an anchor or negated field. The compiler reports `NoEntrypoints` for these.

Both parsers reject:

- Suffixes on anchors or negated fields.
- Anchors or negated fields used as field values or alternatives.
- Negated fields in sibling sequences.
- Children inside `(ERROR)` or `(MISSING)`.

First-line language declarations such as `// @javascript` are ordinary comments in this grammar. The compiler reads and validates their language names during source analysis.

`#`, `//`, and `;` start line comments. `/* ... */` is a block comment. Inside strings and regex literals, comment markers belong to the literal.

`(group/member)` parses as a `named_node` with a `group_selector` token in its `kind` field. Both names use lowercase snake_case, separated by one `/` with no spaces. `(kind)` has an `identifier` in the same field.

The compiler resolves `(kind)` to a concrete kind or a kind group in the selected source grammar. See [Kind groups](../docs/reference.md#kind-groups) for matching rules.

## Layout

- `grammar.js`: parser rules.
- `test/corpus/`: parse expectations, including the rejections in `invalid.txt`.
- `test/highlight/` and `queries/highlights.scm`: highlighting tests.
