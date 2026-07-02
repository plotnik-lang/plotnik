# Ethos

- `AGENTS.md` is our constitution. When you notice systematic gaps (repeated retries, patterns discovered through trial-and-error), propose amendments at the end of the session.
- Resilient parser with user-friendly error messages called "diagnostics" (see `diagnostics/`)
- Stability via invariants: `panic!`/`assert!`/`.expect()` for simple cases, `invariants.rs` otherwise

# Invariant Discipline

One rule, two sides of a trust boundary — where untrusted input becomes trusted state (the compiler admitting a query, the loader admitting bytecode):

- **Outside** (query source, external bytecode): never panic — answer with a diagnostic or a `Result` error.
- **Inside** (after a successful parse or load): trust completely; state invariants loudly (`panic!`/`.expect()`/`invariants.rs`). A violation is _our_ bug.

Validate once, at the boundary. Two smells to fix on sight: a swallowed must-hold fact (`unwrap_or`, `.ok()`) → make it loud; a `panic!` on untrusted input → make it a clean error.

# Documentation

[docs/README.md](docs/README.md) | [CLI Guide](docs/cli.md) | [Language Reference](docs/lang-reference.md) | [Type System](docs/type-system.md) | [Runtime Engine](docs/runtime-engine.md) | [Tree Navigation](docs/tree-navigation.md) | [Binary Format](docs/binary-format/01-overview.md)

# Query Syntax Quick Reference

**Pattern.** A *pattern* is a query matcher over the target syntax tree. Patterns nest — every pattern is built from sub-patterns — so the query AST is a tree of patterns (`Pattern`/`PatternKind`), mirroring rustc's `Pat`/`PatKind`. A node pattern `(kind)` matches a named node; a token pattern `"text"` or `_` matches an anonymous token (or any node); sequences, alternations, quantifiers, fields, and captures are all patterns.

## Core Constructs

| Syntax              | Meaning                            |
| ------------------- | ---------------------------------- |
| `(node_kind)`       | Named node                         |
| `"text"` / `'text'` | Anonymous node (literal token)     |
| `(_)`               | Any named node                     |
| `_`                 | Any node                           |
| `@name`             | Capture (snake_case only)          |
| `@x :: T`           | Type annotation (T is PascalCase)  |
| `field: pattern`    | Field constraint                   |
| `-field`            | Negated field (assert absent)      |
| `?` `*` `+`         | Quantifiers (0-1, 0+, 1+)          |
| `??` `*?` `+?`      | Non-greedy variants                |
| `.`                 | Soft anchor (skips trivia)         |
| `.!`                | Strict anchor (exact adjacency)    |
| `{...}`             | Sequence (siblings in order)       |
| `[...]`             | Alternation (first match wins)     |
| `Name = ...`        | Named definition (entrypoint)      |
| `(Name)`            | Use named expression               |
| `(node == "x")`     | String predicate (== != ^= $= \*=) |
| `(node =~ /x/)`     | Regex predicate (=~ !~)            |

## Data Model Rules

- **Output model**: output exists exactly where output syntax is written — `@capture` (field), def name (type), branch label (variant, when consumed), `:: Name` (type name). No captures — no data, like regex.
- Captures are flat by default: nesting in pattern ≠ nesting in output
- `{...} @x` or `[...] @x` creates a nested scope
- **References are opaque**: bare `(Foo)` matches structurally, no output; `(Foo) @x` → `x: Foo` (void def → `x: Node`); `(Foo)* @xs` → `xs: Foo[]`. Fields never leak through a ref boundary; there is no pure aliasing (`Foo = (Bar)` is void)
- Scalar list (no internal captures): `(x)* @a` → `a: T[]`
- Row list (with internal captures): `(x @y)* @rows` → `rows: { y: T }[]`
- **Strict dimensionality**: `*`/`+` with internal captures requires row capture on the quantifier
- **Single referent**: a capture on a void pattern that doesn't match exactly one node (`{(a) (b)} @x`, `{(a)+} @x`, `{(a)?} @x`, ref to such a def) is an error
- **Type names** are compile-time and path-derived (`{Parent}{PascalField}`); `:: Name` overrides and resets the chain; same name + same shape = one type, different shape = error; `Node` and def names reserved
- `@_` consumes and discards: subtree matches structurally, inner captures inert, no warnings

