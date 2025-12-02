<p align="center">
  <img width="256" height="256" alt="logo" src="https://github.com/user-attachments/assets/2a93f290-a758-4b19-a38e-3f12b61743d7" />
</p>

<h1 align="center">Plotnik</h1>

<p align="center">
  <i>An advanced query language for <a href="https://tree-sitter.github.io/">tree-sitter</a> AST</i>
</p>

<p align="center">
  <a href="https://github.com/zharinov/plotnik/actions/workflows/test.yml"><img src="https://github.com/zharinov/plotnik/actions/workflows/test.yml/badge.svg" alt="Build Status"></a>
  <a href="LICENSE.md"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT License"></a>
</p>

## Example

Extract all function declarations with their names and bodies:

```
(function_declaration
  name: (identifier) @name
  body: (statement_block) @body) @fn
```

Plotnik infers a typed output structure from your query:

```
{ fn: Node, name: Node, body: Node }
```

## Features

- **Recursion** — match nested structures of arbitrary depth
- **Type inference** — output types derived automatically from query structure
- **Named expressions** — define reusable subqueries with `Name = expr`
- **Partial matching** — queries match subtrees, no need to specify every child
- **Sequences** — `{a b c}` for ordered matches with optional adjacency anchors
- **Alternations** — `[a b]` with merge or tagged union output styles
- **Quantifiers** — `?`, `*`, `+` map to optional and array types
- **Field constraints** — `field: expr` and negated `!field`
- **Supertypes** — `(expression/identifier)` for grammar hierarchies

## Installation

```sh
cargo install plotnik-cli
```

Or build from source:

```sh
git clone https://github.com/zharinov/plotnik.git
cd plotnik
cargo build --release
```

## Usage

```sh
# Parse and inspect a query
plotnik debug --query-text '(function_declaration) @fn' --query-ast

# Parse source file and show AST
plotnik debug --source-file example.ts --source-ast

# Run query against source
plotnik debug --query-text '(identifier) @id' --source-file example.ts --result

# List supported languages
plotnik langs

# Show documentation
plotnik docs reference
```

## Documentation

- [Language Reference](docs/REFERENCE.md) — full syntax and semantics

## License

[MIT](LICENSE.md)