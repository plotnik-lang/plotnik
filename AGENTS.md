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

## General rules
- When the changes are made, propose an update to AGENTS.md file if it provides valuable context for future LLM agent calls
- Check diagnostics after your changes