## Alternations

An alternation `[...]` matches one of several branches:

- **Union** — `[(identifier) @x (number) @y]` → `{ x?: Node, y?: Node }`. Branch captures merge into one struct of optional fields: the fields overlap, with no discriminant for which branch matched. A bare-ref branch is a structural alternative (contributes nothing).
- **Enum** — `[A: (id) @x  B: (num) @y]` → `{ $tag: "A", $data: { x } } | { $tag: "B", $data: { y } }`. Each branch label becomes a discriminant tag, i.e. a Rust-style `enum`.

**Tagging on consumption**: labels take effect when the alternation's value is consumed — captured (`[...] @x`), row-captured (`[...]* @xs`), or a def body (`Expr = [...]`). Unconsumed labeled alternations degrade to a plain union + `UnusedBranchLabels` warning. A captureless branch (incl. bare ref or `@_`) is a tag-only variant (no `$data`); tags-only enums are legitimate.

Mixing enum and union branches in one `[...]` is an error.

Note the output shapes invert the usual TypeScript intuition: an **enum** compiles to a TS *discriminated union* (`A | B`), while a **union** compiles to a TS *struct* (`{ x?, y? }`). Enums render as one multi-line union literal; variant payloads are anonymous (inlined, never named declarations).

## Common Patterns

```
; Match with field
(binary_expression left: (identifier) @left)

; Sequence of siblings
{(comment) (function_declaration) @fn}

; Optional child
(function (decorator)? @dec)

; Recursion
Nested = (call function: [(id) @name (Nested) @inner])
```

## Anchor Strictness

The `.` anchor is soft, `.!` is strict:

| Pattern      | Behavior                                              |
| ------------ | ----------------------------------------------------- |
| `(a) . (b)`  | Skip extras + anonymous nodes; no named nodes between |
| `"x" . (b)`  | Skip extras only; no anonymous/named nodes between    |
| `(a) . "x"`  | Skip extras only; no anonymous/named nodes between    |
| `(a) .! (b)` | Strict, nothing between                               |

Rule: `.!` is exact. Soft `.` skips anonymous nodes only when both sides are named.

**Placement**: Boundary anchors require parent node context:

```
(parent . (first))    ; ✓ valid
(parent (last) .)     ; ✓ valid
{(a) . (b)}           ; ✓ interior anchor OK
{. (a)}               ; ✗ boundary without parent
```

## Anti-patterns

```
; WRONG: groups can't be field values
(x field: {...})

; WRONG: dot capture syntax
@function.name  ; use @function_name

; WRONG: tree-sitter predicate syntax
(id) @x (#eq? @x "foo")  ; use (id == "foo") @x

; WRONG: boundary anchors without parent node
{. (a)}  ; use (parent {. (a)})

; WRONG: anchors directly in alternations
[(a) . (b)]  ; use [{(a) . (b)} (c)]
```

## Type System Rules

**Strict dimensionality**: Quantifiers with internal captures require a row capture on the quantifier:

```
(func (id) @name)*              ; ERROR: no row capture
(func (id) @name)* @funcs       ; OK: funcs: { name: Node }[]
{(a) @a (b) @b}*                ; ERROR: no row capture
{(a) @a (b) @b}* @rows          ; OK: rows: { a: Node, b: Node }[]
(Item)* @items                  ; OK: items: Item[] (refs are opaque)
(Item)*                         ; OK: structural, no output
```

Note: `{}` is for grouping siblings into a sequence, not for satisfying dimensionality.

**Optional rows**: `?` follows the same rule — internal captures need a
capture on the quantifier (`@_` to discard); the collected row is nullable:

