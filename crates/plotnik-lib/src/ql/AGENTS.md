# QL Module

Lexer and parser for plotnik's query language.

## Module structure

```
ql/
├── mod.rs              # Public exports
├── lexer.rs            # Logos-based lexer producing Token { kind, span }
├── lexer_tests.rs      # Inline snapshot tests for lexer
├── syntax_kind.rs      # SyntaxKind enum, QLang, TokenSet bitset
└── parser/
    ├── mod.rs          # Re-exports, Parse struct
    ├── core.rs         # Parser struct, trivia handling, checkpoints
    ├── grammar.rs      # Grammar productions
    ├── error.rs        # SyntaxError type
    └── tests/          # Parser snapshot tests
```

## Parser design

Follows patterns from rust-analyzer, rnix-parser, and taplo:

- Zero-copy tokens: `Token { kind: SyntaxKind, span: TextRange }` — text sliced from source only when building the tree
- Rowan's Checkpoint API for wrapping nodes retroactively (e.g., quantifiers around patterns)
- Trivia buffered and drained at `start_node()`, attaching leading trivia to nodes
- Explicit RECOVERY sets per production determine when to break parsing loops

Entry point:
```rust
let result = parse(source);  // -> Parse { green, errors }
let tree = result.syntax();  // -> SyntaxNode (rowan)
```

## Testing

Lexer and parser use `insta` with inline snapshots via custom macros:

- `assert_lex!(input, @"...")` / `assert_lex_raw!(input, @"...")` — lexer
- `assert_parse!(input, @"...")` / `assert_parse_raw!(input, @"...")` — parser
- `*_raw` variants include trivia; regular variants filter it out

Conventions:
- Lexer: flat list of `Kind "text"` pairs
- Parser: tree with indentation, tokens show `Kind "text"`
- Errors appear at end when present
- Run `cargo insta test --accept` to update snapshots