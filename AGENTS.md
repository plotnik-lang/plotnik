# Ethos

- `AGENTS.md` (this file) is our constitution. You're welcome to propose useful amendments.
- We implement resilient parser, provides user-friendly error messages.
- We call error messages "diagnostics" to avoid confusion with other errors (see `diagnostics/` folder).
- We strive to achieve excellent stability by enforcing invariants in the code:
  - `panic!`, `assert!` or `.expect()` for simple cases
  - `invariants.rs` otherwise, to skip the coverage of unreachable code
- We maintain the architecture decision records (ADRs)
  - AI agent is responsible for creating new ADR when such decision was made during agentic coding session

# Architecture Decision Records (ADRs)

- **Location**: `docs/adr/`
- **Naming**: `ADR-XXXX-short-title-in-kebab-case.md` (`XXXX` is a sequential number).
- **Index**:
  - [ADR-0001: Query Parser](docs/adr/ADR-0001-query-parser.md)
  - [ADR-0002: Diagnostics System](docs/adr/ADR-0002-diagnostics-system.md)
  - [ADR-0003: Query Intermediate Representation](docs/adr/ADR-0003-query-intermediate-representation.md)
- **Template**:

  ```markdown
  # ADR-XXXX: Title of the Decision

  - **Status**: Proposed | Accepted | Deprecated | Superseded by [ADR-YYYY](ADR-YYYY-...)
  - **Date**: YYYY-MM-DD

  ## Context

  Describe the issue, problem, or driving force.

  ## Decision

  Clearly state the decision that was made.

  ## Consequences

  - **Positive**: Benefits, alignment with goals.
  - **Negative**: Drawbacks, trade-offs, future challenges.
  - **Considered Alternatives**: Describe rejected options and why.
  ```

# Plotnik Query Language

Plotnik is a strongly-typed, whitespace-delimited pattern matching language for syntax trees (similar to Tree-sitter but stricter).

## Grammar Synopsis

- **Root**: List of definitions (`Def = expr`).
- **Nodes**: `(kind child1 child2)` or `(kind)`.
- **Strings**: `"literal"`, `'literal'`.
- **Wildcards**: `_` (matches any node).
- **Sequences**: `{ expr1 expr2 }`.
- **Alternations**: `[ expr1 expr2 ]` (untagged) OR `[ Label: expr1 Label: expr2 ]` (tagged).
- **References**: `(DefName)` (Must be PascalCase, no children).

## Modifiers & Constraints

| Feature        | Syntax           | Constraint                                             |
| :------------- | :--------------- | :----------------------------------------------------- |
| **Field**      | `name: expr`     | `expr` must match exactly **one** node (no multi-seq). |
| **Negation**   | `!name`          | Asserts field `name` is absent.                        |
| **Capture**    | `expr @name`     | `snake_case`. Suffix.                                  |
| **Type**       | `expr ::Type`    | `PascalCase` or `::string`. Suffix.                    |
| **Quantifier** | `*`, `+`, `?`    | Greedy. Suffix.                                        |
| **Non-Greedy** | `*?`, `+?`, `??` | Suffix.                                                |
| **Anchor**     | `.`              | Immediate child anchor.                                |

## CRITICAL RULES (Strict Enforcement)

1.  **CASING MATTERS**:
    - **Definitions/Refs**: `PascalCase` (e.g., `MethodDecl`, `(MethodDecl)`).
    - **Node Kinds**: `snake_case` (e.g., `(identifier)`).
    - **Fields/Captures**: `snake_case` (e.g., `name:`, `@val`).
    - **Branch Labels**: `PascalCase` (e.g., `[ Ok: (true) Err: (false) ]`).
2.  **NO MIXED ALTS**: Alternations must be ALL labeled or ALL unlabeled.
3.  **REFS HAVE NO CHILDREN**:
    - Does not work: `(MyDef child)`

## Examples

```plotnik
// Definition
Function = (function_definition
    name: (identifier) @name
    parameters: (parameters {
        (identifier)*
    })
    body: (Block)
)

// Reference usage
Block = (block {
    [
        Stmt: (Statement)
        Expr: (Expression)
    ]*
})

// Alternation with labels
Boolean = [
    True: "true"
    False: "false"
]
```

# Plotnik Query Data Model and Type Inference

1.  **Flat Scoping (Golden Rule)**
    - Query nesting doesn't create data nesting
    - `(A (B (C @val)))` → `{ val: Node }`. Intermediate nodes are ignored.
    - **New Scope** is created _only_ by capturing a container: `{...} @name` or `[...] @name`.

2.  **Field Generation**
    - Only explicit `@capture` creates a field.
    - `key: (pattern)` is a structural constraint, **NOT** an extraction. It has nothing to do with tree-sitter fields.