```
{(a) @a (b) @b}?    ; ERROR: captures skip together, nothing collects them
{(a) @a (b) @b}? @x ; OK: x: { a: Node, b: Node } | null (one nullable row)
(el (a) @a)? @x     ; OK: x: { a: Node } | null (like (el (a) @a)* @xs rows)
(a)? @x             ; OK: x: Node | null (capture on the quantifier itself)
```

A skip yields `x: null`, never `{ a: null, b: null }` — fields keep their
true modality inside the row.

**Recursion rules** — every cycle must both escape (a non-recursive branch) and consume (descend into a child each pass):

```
Loop = (Loop)                       ; ERROR: no escape path
Expr = [Lit: (n) @n  Rec: (Expr)]   ; OK: Lit escapes (Rec is a tag-only variant)

A = (foo (B))  B = (bar (A))        ; ERROR: descends but no branch terminates
A = [X: (n) @n  Y: (B) @b]  B = (A) ; ERROR: escape exists, but cycle consumes no input
```

## ⚠️ Sequence Syntax (Tree-sitter vs Plotnik)

Tree-sitter: `((a) (b))` — Plotnik: `{(a) (b)}`. The #1 syntax error.

# Project Structure

```
crates/
  plotnik-cli/         # CLI tool and language-feature registry
    src/commands/      # Subcommands (run, check, ast, infer, dump, trace, lang)
  plotnik-lib/         # Published library crate and internal implementation modules
    src/
      bytecode/        # Binary format definitions, instruction set, module loading
      compiler/        # Private compilation pipeline
        core/          # Inter-pass data types: AST, IR, source maps, type shapes
        diagnostics/   # User-friendly error reporting
        parse/         # Syntactic parsing (lexer, grammar, AST wrappers)
        analyze/       # Semantic analysis (names, refs, recursion, shape, types, grammar)
        lower/         # AST/type analysis → bytecode IR lowering passes
        emit/          # Bytecode emission (IR → binary)
        query/         # Query facade (Query, QueryBuilder, SourceMap)
        typegen/       # Type declaration extraction (bytecode → .d.ts)
      core/            # Grammar metadata, node kind database, interning, colors
      vm/              # Runtime VM execution, backtracking, effects, materialization
docs/
  binary-format/       # Bytecode format specification
  lang-reference.md    # Language specification
```

# CLI Reference

Run: `cargo run -p plotnik -- <command>`

| Command       | Purpose                                                    |
| ------------- | ---------------------------------------------------------- |
| `run`         | Execute query, output JSON (`exec` is a hidden alias)      |
| `check`       | Validate query (`--json` for machine-readable diagnostics) |
| `ast`         | Show AST of query and/or source                            |
| `infer`       | Generate TypeScript types                                  |
| `dump`        | Show compiled bytecode                                     |
| `trace`       | Trace execution for debugging                              |
| `lang list`   | List supported languages                                   |
| `lang dump`   | Dump grammar for a language                                |
| `completions` | Generate shell completions                                 |

Exit codes are uniform: `0` yes/success, `1` domain "no" (run: no match;
check: invalid), `2` couldn't answer (usage/IO/internal).

`.ptk` files may declare their language on line 1 via shebang
(`#!/usr/bin/env -S plotnik run -l typescript`); all commands read it, and
explicit `-l` must agree with it. `plotnik query.ptk app.ts` (no subcommand)
routes to `run`.

## ast

Show AST of query and/or source file.

```sh
cargo run -p plotnik -- ast query.ptk               # query AST
cargo run -p plotnik -- ast app.ts                  # source AST (tree-sitter)
cargo run -p plotnik -- ast query.ptk app.ts        # both ASTs
cargo run -p plotnik -- ast query.ptk app.ts --raw  # CST / include anonymous nodes
```

## check

Validate a query (silent on success, like `cargo check`).

```sh
cargo run -p plotnik -- check query.ptk -l typescript
cargo run -p plotnik -- check queries.ts/ -l typescript   # workspace directory
cargo run -p plotnik -- check -q '(identifier) @id' -l javascript
```

## dump

Show compiled bytecode.

```sh
cargo run -p plotnik -- dump query.ptk -l typescript
cargo run -p plotnik -- dump -q '(identifier) @id' -l typescript
```

