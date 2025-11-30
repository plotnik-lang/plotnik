# Project context

> See [docs/REFERENCE.md](docs/REFERENCE.md) for the full language specification.

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

## What's implemented

- Lexer: all token types including trivia, error coalescing
- Parser structure: trivia handling, error recovery, checkpoints
- Basic grammar: named nodes `(type)`, alternation `[a b]`, wildcards `_`, captures `@name` (snake_case only) with types `::T`, fields `field:`, quantifiers `*+?` (and non-greedy), anonymous nodes `"literal"`, supertypes `(a/b)`, special nodes `(ERROR)`

## What's NOT yet implemented

- Named expressions / subqueries (the "extended" part of the QL)
- AST layer: typed wrappers over `SyntaxNode` (like `struct NamedNode(SyntaxNode)`)
- Full grammar validation (some patterns may parse but be semantically invalid)

## Intentionally deferred (post-MVP)

- Predicates (`#match?`, `#eq?`, etc.) and directives (`#set!`, etc.) from tree-sitter QL
  - These are runtime filters, not structural patterns
  - Plotnik's value is in named expressions, recursion, and type inference
  - May be added later if there's demand, but not a priority

## Grammar Constraints

- Fields: `field: pattern` constraints are strict. The pattern must be a node, alternation, or quantifier. Sibling sequences `{...}` are not allowed as direct field values.
- Alternations: In unlabeled alternations, captures with the same name must have the same type across all branches where they appear. A capture is required if present in all branches, optional otherwise. When branches mix bare nodes and structures, bare node captures are auto-promoted to single-field structures. Merged structures require explicit type annotation (`@x::TypeName`) for codegen. Use tagged alternations (`[A: ... B: ...]`) for discriminated unions.
- Anchors: The `.` anchor enforces strict adjacency. Without it, sibling matching allows gaps (scanning).
- Naming: Capitalized names (`Expr`) are user-defined (expressions, labels). Lowercase names (`stmt`) are language-defined (tree-sitter nodes).
- Captures: Must use snake_case (`@name`, `@func_body`). Dots are not allowed in capture names. This ensures captures map directly to valid struct field names in generated code (Rust, TypeScript, Python).

## Data Model

- **Flattening**: Node nesting in the query does NOT create nesting in the output. `(a (b @b))` yields `{ b: Node }`.
- **Structure**: New data scopes/nesting are created ONLY by capturing sequences `{...} @seq` or alternations `[...] @choice`.
- **Arrays**: Quantifiers `?`, `*`, `+` determine the cardinality (optional, list, non-empty list).
- **Fields**: Captures `@name` create fields within the current scope.

## General rules

- When the changes are made, propose an update to AGENTS.md file if it provides valuable context for future LLM agent calls
- Check diagnostics after your changes
- Follow established parser patterns (see rnix-parser, taplo for reference)
- Keep tokens span-based, avoid storing text in intermediate structures
- Don't write AI slop code comments, write only useful ones
