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

# Commands

| Command                  | What it does                                          |
| ------------------------ | ----------------------------------------------------- |
| `make check`             | `cargo check --workspace --all-targets`               |
| `make clippy`            | clippy with `-D warnings`                             |
| `make test [FILTER=...]` | full test suite via `cargo nextest`                   |
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
    src/bytecode/              # internal VM representation and validation
    src/compiler/
      parse/                   # lexer, grammar, AST
      analyze/                 # semantic analysis passes
      lower/                   # Thompson NFA build + optimization passes
      emit/                    # IR → internal VM representation
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
docs/                          # specs: language, type system, CLI, runtime, internal bytecode
```

Pipeline:

1. Parse: query text to AST
2. Analyze: resolve names, infer types, check recursion
3. Link (a grammar): bind node kinds and fields to the target grammar
4. Lower: build and optimize the Thompson NFA
5. Emit: NFA to the VM's internal bytecode representation

# Query language

Full spec: `docs/lang-reference.md`, `docs/type-system.md`. The essentials:

| Syntax              | Meaning                            |
| ------------------- | ---------------------------------- |
| `(node_kind)`       | Named node                         |
| `"text"` / `'text'` | Anonymous node (literal token)     |
| `(_)` / `_`         | Any named node / any node          |
| `@name`             | Capture (snake_case only)          |
| `@x :: T`           | Type annotation (T is PascalCase)  |
| `field: pattern`    | Field constraint                   |
| `-field`            | Negated field (assert absent)      |
| `?` `*` `+`         | Quantifiers (non-greedy: `??` etc) |
| `.` / `.!`          | Soft / strict anchor               |
| `{...}`             | Sequence (siblings in order)       |
| `[...]`             | Union / enum (first match wins)    |
| `Name = ...`        | Named definition (entrypoint)      |
| `(Name)`            | Use named definition               |
| `(node == "x")`     | String predicate (== != ^= $= \*=) |
| `(node =~ /x/)`     | Regex predicate (=~ !~)            |

Rules that trip everyone:

- There is no implicit "search anywhere" (matching starts at the tree root):
  - `Q = (identifier) @id` matches nothing
  - `Q = (program (lexical_declaration (variable_declarator name: (identifier) @id)))` could match.
- Sequences are `{(a) (b)}`, not tree-sitter's `((a) (b))`.
- Predicates: `(id == "foo") @x`, not tree-sitter's `(#eq? @x "foo")`
- Capture names are snake_case: `@function_name`, not `@function.name`.
- Output exists only where output syntax is written:
  - `@capture` becomes a field
  - `Foo = (...)` becomes a type
  - `[Foo: (...) Bar: (...)]` creates `Foo` and `Bar` enum variants
  - `@foo :: Name` helps to specify type name and avoid synthetic name
  - Think regex: no capture — no data
  - Definition root is captured by default when possible ("group 0"): `Foo = (program)` produces `Node`
  - Refs are opaque:
    - `(Foo)` match structure only (no data from `Foo`)
    - `(Foo) @x` match + capture `x: Foo` (error if `Foo` is void)
- Strict dimensionality — a repeated capture must be collected into a list:
  - a capture under a `*`/`+`/`?` repeats once per match, so a capture on the repeat gathers them (or `@_` discards)
  - No inner captures: `(id)* @ids` produces `ids: Node[]` (scalar list)
  - Inner captures: `(f (id) @name)* @funcs` produces `funcs: { name }[]` (row list)
  - Inner captures with nothing collecting them: error
- Unions and enums (`[...]`) — one branch matches, first wins:
  - Union merges captures from each branch into a struct
    - field is nullable unless every branch captures it
    - merging is 1-level deep
    - example: `[(id) @a (num) @b]` produces `{ a: Node | null; b: Node | null }`
  - Enum produces discriminated union with `$tag` and `$data` fields:
    - example: `[Str: (s) @s Num: (n) @n]` produces `{ $tag: "Str"; $data: { s } } | { $tag: "Num"; $data: { n } }`
    - an enum branch with no capture is tag-only: `{ $tag: "..." }`, no `$data`
  - a `[...]` can't be part union, part enum
  - the right framing: enums are unions with extra precision, unions are loose enums

```
// field constraint
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

// rows: funcs: { name }[]
(func
  (id) @name
)* @funcs

// recursive enum
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
cargo run -p plotnik-cli -- ast app.ts                      # tree-sitter AST of source
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
