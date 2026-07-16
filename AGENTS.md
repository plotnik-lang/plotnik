# Plotnik

- Tree-sitter-based code query language:
  - bytecode and VM
  - compiler
- Rust 2024

# Coding rules

- Use early exit (`return`, `continue`, `break`) over deep nesting
- Rust 2024 `let` chains: `if let Some(x) = a && let Some(y) = b { ... }`
- Write comments like the reader is new to the codebase but familiar with the goal of the project

Lifetimes:

| Lifetime | Meaning       |
| -------- | ------------- |
| `'q`     | query source  |
| `'d`     | diagnostics   |
| `'s`     | source code   |
| `'t`     | parsed tree   |
| `'a`     | anything else |

## Error handling

Two zones, one rule each:

- Outside (query text, source files, CLI args):
  - never panic
  - report through `Diagnostics` or `Result`
- Inside (everything past validation, including subsystem boundaries):
  - trust completely
  - assert with: `.expect("why")`, `panic!`, `unreachable!`
  - `.unwrap()` is denied (except tests)
  - hedging with `unwrap_or` / `.ok()` is a smell:
    - either assert, or move the check to the validation layer

## Backward compatibility

As Plotnik is pre-release, backward compatibility is irrelevant and must not affect design or implementation. Make breaking changes directly and update all in-tree callers, tests, fixtures, and docs. Keep existing compatibility machinery, but use it only to validate the current format and keep all internal format-version integers at `0`.

Do not add:

- migrations
- legacy paths
- compatibility modes
- deprecation periods
- speculative version wrappers

# Commands

| Command                  | What it does                                          |
| ------------------------ | ----------------------------------------------------- |
| `make check`             | `cargo check --workspace --all-targets`               |
| `make clippy`            | clippy with `-D warnings`                             |
| `make test [FILTER=...]` | full test suite via native Rust test harnesses        |
| `make shot [FILTER=...]` | accept golden fixtures + insta snapshots, then re-run |
| `make fmt`               | `cargo fmt` + prettier                                |
| `make coverage-lines`    | per-file missing lines for `plotnik-lib`              |

Check your changes: `make test`
Before commit: `make fmt`

# Project structure

```
crates/
  plotnik/                     # facade: query! + rt + tree_sitter re-exports
  plotnik-macros/              # proc-macro shell: args, grammar resolution, expansion
  plotnik-cli/
    src/cli/                   # clap defs, dispatch, shebang, limit flags
    src/commands/              # one module per CLI subcommand
    src/language_registry.rs   # define_langs! table, one entry per language
    build.rs                   # embeds gzipped grammar.json per enabled lang feature
  plotnik-lib/
    src/bytecode/              # bytecode format, instruction set, module loading
    src/compiler/
      parse/                   # lexer, grammar, AST
      analyze/                 # semantic analysis passes
      lower/                   # Thompson NFA build + optimization passes
      emit/                    # IR → bytecode module
      query/                   # facade: Query, QueryBuilder, CheckedQuery, CompiledQuery
      typegen/                 # bytecode → TypeScript .d.ts / Rust types
      codegen/                 # fork-point NFA → generated Rust matcher
      diagnostics/             # user-facing error reporting
    src/core/                  # grammar metadata, node-kind database, interner
    src/vm/                    # runtime engine, backtracking, materialization
  plotnik-rt/                  # shared runtime engine (VM + generated matchers)
  plotnik-tests/
    tests/                     # golden fixtures
      mod.rs                   # test harness + docs
      01-lexer/
      02-parser/
      03-analyze/
      04-emit/
      05-typegen/
      06-vm/
      07-codegen/
docs/                          # specs: language, type system, CLI, runtime, binary format
```

Pipeline:

1. Parse: query text to AST
2. Analyze: resolve names, infer types, check recursion
3. Bind: bind node kinds and grammar fields to the selected source-language grammar
4. Lower: build and optimize the Thompson NFA
5. Emit: NFA to binary bytecode module

# Query language

Full spec: `docs/lang-reference.md`, `docs/type-system.md`. The essentials:

| Syntax              | Meaning                                      |
| ------------------- | -------------------------------------------- |
| `(node_kind)`       | Named node                                   |
| `"text"` / `'text'` | Anonymous node (literal token)               |
| `(_)` / `_`         | Any named node / any node                    |
| `@name`             | Capture (snake_case only)                    |
| `@x :: T`           | Capture type (`text`, `bool`, or PascalCase) |
| `field: pattern`    | Grammar-field constraint                     |
| `-field`            | Negated grammar-field constraint             |
| `?` `*` `+`         | Quantifiers (lazy: `??` etc)                 |
| `.` / `.!`          | Soft / exact anchor                          |
| `{...}`             | Sequence (siblings in order)                 |
| `[...]`             | Alternation                                  |
| `Name = ...`        | Definition                                   |
| `(Name)`            | Definition reference                         |
| `(node == "x")`     | String predicate (== != ^= $= \*=)           |
| `(node =~ /x/)`     | Regex predicate (=~ !~)                      |

