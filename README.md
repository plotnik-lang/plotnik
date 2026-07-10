<br/>

<p align="center">
  <img width="400" alt="The logo: a curled wood shaving on a workbench" src="https://github.com/user-attachments/assets/8f1162aa-5769-415d-babe-56b962256747" />
</p>

<h1><p align="center">Plotnik</p></h1>

<br/>

<p align="center">
  A type-safe query language for <a href="https://tree-sitter.github.io">Tree-sitter</a>.<br/>
  Powered by the <a href="https://github.com/bearcove/arborium">arborium</a> grammar collection.
</p>

<br/>

<p align="center">
  <a href="https://github.com/plotnik-lang/plotnik/actions/workflows/stable.yml"><img src="https://github.com/plotnik-lang/plotnik/actions/workflows/stable.yml/badge.svg" alt="stable"></a>
  <a href="https://github.com/plotnik-lang/plotnik/actions/workflows/nightly.yml"><img src="https://github.com/plotnik-lang/plotnik/actions/workflows/nightly.yml/badge.svg" alt="nightly"></a>
  <a href="https://codecov.io/gh/plotnik-lang/plotnik"><img src="https://codecov.io/gh/plotnik-lang/plotnik/graph/badge.svg?token=071HXJIY3E"/></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue.svg" alt="Apache-2.0 License"></a>
</p>

<br/>

<p align="center">
    <sub>
      <strong>
        ⚠️ BETA: NOT FOR PRODUCTION USE ⚠️<br/>
      </strong>
    </sub>
</p>

<br/>

Tree-sitter gives you the syntax tree. Extracting structured data from it still means writing imperative navigation code, null checks, and maintaining type definitions by hand. Plotnik makes extraction declarative: write a pattern, get typed data. The query is the type definition.

## Features

- [x] Static type inference from query structure
- [x] Named expressions for composition and reuse
- [x] Recursion for nested structures
- [x] Enums (discriminated unions)
- [x] TypeScript type generation
- [x] CLI: `exec` for matches, `infer` for types, `ast`/`trace`/`dump` for debug
- [ ] Full validation against grammar (reject queries that can never match)
- [x] Compile-time queries via proc-macro (the `plotnik` crate's `query!`)
- [ ] WASM
- [ ] LSP, editor extensions

## Installation

```sh
cargo install plotnik-cli
```

By default, 15 common languages are included. To add specific languages:

```sh
cargo install plotnik-cli --features lang-ruby,lang-elixir
```

Or with all 80+ languages:

```sh
cargo install plotnik-cli --features all-languages
```

## In Rust: compile-time queries

The `plotnik` crate compiles a query at build time into typed Rust — output
structs and enums with `parse`/`matches` entry points, no bytecode, no
dynamic values:

```toml
[dependencies]
plotnik = "0.4"
tree-sitter-javascript = "0.25"
```

```rust
// `query!` defines types, so invoke it at module scope — not inside a function.
plotnik::query! {
    r#"
    Q = (program (expression_statement (identifier) @id))
    "#,
    grammar = "tree-sitter-javascript",
}

fn main() {
    let source = "x;";
    let mut parser = plotnik::tree_sitter::Parser::new();
    parser.set_language(&tree_sitter_javascript::LANGUAGE.into()).unwrap();
    let tree = parser.parse(source, None).unwrap();

    // Safe entry points run under compiled-in step/memory/depth limits.
    let q = Q::parse(&tree, source)
        .expect("auto limits fit")
        .expect("matches"); // q.id: Node
}
```

There is no built-in language list: `grammar = "..."` names any dependency
that ships a `grammar.json` (`tree-sitter-*`, `arborium-*`, or your own
grammar crate), so the compiled query is pinned to the exact grammar version
your lockfile resolves. Invalid queries fail the build with the compiler's
own diagnostics; `parse` and `matches` run under compiled-in limits for
untrusted inputs.

## Example

Extract function signatures from Rust. `Type` references itself to handle nested generics like `Option<Vec<String>>`.

`query.ptk`:

```clojure
Type = [
  Simple: [(type_identifier) (primitive_type)] @name
  Generic: (generic_type
    type: (type_identifier) @name
    type_arguments: (type_arguments (Type)* @args))
]

Func = (function_item
  name: (identifier) @name
  parameters: (parameters
    (parameter
      pattern: (identifier) @param
      type: (Type) @type
    )* @params))

Funcs = (source_file (Func)* @funcs)
```

`lib.rs`:

```rust
fn get(key: Option<Vec<String>>) {}

fn set(key: String, val: i32) {}
```

Plotnik infers TypeScript types from the query structure. `Type` is recursive: `args: Type[]`.

```sh
❯ plotnik infer query.ptk --lang rust
export interface Node {
  kind: string;
  text: string;
  span: [number, number];
}

export interface TypeSimple {
  $tag: "Simple";
  $data: { name: Node };
}

export interface TypeGeneric {
  $tag: "Generic";
  $data: { args: Type[]; name: Node };
}

export type Type = TypeSimple | TypeGeneric;

export interface FuncParams {
  param: Node;
  type: Type;
}

export interface Func {
  name: Node;
  params: FuncParams[];
}

export interface Funcs {
  funcs: Func[];
}
```

Run the query against `lib.rs` to extract structured JSON:

```sh
❯ plotnik exec query.ptk lib.rs --entry Funcs
{
  "funcs": [
    {
      "name": { "kind": "identifier", "text": "get", "span": [3, 6] },
      "params": [{
        "param": { "kind": "identifier", "text": "key", "span": [7, 10] },
        "type": {
          "$tag": "Generic",
          "$data": {
            "name": { "kind": "type_identifier", "text": "Option", "span": [12, 18] },
            "args": [{
              "$tag": "Generic",
              "$data": {
                "name": { "kind": "type_identifier", "text": "Vec", "span": [19, 22] },
                "args": [{
                  "$tag": "Simple",
                  "$data": { "name": { "kind": "type_identifier", "text": "String", "span": [23, 29] } }
                }]
              }
            }]
          }
        }
      }]
    },
    {
      "name": { "kind": "identifier", "text": "set", "span": [40, 43] },
      "params": [
        {
          "param": { "kind": "identifier", "text": "key", "span": [44, 47] },
          "type": { "$tag": "Simple", "$data": { "name": { "kind": "type_identifier", "text": "String", "span": [49, 55] } } }
        },
        {
          "param": { "kind": "identifier", "text": "val", "span": [57, 60] },
          "type": { "$tag": "Simple", "$data": { "name": { "kind": "primitive_type", "text": "i32", "span": [62, 65] } } }
        }
      ]
    }
  ]
}
```

## Why

Pattern matching over syntax trees is powerful, but tree-sitter queries produce flat capture lists. You still need to assemble the results, handle missing captures, and define types by hand. Plotnik closes this gap: the query describes structure, the engine guarantees it.

## Documentation

- [CLI Guide](docs/cli.md)
- [Language Reference](docs/lang-reference.md)
- [Type System](docs/type-system.md)

## Acknowledgments

[Max Brunsfeld](https://github.com/maxbrunsfeld) created Tree-sitter; [Amaan Qureshi](https://github.com/amaanq) and other contributors maintain the parser ecosystem that makes this project possible.

## License

This project is licensed under the [Apache License (Version 2.0)].

[Apache License (Version 2.0)]: LICENSE
