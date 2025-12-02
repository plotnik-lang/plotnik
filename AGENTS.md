# plotnik

Query language for tree-sitter AST with named subqueries, recursion, and type inference. See [docs/REFERENCE.md](docs/REFERENCE.md) for spec.

Lexer (logos) + parser (rowan) are resilient: collect errors, don't fail-fast.

## CLI

Run: `cargo run -p plotnik-cli -- <command>`

**Update this section when CLI changes.**

| Command        | Purpose                          |
| -------------- | -------------------------------- |
| `debug`        | Inspect queries/sources          |
| `docs [topic]` | Print docs (reference, examples) |
| `langs`        | List supported languages         |

### debug options

Inputs: `--query-text <Q>`, `--query-file <F>`, `--source-text <S>`, `--source-file <F>`, `-l/--lang <L>`

Outputs (composable):
| Flag | Needs |
|------|-------|
| `--query-cst` | query |
| `--query-ast` | query |
| `--query-refs` | query |
| `--query-types` | query |
| `--source-ast` | source |
| `--source-ast-raw` | source |
| `--trace` | both |
| `--result` | both |

Examples:

```sh
cargo run -p plotnik-cli -- debug --query-text '(identifier) @id' --query-ast
cargo run -p plotnik-cli -- debug --source-file f.ts --source-ast
cargo run -p plotnik-cli -- debug --query-text '(fn) @f' --source-file f.ts --result
cargo run -p plotnik-cli -- debug --query-text '(x)' --source-text 'x' --lang typescript
```

## Syntax

Grammar: `(type)`, `[a b]` (alt), `{a b}` (seq), `_` (wildcard), `@name`, `::Type`, `field:`, `*+?`, `"lit"`/`'lit'`, `(a/b)` (supertype), `(ERROR)`, `Name = expr` (def), `[A: ... B: ...]` (tagged alt)

SyntaxKind: `Tree`, `Lit`, `Def`, `Alt`, `Branch`, `Seq`, `Quantifier`, `Capture`, `Type`

Expr = `Tree | Alt | Seq | Quantifier | Capture`. Quantifier/Capture wrap their target.

## Errors

Stages: `Parse` → `Resolve` → `Escape`. Use `Query::errors_in_stage()`, `Query::render_errors_grouped()`.

## Constraints

- Defs must be named except last (entry point)
- Fields: `field: expr` — no sequences as direct values
- Alternations: same-name captures need same type; use `@x :: T` for merged structs; tagged alts for discriminated unions
- `.` anchor = strict adjacency; without = scanning
- Names: `Upper` = user-defined, `lower` = tree-sitter nodes
- Captures: snake_case only, no dots

## Data Model

- Nesting in query ≠ nesting in output: `(a (b @b))` → `{b: Node}`
- New scopes only from captured `{...}@s` or `[...]@c`
- `?`/`*`/`+` = optional/list/non-empty list

## AST Layer (`ql/ast.rs`)

Types: `Root`, `Def`, `Tree`, `Ref`, `Lit`, `Alt`, `Branch`, `Seq`, `Capture`, `Type`, `Quantifier`, `Field`, `NegatedField`, `Wildcard`, `Anchor`, `Expr`

Use `Option<T>` for casts, not `TryFrom`. `format_ast()` for concise output.

## Testing

- Valid parsing: `assert!(query.is_valid())` + snapshot `format_*()` output
- Error recovery: `assert!(!query.is_valid())` + snapshot `render_errors()` only
- Use insta: write empty strings, run `cargo insta review`

## Not implemented

- Semantic validation (phase 5): field constraints, casing rules

## Deferred

- Predicates (`#match?` etc.) — runtime filters, not structural

## Rules

- Update AGENTS.md when changes add useful context
- Check diagnostics after changes
- Follow rnix-parser/taplo patterns
- Span-based tokens, no text in intermediate structures
- No slop comments
- For insta snapshots: write empty strings, run `cargo insta`