3.  **Cardinality**
    - `(x) @k` → `k: T` (Required)
    - `(x)? @k` → `k: T?` (Optional)
    - `(x)* @k` → `k: T[]` (List)
    - `(x)+ @k` → `k: [T, ...T[]]` (Non-empty List)

4.  **Types**
    - `(some_node) @x` (default) → `Node` (AST reference).
    - `{...} @x` → receives some synthetic name based on the type of parent scope and capture name
      - `Query = { (foo) @foo (bar) @bar (baz) @baz } @qux`:
        - `@foo`, `@bar`, `@baz`: `Node` for
        - `@qux`: `struct QueryQux { foo: Node, bar: Node, baz: Node }`
        - entry point: `struct Query { qux : QueryQux }`
    - `@x :: string` → `string` (extracts source text).
    - `@x :: Type` → `Type` (assigns nominal type to the structure).

5.  **Alternations**
    - Tagged: `[ L1: (a) @x  L2: (b) @y ]`
      → Discriminated Union: `{ tag: "L1", x: Node } | { tag: "L2", y: Node }`.
    - Untagged: `[ (a) @x  (b) @x ]`
      → Merged Struct: `{ x: Node }`. Captures must be type-compatible across branches.
    - Mixed: `[ (a) @x  (b) ]` (invalid) - the diagnostics will be reported, but we infer as for untagged
      → Merged Struct: `{ x: Node }`. Captures must be type-compatible across branches.

# Project Structure

```
crates/
  plotnik-cli/         # CLI tool
    src/commands/      # Subcommands (debug, docs, langs)
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
  REFERENCE.md         # Language specification
```

# CLI

Run: `cargo run -p plotnik-cli -- <command>`

- `debug` — Inspect queries and source file ASTs
  - Example: `cargo run -p plotnik-cli -- debug -q '(foo) @bar'`
- `langs` — List supported languages

Inputs: `-q/--query <Q>`, `--query-file <F>`, `--source <S>`, `-s/--source-file <F>`, `-l/--lang <L>`

Output (inferred from input): `--only-symbols`, `--cst`, `--raw`, `--spans`, `--cardinalities`

```sh
cargo run -p plotnik-cli -- debug -q '(identifier) @id'
cargo run -p plotnik-cli -- debug -q '(identifier) @id' --only-symbols
cargo run -p plotnik-cli -- debug -s app.ts
cargo run -p plotnik-cli -- debug -s app.ts --raw
cargo run -p plotnik-cli -- debug -q '(function_declaration) @fn' -s app.ts -l typescript
```

# Coding rules

- Avoid nesting logic: prefer early exit in functions (return) and loops (continue/break)
- Write code comments for seniors, not for juniors

# Testing rules

## File organization

- Code lives in `foo.rs`, tests live in `foo_tests.rs`
- Test module included via `#[cfg(test)] mod foo_tests;` in parent

## CLI commands

- IMPORTANT: the `debug` is your first tool you should use to test your changes
- Run tests: `make test`
- We use snapshot testing (`insta`) heavily
  - Accept snapshots: `make snapshots`

## Test structure

- Separate AAA (Arrange-Act-Assert) parts by blank lines
  - Exception: when the test is 3 or less lines total
- Desired structure: input is string, output is string (snapshot of something)
- Single-line input: plain string literal
- Multi-line input: `indoc!` macro
- IMPORTANT: never write snapshots manually — always use `@""` and then `cargo insta accept`

```rust
#[test]
fn valid_query() {
    let input = indoc! {r#"
      (function_declaration
        name: (identifier) @name)
    "#};

    let query = Query::try_from(input).unwrap();

    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @"");
}

#[test]
fn simple_case() {
    let query = Query::try_from("(identifier)").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @"");
}

#[test]
fn error_case() {
    let query = Query::try_from("(unclosed").unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @"");
}
```

## Patterns by test type

- Valid parsing: `assert!(query.is_valid())` + snapshot `dump_*()` output
- Error recovery: `assert!(!query.is_valid())` + snapshot `dump_diagnostics()` only
- Lexer tests: use helper functions `snapshot(input)` / `snapshot_raw(input)`

## Coverage

Uses `cargo-llvm-cov` (already installed)

Find uncovered lines per file:

```sh
$ make coverage-lines | grep recursion
crates/plotnik-lib/src/query/recursion.rs: 78, 210, 214, ...
```

### `invariants.rs`

- The goal of this file is to exclude coverage of the unreachable code branches
- It contains functions and `impl` blocks for invariant check functionality
- Each function panics on invariant violation
- The naming convention: `ensure_something(...)`, where something refers the return value
- It doesn't make sense to put the `panic!(...)`, `assert!()` or `.expect()` because they don't cause coverage problems:
  - `panic!()` usually is called in catch-all `match` branches
    - eventually we extract the whole `match` to the `invariants.rs`, for well-established code
  - `assert!()` is coverage-friendly alternative for `if condition { panic!(...) }`
  - `.expect()` is useful for unwrapping `Result`/`Option` values
