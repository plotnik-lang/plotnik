# Ethos

- `AGENTS.md` is our constitution. Propose useful amendments.
- Resilient parser with user-friendly error messages called "diagnostics" (see `diagnostics/`)
- Stability via invariants: `panic!`/`assert!`/`.expect()` for simple cases, `invariants.rs` otherwise
- AI agents create ADRs when architectural decisions are made

# Documentation

[docs/README.md](docs/README.md) | [Language Reference](docs/lang-reference.md) | [Type System](docs/type-system.md) | [Runtime Engine](docs/runtime-engine.md) | [Binary Format](docs/binary-format/01-overview.md)

# Query Syntax Quick Reference

## Core Constructs

| Syntax              | Meaning                        |
| ------------------- | ------------------------------ |
| `(node_kind)`       | Named node                     |
| `"text"` / `'text'` | Anonymous node (literal token) |
| `(_)`               | Any named node                 |
| `_`                 | Any node                       |
| `@name`             | Capture (snake_case only)      |
| `@x :: T`           | Type annotation                |
| `@x :: string`      | Extract node text              |
| `field: pattern`    | Field constraint               |
| `!field`            | Negated field (assert absent)  |
| `?` `*` `+`         | Quantifiers (0-1, 0+, 1+)      |
| `??` `*?` `+?`      | Non-greedy variants            |
| `.`                 | Anchor (adjacency, see below)  |
| `{...}`             | Sequence (siblings in order)   |
| `[...]`             | Alternation (first match wins) |
| `Name = ...`        | Named definition (entrypoint)  |
| `(Name)`            | Use named expression           |

## Data Model Rules

- Captures are flat by default: nesting in pattern ≠ nesting in output
- `{...} @x` or `[...] @x` creates a nested scope
- Scalar list (no internal captures): `(x)* @a` → `a: T[]`
- Row list (with internal captures): `{(x) @x}* @rows` → `rows: { x: T }[]`
- **Strict dimensionality**: `*`/`+` with internal captures requires row capture

## Alternations

Unlabeled (merge style):

```
[(identifier) @x (number) @y]  → { x?: Node, y?: Node }
```

Labeled (tagged union):

```
[A: (id) @x  B: (num) @y]  → { $tag: "A", $data: { x: ... } } | { $tag: "B", $data: { y: ... } }
```

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

The `.` anchor adapts to what it's anchoring:

| Pattern     | Behavior                                    |
| ----------- | ------------------------------------------- |
| `(a) . (b)` | Skip trivia, no named nodes between         |
| `"x" . (b)` | Strict—nothing between (anonymous involved) |
| `(a) . "x"` | Strict—nothing between (anonymous involved) |

Rule: anchor is as strict as its strictest operand.

## Anti-patterns

```
; WRONG: groups can't be field values
(x field: {...})

; WRONG: dot capture syntax
@function.name  ; use @function_name

; WRONG: predicates (unsupported)
(id) @x (#eq? @x "foo")
```

## Type System Rules

**Strict dimensionality**: Quantifiers with internal captures require explicit row capture:

```
{(a) @a (b) @b}*          ; ERROR: internal captures, no row capture
{(a) @a (b) @b}* @rows    ; OK: rows: { a: Node, b: Node }[]
(func (id) @name)*        ; ERROR: internal capture without row
{(func (id) @name) @f}* @funcs  ; OK: funcs: { f: Node, name: Node }[]
```

**Optional bubbling**: `?` does NOT require row capture (no dimensionality added):

```
{(a) @a (b) @b}?    ; OK: a?: Node, b?: Node (bubbles to parent)
```

**Recursion rules**:

```
Loop = (Loop)                     ; ERROR: no escape path
Expr = [Lit: (n) @n  Rec: (Expr)] ; OK: Lit escapes

A = (B)  B = (A)                  ; ERROR: no input consumed
A = (foo (B))  B = (bar (A))      ; OK: descends each step
```

## ⚠️ Sequence Syntax (Tree-sitter vs Plotnik)

Tree-sitter: `((a) (b))` — Plotnik: `{(a) (b)}`. The #1 syntax error.

