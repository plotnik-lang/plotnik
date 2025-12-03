<br/>
<br/>

<div id="user-content-toc" align="center">
  <ul>
  <summary>
    <p align="center">
      <img width="400" alt="Plotnik banner" src=".github/plotnik_banner.png" />
    </p>
    <h1><p>plotnik</p></h1>  
  </summary>
  </ul>
</div>

<p align="center">
  <i>Typed query language for <a href="https://tree-sitter.github.io/">tree-sitter</a></i>
</p>

<br/>
<br/>

<p align="center">
  <a href="https://github.com/plotnik-lang/plotnik/actions/workflows/stable.yml"><img src="https://github.com/plotnik-lang/plotnik/actions/workflows/stable.yml/badge.svg" alt="stable"></a>
  <a href="https://github.com/plotnik-lang/plotnik/actions/workflows/nightly.yml"><img src="https://github.com/plotnik-lang/plotnik/actions/workflows/nightly.yml/badge.svg" alt="nightly"></a>
  <a href="https://codecov.io/gh/plotnik-lang/plotnik"><img src="https://codecov.io/gh/plotnik-lang/plotnik/graph/badge.svg?token=071HXJIY3E"/></a>
  <a href="LICENSE.md"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT License"></a>
</p>

<br/>
<hr/>
<br/>

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
git clone https://github.com/plotnik-lang/plotnik.git
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

## Roadmap

**Ignition** _(the parser)_

- [x] Resuilient query language parser
- [x] Basic error messages
- [x] Name resolution
- [x] Recursion validator
- [ ] Semantic analyzer

**Liftoff** _(type inference)_

- [ ] Basic validation against `node-types.json` schemas
- [ ] Type inference of the query result shape

**Acceleration** _(query engine)_

- [ ] Thompson construction of query IR
- [ ] Runtime execution engine
- [ ] Advanced validation powered by `grammar.json` files

**Orbit** _(the tooling)_

- [ ] The CLI app available via installers
- [ ] Compiled queries (using procedural macros)
- [ ] Enhanced error messages
- [ ] Bindings (TypeScript, Python, Ruby)
- [ ] LSP server
- [ ] Editor support (VSCode, Zed, Neovim)

## License

[MIT](LICENSE.md)