## infer

Generate TypeScript type definitions from a query.

```sh
cargo run -p plotnik -- infer query.ptk -l javascript
cargo run -p plotnik -- infer queries.ts/ -l typescript -o types.d.ts
cargo run -p plotnik -- infer -q '(identifier) @id' -l typescript
```

Options: `--verbose-nodes`, `--no-node-type`, `--no-export`, `-o <FILE>`

## run

Execute a query against source code and output JSON.

**Usage variants:**

```
run <QUERY> <SOURCE>           # two positional files
run -q <TEXT> <SOURCE>         # inline query + source file
run -q <TEXT> -s <TEXT> -l <LANG>  # all inline
```

```sh
cargo run -p plotnik -- run query.ptk app.ts
cargo run -p plotnik -- run -q 'Q = (identifier) @id' app.ts
cargo run -p plotnik -- run -q 'Q = (identifier) @id' -s 'let x' -l javascript
```

Options: `--compact`, `--entry <NAME>`, `--max-steps <auto|unbounded|N>`,
`--max-memory <auto|unbounded|SIZE>`, `--limits <auto|unbounded>`

## trace

Trace query execution for debugging.

**Usage variants:**

```
trace <QUERY> <SOURCE>           # two positional files
trace -q <TEXT> <SOURCE>         # inline query + source file
trace -q <TEXT> -s <TEXT> -l <LANG>  # all inline
```

```sh
cargo run -p plotnik -- trace query.ptk app.ts
cargo run -p plotnik -- trace -q 'Q = (identifier) @id' app.ts
cargo run -p plotnik -- trace query.ptk app.ts --no-result -vv
```

Options: `-v` (verbose), `-vv` (very verbose), `--no-result`,
`--max-steps <auto|unbounded|N>`, `--max-memory <auto|unbounded|SIZE>`,
`--limits <auto|unbounded>`

## lang

Language information and grammar tools.

```sh
cargo run -p plotnik -- lang list                  # List languages with aliases
cargo run -p plotnik -- lang dump json             # Dump JSON grammar
cargo run -p plotnik -- lang dump typescript       # Dump TypeScript grammar
```

# Coding Rules

- Early exit (`return`, `continue`, `break`) over deep nesting
- Comments for seniors, not juniors
- Rust 2024 `let` chains: `if let Some(x) = a && let Some(y) = b { ... }`
- Never claim "all tests pass" — CI verifies this

## Lifetime Conventions

| Lifetime | Meaning                                     |
| -------- | ------------------------------------------- |
| `'q`     | Query source string (`.ptk` file content)   |
| `'d`     | Diagnostics reference                       |
| `'s`     | Source code string (tree-sitter input)      |
| `'t`     | Parsed tree-sitter tree                     |
| `'a`     | Any other (generic borrows, bytecode views) |

# Testing Rules

Behavioral tests are golden fixtures under `crates/plotnik-lib/tests/0N-stage/`, not
Rust: author a query (+ `==== input ====` source for `06-vm`), then `make shot` fills the
generated sections. Rust `*_tests.rs` are unit-logic only — `foo.rs` → `foo_tests.rs`
(`#[cfg(test)] mod foo_tests;`).

```sh
make test  # Run tests
make shot  # Accept fixtures + insta snapshots
```

- AAA sections separated by blank lines (unless ≤3 lines)
- Single-line input: literal; Multi-line: `indoc!`
- Never write snapshots manually — use `@""` then `cargo insta accept`

Coverage: `make coverage-lines | grep recursion`

`invariants.rs`: `ensure_*()` functions for unreachable code exclusion from coverage.

# PR Body Format

```
## Summary
<1-3 bullets: what this PR does>

## Why
<1-2 sentences: motivation, problem solved, or link to relevant docs>

## Notes (optional)
<Tradeoffs, alternatives considered, gotchas for future reference>
```

- **Summary**: Quick scan; becomes squash-merge commit body
- **Why**: Captures context that code/diff doesn't convey
- **Notes**: Escape hatch for edge cases

**Omit**: How (diff shows this), Testing (CI covers it).
