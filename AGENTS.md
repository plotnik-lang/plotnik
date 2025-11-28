# Project context

- This is `plotnik`: a query language and toolkit for tree-sitter AST
  - Query language (QL) is similar to `tree-sitter` queries, but more powerful
    - named subqueries (expressions)
    - recursion
    - structured data capture with type inference
  - Types of data are inferred from the structure of query
    - could be output in several formats: Rust, TypeScript, Python, etc
    - Rust could use type information to compile queries via procedural macros
    - TypeScript/Python/etc bindings could use type information to avoid the manual data shape checks
- The goal of QL lexer (using `logos`) and parser (using `rowan`) is to be resilient:
  - Do not fail-fast
  - Provide necessary context which could be used by CLI and LSP tooling being built

## Module structure

```
crates/plotnik-lib/src/ql/
├── mod.rs           # Public exports
├── lexer.rs         # Logos-based lexer producing Token { kind, span }
├── lexer_tests.rs   # Inline snapshot tests for lexer
├── syntax_kind.rs   # SyntaxKind enum, QLang, TokenSet bitset
├── parser.rs        # Resilient LL parser using Rowan
└── parser_tests.rs  # Inline snapshot tests for parser
```

## Parser design

The parser follows patterns from rust-analyzer, rnix-parser, and taplo:

- **Zero-copy tokens**: `Token { kind: SyntaxKind, span: TextRange }` - text is sliced from source only when building the tree
- **Rowan's Checkpoint API**: Used for wrapping nodes retroactively (e.g., quantifiers around patterns)
- **Trivia buffering**: Whitespace/comments are buffered and drained at `start_node()`, attaching leading trivia to nodes
- **Error collection**: `SyntaxError { range, message }` stored in parser, returned in `Parse` result
- **Recursion depth limit**: `MAX_DEPTH = 512` prevents stack overflow on malicious input
- **Fuel mechanism** (debug-only): Decremented on lookahead, replenished on progress - panics if parser makes no progress for 256 iterations
- **TokenSet bitset**: `u64`-based O(1) membership testing for FIRST/FOLLOW/RECOVERY sets
- **Explicit RECOVERY sets**: Per-production recovery sets (e.g., `NAMED_NODE_RECOVERY`, `ALTERNATION_RECOVERY`) determine when to break out of parsing loops and let parent handle recovery

Key entry point:

```rust
let result = parse(source);  // -> Parse { green, errors }
let tree = result.syntax();  // -> SyntaxNode (rowan)
```

## What's implemented

- Lexer: all token types including trivia, error coalescing
- Parser structure: trivia handling, error recovery, checkpoints
- Basic grammar: named nodes `(type)`, alternation `[a b]`, wildcards `_`, captures `@name`, fields `field:`, quantifiers `*+?`, anonymous nodes `"literal"`

## What's NOT yet implemented

- Predicates: `#match?`, `#eq?`, etc.
- Named expressions / subqueries (the "extended" part of the QL)
- AST layer: typed wrappers over `SyntaxNode` (like `struct NamedNode(SyntaxNode)`)
- Full grammar validation (some patterns may parse but be semantically invalid)

## Testing

Both lexer and parser use `insta` with inline snapshots, wrapped in custom declarative macros:

- `assert_lex!(input, @"...")` / `assert_lex_raw!(input, @"...")` - lexer tests
- `assert_parse!(input, @"...")` / `assert_parse_raw!(input, @"...")` - parser tests
- `*_raw` variants include trivia (whitespace, comments, newlines); regular variants filter it out

### Lexer tests (`lexer_tests.rs`)

```rust
#[test]
fn capture_simple() {
    assert_lex!("@name", @r#"
    At "@"
    CaptureName "name"
    "#);
}

#[test]
fn trivia_between_tokens() {
    assert_lex_raw!("foo  bar", @r#"
    LowerIdent "foo"
    Whitespace "  "
    LowerIdent "bar"
    "#);
}
```

### Parser tests (`parser_tests.rs`)

```rust
#[test]
fn capture() {
    assert_parse!("(identifier) @name", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Capture
        At "@"
        CaptureName "name"
    "#);
}

#[test]
fn trivia_whitespace_preserved() {
    assert_parse_raw!("(identifier)  @name", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Whitespace "  "
      Capture
        At "@"
        CaptureName "name"
    "#);
}
```

- Input is visible in macro call
- Lexer tests: flat list of `Kind "text"` pairs
- Parser tests: tree structure with indentation, tokens show `Kind "text"`
- Errors appear at the end when present
- Run `cargo insta test --accept` to update snapshots after changes
- Use `snapshot()` for grammar tests; use `snapshot_raw()` for trivia attachment tests

## General rules

- When the changes are made, propose an update to AGENTS.md file if it provides valuable context for future LLM agent calls
- Check diagnostics after your changes
- Follow established parser patterns (see rnix-parser, taplo for reference)
- Keep tokens span-based, avoid storing text in intermediate structures
