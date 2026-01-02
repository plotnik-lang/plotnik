# Ethos

- `AGENTS.md` is our constitution. Propose useful amendments.
- Resilient parser with user-friendly error messages called "diagnostics" (see `diagnostics/`)
- Stability via invariants: `panic!`/`assert!`/`.expect()` for simple cases, `invariants.rs` otherwise
- AI agents create ADRs when architectural decisions are made

# Documentation

[docs/README.md](docs/README.md) | [CLI Guide](docs/cli.md) | [Language Reference](docs/lang-reference.md) | [Type System](docs/type-system.md) | [Runtime Engine](docs/runtime-engine.md) | [Binary Format](docs/binary-format/01-overview.md)

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

; WRONG: predicates (unsupported)
(id) @x (#eq? @x "foo")

; WRONG: boundary anchors without parent node
{. (a)}  ; use (parent {. (a)})

; WRONG: anchors directly in alternations
[(a) . (b)]  ; use [{(a) . (b)} (c)]
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
    src/commands/      # Subcommands (check, dump, infer, tree, langs)
  plotnik-core/        # Common code (Interner, Symbol)
  plotnik-lib/         # Plotnik as library
    src/
      analyze/         # Semantic analysis (symbol_table, dependencies, type_check, validation)
      bytecode/        # Binary format definitions
      typegen/         # Type declaration extraction (bytecode → .d.ts)
      compile/         # Thompson NFA construction (AST → IR)
      diagnostics/     # User-friendly error reporting
      emit/            # Bytecode emission (IR → binary)
      parser/          # Syntactic parsing (lexer, grammar, AST)
      query/           # Query facade (Query, QueryBuilder, SourceMap)
      type_system/     # Shared type primitives
  plotnik-langs/       # Tree-sitter language bindings
  plotnik-macros/      # Proc macros
docs/
  adr/                 # Architecture Decision Records
  binary-format/       # Bytecode format specification
  lang-reference.md    # Language specification
```

# CLI Reference

Run: `cargo run -p plotnik-cli -- <command>`

| Command | Purpose                    | Status  |
| ------- | -------------------------- | ------- |
| `tree`  | Explore tree-sitter AST    | Working |
| `check` | Validate query             | Working |
| `dump`  | Show compiled bytecode     | Working |
| `infer` | Generate TypeScript types  | Working |
| `langs` | List supported languages   | Working |
| `exec`  | Execute query, output JSON | Not yet |

## tree

Explore a source file's tree-sitter AST.

```sh
cargo run -p plotnik-cli -- tree app.ts
cargo run -p plotnik-cli -- tree app.ts --raw
cargo run -p plotnik-cli -- tree app.ts --spans
```

## check

Validate a query (silent on success, like `cargo check`).

```sh
cargo run -p plotnik-cli -- check query.ptk -l typescript
cargo run -p plotnik-cli -- check queries.ts/              # workspace with lang inference
cargo run -p plotnik-cli -- check -q '(identifier) @id' -l javascript
```

## dump

Show compiled bytecode.

```sh
cargo run -p plotnik-cli -- dump query.ptk                 # unlinked
cargo run -p plotnik-cli -- dump query.ptk -l typescript   # linked
cargo run -p plotnik-cli -- dump -q '(identifier) @id'
```

## infer

Generate TypeScript type definitions from a query.

```sh
cargo run -p plotnik-cli -- infer query.ptk -l javascript
cargo run -p plotnik-cli -- infer queries.ts/ -o types.d.ts
cargo run -p plotnik-cli -- infer -q '(identifier) @id' -l typescript
```

Options: `--verbose-nodes`, `--no-node-type`, `--no-export`, `-o <FILE>`

## langs

List supported tree-sitter languages.

```sh
cargo run -p plotnik-cli -- langs
```

# Coding Rules

- Early exit (`return`, `continue`, `break`) over deep nesting
- Comments for seniors, not juniors
- Rust 2024 `let` chains: `if let Some(x) = a && let Some(y) = b { ... }`

## Lifetime Conventions

| Lifetime | Meaning                                          |
| -------- | ------------------------------------------------ |
| `'q`     | Query source string (`.ptk` file content)        |
| `'d`     | Diagnostics reference                            |
| `'s`     | Source code string (tree-sitter input)           |
| `'t`     | Parsed tree-sitter tree                          |
| `'a`     | Any other (generic borrows, bytecode views)      |

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
