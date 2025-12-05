# plotnik

Query language for tree-sitter AST with named subqueries, recursion, and type inference. See [docs/REFERENCE.md](docs/REFERENCE.md) for spec.

Lexer (logos) + parser (rowan) are resilient: collect errors, don't fail-fast.

## Project Structure

```
crates/
  plotnik-lib/         # Core library
    src/
      diagnostics/     # Diagnostic infrastructure
        mod.rs         # Diagnostics struct, DiagnosticBuilder
        message.rs     # DiagnosticMessage, Severity, Fix
        printer.rs     # DiagnosticsPrinter for rendering
      parser/          # Syntax infrastructure
        lexer.rs       # Token definitions (logos)
        cst.rs         # SyntaxKind enum
        ast.rs         # Typed AST wrappers over CST
        core.rs        # Parser infrastructure
        grammar.rs     # Grammar rules
        invariants.rs  # Parser invariant checks
        mod.rs         # Re-exports, Parse struct, parse()
        tests/         # Parser tests (snapshots)
        *_tests.rs     # Test files (lexer_tests, ast_tests, cst_tests)
      query/           # Query processing
        mod.rs         # Query struct, new(), pipeline
        dump.rs        # dump_* debug output methods (test-only)
        printer.rs     # QueryPrinter for AST output
        invariants.rs  # Query invariant checks
        alt_kind.rs    # Alternation validation
        named_defs.rs  # Name resolution, symbol table
        ref_cycles.rs  # Escape analysis (recursion validation)
        shape_cardinalities.rs  # Shape inference
        *_tests.rs     # Test files per module
      lib.rs           # Re-exports Query, Diagnostics, Error
  plotnik-cli/         # CLI tool
    src/commands/      # Subcommands (debug, docs, langs)
  plotnik-langs/       # Tree-sitter language bindings
docs/
  REFERENCE.md         # Language specification
```

## Pipeline

```rust
parser::parse()                   // Parse → CST
alt_kind::validate()              // Validate alternation kinds
named_defs::resolve()             // Resolve names → SymbolTable
ref_cycles::validate()            // Validate recursion termination
shape_cardinalities::analyze()    // Infer and validate shape cardinalities
```

Module = "what", function = "action".

## CLI

Run: `cargo run -p plotnik-cli -- <command>`

- `debug` — Inspect queries/sources
- `docs [topic]` — Print docs (reference, examples)
- `langs` — List supported languages

### debug options

Inputs: `-q/--query <Q>`, `--query-file <F>`, `--source <S>`, `-s/--source-file <F>`, `-l/--lang <L>`

Output: `--query`, `--source`, `--only-symbols`, `--cst`, `--raw`, `--spans`, `--cardinalities`

```sh
cargo run -p plotnik-cli -- debug -q '(identifier) @id' --query
cargo run -p plotnik-cli -- debug -q '(identifier) @id' --only-symbols
cargo run -p plotnik-cli -- debug -s app.ts --source
cargo run -p plotnik-cli -- debug -s app.ts --source --raw
cargo run -p plotnik-cli -- debug -q '(function_declaration) @fn' -s app.ts -l typescript --query
```

## Syntax

Grammar: `(type)`, `[a b]` (alt), `{a b}` (seq), `_` (wildcard), `@name`, `::Type`, `field:`, `*+?`, `"lit"`/`'lit'`, `(a/b)` (supertype), `(ERROR)`, `Name = expr` (def), `[A: ... B: ...]` (tagged alt)

SyntaxKind: `Root`, `Tree`, `Ref`, `Str`, `Field`, `Capture`, `Type`, `Quantifier`, `Seq`, `Alt`, `Branch`, `Wildcard`, `Anchor`, `NegatedField`, `Def`

Expr = `Tree | Ref | Str | Alt | Seq | Capture | Quantifier | Field | Wildcard`. Quantifier/Capture wrap their target. `Anchor` and `NegatedField` are predicates (not expressions).

## Diagnostics

`Diagnostics` struct collects errors/warnings across passes. Access per-pass or combined:

```rust
query.parse_diagnostics()      // Parse errors
query.alt_kind_diagnostics()   // Alternation validation
query.resolve_diagnostics()    // Name resolution
query.ref_cycle_diagnostics()  // Recursion validation
query.shape_diagnostics()      // Shape cardinality validation
query.all_diagnostics()        // All combined
query.diagnostics()            // Alias for all_diagnostics()
```

Render: `query.render_diagnostics()` or `query.render_diagnostics_colored(bool)`.

Check validity: `query.is_valid()` returns false if any pass has errors (warnings allowed).

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

## AST Layer (`parser/ast.rs`)

Types: `Root`, `Def`, `Tree`, `Ref`, `Str`, `Alt`, `Branch`, `Seq`, `Capture`, `Type`, `Quantifier`, `Field`, `NegatedField`, `Wildcard`, `Anchor`, `Expr`

Use `Option<T>` for casts, not `TryFrom`. Use `QueryPrinter` from `query/printer.rs` for output.

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

    let query = Query::new(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @""); // <-- empty string, always
}
```

Then run:

```sh
cargo test --workspace
cargo insta accept
```

Never write snapshot content manually. Let insta generate it.

**Test patterns:**

- Valid parsing: `assert!(query.is_valid())` + snapshot `dump_*()` output
- Error recovery: `assert!(!query.is_valid())` + snapshot `dump_diagnostics()` only

## Coverage

Uses `cargo-llvm-cov`, already installed.

Find uncovered lines per file:

```sh
cargo llvm-cov --package plotnik-lib --text --show-missing-lines 2>/dev/null | grep '\.rs: [0-9]\+\(, [0-9]\+\)\*\?'
```

## Invariants

Two-tier resilience strategy:

1. Parser: resilient, collects errors, continues parsing
2. Our code: strict invariants, maximal coverage in tests, panic on violations

Invariant checks live in dedicated modules named `invariants.rs`.
They are excluded from test coverage because they're unreachable.
They usually wrap a specific assert.
It was done due to limitation of inline coverage exclusion in Rust.
But it seems to be useful to extract such invariant check helpers anyways:

- if it just performs assertion and doesn't return value, it starts with `assert_`
- if it returns value, it's name consists of' `ensure_` and some statement about return value
  Find any of such files for more examples.

## Not implemented

- Semantic validation: casing rules

## Deferred

- Predicates (`#match?` etc.) — runtime filters, not structural

## Rules

- Update AGENTS.md when changes add useful context
- Check diagnostics after changes
- Follow rnix-parser/taplo patterns
- Span-based tokens, no text in intermediate structures
- Don't put AI slop comments in the code
- IMPORTANT: Avoid nesting logic, prefer early exit code flow in functions (return) and loops (continue/break)