# Architecture Decision Records (ADRs)

- **Location**: `docs/adr/`
- **Naming**: `ADR-XXXX-short-title-in-kebab-case.md` (`XXXX` is a sequential number).
- **Index**:
  - _(no ADRs yet)_
- **Template**:

```markdown
# ADR-XXXX: Title

- **Status**: Proposed | Accepted | Deprecated | Superseded by [ADR-YYYY](ADR-YYYY-...)
- **Date**: YYYY-MM-DD

## Context

## Decision

## Consequences

- **Positive** | **Negative** | **Alternatives Considered**
```

# Project Structure

```
crates/
  plotnik-cli/         # CLI tool
    src/commands/      # Subcommands (debug, exec, langs, types)
  plotnik-core/        # Common code
  plotnik-lib/         # Plotnik as library
    src/
      diagnostics/     # Diagnostics (user-friendly errors)
      parser/          # Syntactic parsing of the query
      query/           # Analysis and representation of the parsed query
  plotnik-langs/       # Tree-sitter language bindings (wrapped)
  plotnik-macros/      # Proc macros of the project
docs/
  adr/                 # Architecture Decision Records (ADRs)
  lang-reference.md    # Language specification
```

# CLI Reference

Run: `cargo run -p plotnik-cli -- <command>`

| Command | Purpose                         | Status  |
| ------- | ------------------------------- | ------- |
| `debug` | Inspect queries and source ASTs | Working |
| `types` | Generate TypeScript types       | Working |
| `langs` | List supported languages        | Working |
| `exec`  | Execute query, output JSON      | Not yet |

## debug

Inspect query AST/CST or parse source files with tree-sitter.

```sh
cargo run -p plotnik-cli -- debug -q 'Test = (identifier) @id'
cargo run -p plotnik-cli -- debug -q 'Test = (identifier) @id' --only-symbols
cargo run -p plotnik-cli -- debug -q 'Test = (identifier) @id' --types
cargo run -p plotnik-cli -- debug -s app.ts
cargo run -p plotnik-cli -- debug -s app.ts --raw
```

Options: `--only-symbols`, `--cst`, `--raw`, `--spans`, `--arities`, `--types`

## types

Generate TypeScript type definitions from a query. Requires `-l/--lang` to validate node types against grammar.

```sh
cargo run -p plotnik-cli -- types -q 'Test = (identifier) @id' -l javascript
cargo run -p plotnik-cli -- types --query-file query.ptk -l typescript -o types.d.ts
```

Options: `--root-type <N>`, `--verbose-nodes`, `--no-node-type`, `--no-export`, `-o <F>`

## langs

List supported tree-sitter languages.

```sh
cargo run -p plotnik-cli -- langs
```

# Coding Rules

- Early exit (`return`, `continue`, `break`) over deep nesting
- Comments for seniors, not juniors
- Rust 2024 `let` chains: `if let Some(x) = a && let Some(y) = b { ... }`

# Testing Rules

Code: `foo.rs` → tests: `foo_tests.rs` (include via `#[cfg(test)] mod foo_tests;`)

```sh
make test  # Run tests
make shot  # Accept insta snapshots
```

- AAA sections separated by blank lines (unless ≤3 lines)
- Single-line input: literal; Multi-line: `indoc!`
- Never write snapshots manually — use `@""` then `cargo insta accept`

```rust
#[test]
fn valid_query() {
    let input = indoc! {r#"
      (function_declaration name: (identifier) @name)
    "#};

    let res = Query::expect_valid_ast(input).unwrap();

    insta::assert_snapshot!(res, @"");
}
```

| Test Type      | Pattern                                                      |
| -------------- | ------------------------------------------------------------ |
| Valid parsing  | `assert!(query.is_valid())` + snapshot `dump_*()`            |
| Error recovery | `assert!(!query.is_valid())` + snapshot `dump_diagnostics()` |

Coverage: `make coverage-lines | grep recursion`

`invariants.rs`: `ensure_*()` functions for unreachable code exclusion from coverage.
