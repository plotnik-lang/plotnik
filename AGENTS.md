# plotnik

Query language for tree-sitter AST with named subqueries, recursion, and type inference. See [docs/REFERENCE.md](docs/REFERENCE.md) for spec.

Lexer (logos) + parser (rowan) are resilient: collect errors, don't fail-fast.

## Project Structure

```
crates/
  plotnik-lib/        # Core library
    src/
      lexer.rs        # Token definitions (logos)
      syntax_kind.rs  # SyntaxKind enum
      parser/
        core.rs       # Parser infrastructure
        grammar.rs    # Grammar rules
        error.rs      # Parse errors
        tests/        # Parser tests (snapshots)
      ast.rs          # Typed AST layer over CST
      resolve.rs      # Name resolution, symbol table
      escape.rs       # Escape analysis (recursion validation)
      validate.rs     # Semantic validation
      lib.rs          # Public API (Query type)
  plotnik-cli/        # CLI tool
    src/commands/     # Subcommands (debug, docs, langs)
  plotnik-langs/      # Tree-sitter language bindings
docs/
  REFERENCE.md        # Language specification
```

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

Use `debug` to explore tree-sitter ASTs and test queries interactively:

```sh
# See what tree-sitter nodes exist in a file
cargo run -p plotnik-cli -- debug --source-file example.ts --source-ast

# Raw tree-sitter output (with anonymous nodes)
cargo run -p plotnik-cli -- debug --source-file example.ts --source-ast-raw

# Test a query against source
cargo run -p plotnik-cli -- debug --query-text '(function_declaration) @fn' --source-file example.ts --result

# Debug query parsing
cargo run -p plotnik-cli -- debug --query-text '[(a) (b)]' --query-cst --query-ast

# Inline source for quick tests
cargo run -p plotnik-cli -- debug --query-text '(x)' --source-text 'x' --lang typescript
```

This is the primary way to understand what nodes to match before writing queries.

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

Uses `insta` for snapshot testing. Critical workflow:

1. Use `indoc!` macro for multi-line query input
2. Always write empty string `@""` for new snapshots
3. Run `cargo insta accept` to populate snapshots (or `cargo insta review` to inspect)

```rust
#[test]
fn my_test() {
    let input = indoc! {r#"
    (function_declaration
        name: (identifier) @name)
    "#};

    let query = Query::new(input);
    assert!(query.is_valid());
    insta::assert_snapshot!(query.format_ast(), @""); // <-- empty string, always
}
```

Then run:

```sh
cargo test --workspace
cargo insta accept
```

Never write snapshot content manually. Let insta generate it.

**Test patterns:**

- Valid parsing: `assert!(query.is_valid())` + snapshot `format_*()` output
- Error recovery: `assert!(!query.is_valid())` + snapshot `render_errors()` only

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
