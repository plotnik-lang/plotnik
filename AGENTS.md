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
| `.`                 | Anchor (adjacency)             |
| `{...}`             | Sequence (siblings in order)   |
| `[...]`             | Alternation (first match wins) |
| `Name = ...`        | Named definition (entrypoint)  |
| `(Name)`            | Use named expression           |

## Data Model Rules

- Captures are flat by default: nesting in pattern ≠ nesting in output
- `{...} @x` or `[...] @x` creates a nested scope
- Quantifier on captured pattern → array: `(x)* @a` → `a: T[]`

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

## Anti-patterns

```
; WRONG: groups can't be field values
(x field: {...})

; WRONG: dot capture syntax
@function.name  ; use @function_name

; WRONG: predicates (unsupported)
(id) @x (#eq? @x "foo")
```

## Type System Gotchas

**Columnar output**: Quantifiers produce parallel arrays, not list of objects:

```
{(A) @a (B) @b}*  → { a: Node[], b: Node[] }  // NOT [{a,b}, {a,b}]
```

For list of objects, wrap in sequence: `({(A) @a (B) @b} @row)*`

**Row integrity**: Can't mix `*`/`+` with `1`/`?` in same quantified scope:

```
{(A)* @a (B) @b}*   ; ERROR: @a desync, @b sync
{(A)? @a (B) @b}*   ; OK: both synchronized (? emits null)
```

**Recursion rules**:

```
Loop = (Loop)                     ; ERROR: no escape path
Expr = [Lit: (n) @n  Rec: (Expr)] ; OK: Lit escapes

A = (B)  B = (A)                  ; ERROR: no input consumed
A = (foo (B))  B = (bar (A))      ; OK: descends each step
```

## ⚠️ Sequence Syntax (Tree-sitter vs Plotnik)

Tree-sitter: `((a) (b))` — Plotnik: `{(a) (b)}`. The #1 syntax mistake.

`((a) (b))` in Plotnik means "node `(a)` with child `(b)`", NOT a sequence.

# Architecture Decision Records (ADRs)

- **Location**: `docs/adr/`
- **Naming**: `ADR-XXXX-short-title-in-kebab-case.md` (`XXXX` is a sequential number).
- **Index**:
  - _(no ADRs yet)_
- **Template**:

[ADR-0001](docs/adr/ADR-0001-query-parser.md) | [ADR-0002](docs/adr/ADR-0002-diagnostics-system.md) | [ADR-0004](docs/adr/ADR-0004-query-ir-binary-format.md) | [ADR-0005](docs/adr/ADR-0005-transition-graph-format.md) | [ADR-0006](docs/adr/ADR-0006-dynamic-query-execution.md) | [ADR-0007](docs/adr/ADR-0007-type-metadata-format.md) | [ADR-0008](docs/adr/ADR-0008-tree-navigation.md) | [ADR-0009](docs/adr/ADR-0009-type-system.md) | [ADR-0010](docs/adr/ADR-0010-type-system-v2.md) | [ADR-0012](docs/adr/ADR-0012-variable-length-ir.md)

## Template

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
    src/commands/      # Subcommands (debug, docs, exec, langs, types)
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

| Command | Purpose                         |
| ------- | ------------------------------- |
| `debug` | Inspect queries and source ASTs |
| `exec`  | Execute query, output JSON      |
| `types` | Generate TypeScript types       |
| `langs` | List supported languages        |

Common: `-q/--query <Q>`, `--query-file <F>`, `--source <S>`, `-s/--source-file <F>`, `-l/--lang <L>`

`debug`: `--only-symbols`, `--cst`, `--raw`, `--spans`, `--arities`, `--graph`, `--graph-raw`, `--types`
`exec`: `--pretty`, `--verbose-nodes`, `--check`, `--entry <NAME>`
`types`: `--format <F>`, `--root-type <N>`, `--verbose-nodes`, `--no-node-type`, `--no-export`, `-o <F>`

```sh
cargo run -p plotnik-cli -- debug -q '(identifier) @id' --graph -l javascript
cargo run -p plotnik-cli -- exec -q '(identifier) @id' -s app.js --pretty
cargo run -p plotnik-cli -- types -q '(identifier) @id' -l javascript -o types.d.ts
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

    let query = Query::try_from(input).unwrap();

    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @"");
}
```

| Test Type      | Pattern                                                      |
| -------------- | ------------------------------------------------------------ |
| Valid parsing  | `assert!(query.is_valid())` + snapshot `dump_*()`            |
| Error recovery | `assert!(!query.is_valid())` + snapshot `dump_diagnostics()` |

Coverage: `make coverage-lines | grep recursion`

`invariants.rs`: `ensure_*()` functions for unreachable code exclusion from coverage.