Rules that trip everyone:

- There is no implicit "search anywhere" (matching starts at the tree root):
  - `Q = (identifier) @id` matches nothing
  - `Q = (program (lexical_declaration (variable_declarator name: (identifier) @id)))` could match.
- Sequences are `{(a) (b)}`, not Tree-sitter's `((a) (b))`.
- Predicates: `(id == "foo") @x`, not Tree-sitter's `(#eq? @x "foo")`
- Capture names are snake_case: `@function_name`, not `@function.name`.
- Result data exists only where result-producing syntax is written:
  - `@capture` becomes a result field
  - `Foo = (...)` declares a result type when the definition produces a value
  - `[Foo: (...) Bar: (...)]` creates `Foo` and `Bar` variant cases when the alternation produces a value
  - `@foo :: Name` supplies an explicit result type name
  - Think regex: no capture — no data
  - Refs are opaque:
    - `(Foo)` match structure only (no data from `Foo`)
    - `(Foo) @x` match + capture `x: Foo` (error if `Foo` is match-only)
- A repeated capture must be collected into a list:
  - a capture under a `*`/`+`/`?` repeats once per match, so a capture on the repeat gathers them (or `@_` discards)
  - No inner captures: `(id)* @ids` produces `ids: Node[]`
  - Inner captures: `(f (id) @name)* @funcs` produces `funcs: { name }[]` (a list of records)
  - Inner captures with nothing collecting them: error
- Alternations (`[...]`) use source-order preference with backtracking:
  - An unlabeled alternation merges captures from its alternatives into a record
    - a result field has option type unless every alternative captures it
    - merging is 1-level deep
    - example: `[(id) @a (num) @b]` produces `{ a: Node | null; b: Node | null }`
  - A labeled alternation produces a variant, represented in JSON with `$tag` and `$data`:
    - example: `[Str: (s) @s Num: (n) @n]` produces `{ $tag: "Str"; $data: { s } } | { $tag: "Num"; $data: { n } }`
    - a no-payload case is tag-only: `{ $tag: "..." }`, no `$data`
  - an alternation cannot mix labeled and unlabeled alternatives

```
// grammar-field constraint
(binary_expression
  left: (identifier) @left
)

// sibling sequence
{
  (comment)
  (function_declaration) @fn
}

// optional
(function (decorator)? @dec)

// list of records: funcs: { name }[]
(func
  (id) @name
)* @funcs

// recursive variant
Expr = [
  Lit: (num) @n
  Rec: (Expr) @e
]
```

# Running queries

`cargo run -p plotnik-cli -- <command>`. Full reference: `docs/cli.md`.

```sh
cargo run -p plotnik-cli -- run query.ptk app.ts
cargo run -p plotnik-cli -- run -q 'Q = (program (expression_statement (identifier) @id))' -s 'x' -l javascript
cargo run -p plotnik-cli -- check query.ptk -l typescript   # silent on success; --json, --strict
cargo run -p plotnik-cli -- infer query.ptk -l typescript   # emit TypeScript types
cargo run -p plotnik-cli -- tree app.ts                     # source syntax tree
cargo run -p plotnik-cli -- trace query.ptk app.ts -vv      # step-by-step execution
cargo run -p plotnik-cli -- lang list                       # languages + aliases
```

- Exit codes:
  - `0`: yes/success
  - `1`: domain "no" (no match or invalid query)
  - `2`: couldn't answer (usage/IO/internal)

# Testing

The golden fixtures have priority over Rust-based tests.

- Run `make shot` to (re)write generated sections
- Use `FILTER=<name>` with `make test` or `make shot` to run the same filtered fixture subset
- Name new fixture folders after existing ones in sibling stages
- Rust `*_tests.rs` are unit-logic only
  - `foo.rs` gets a sibling `foo_tests.rs`, declared as `#[cfg(test)] mod foo_tests;`
  - AAA sections separated by blank lines (unless all 3 are one-liners)
  - single-line input literal, multi-line uses `indoc!`
- Don't generate data for `insta` snapshots and golden fixtures by hand:
  - use `@""` for `insta` placeholders, then `make shot`
  - fill inputs only for golden snapshots
