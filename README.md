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
- [x] Tagged unions (discriminated unions)
- [x] TypeScript type generation
- [x] CLI: `exec` for matches, `infer` for types, `ast`/`trace`/`dump` for debug
- [ ] Grammar verification (validate queries against tree-sitter node types)
- [ ] Compile-time queries via proc-macro
- [ ] LSP server
- [ ] Editor extensions

## Installation

```sh
cargo install plotnik
```

By default, 15 common languages are included. To add specific languages:

```sh
cargo install plotnik --features lang-ruby,lang-elixir
```

Or with all 80+ languages:

```sh
cargo install plotnik --features all-languages
```

## Example

Extract function signatures from Rust. `Type` references itself to handle nested generics like `Option<Vec<String>>`.

`query.ptk`:

```clojure
Type = [
  Simple: [(type_identifier) (primitive_type)] @name :: string
  Generic: (generic_type
    type: (type_identifier) @name :: string
    type_arguments: (type_arguments (Type)* @args))
]

Func = (function_item
  name: (identifier) @name :: string
  parameters: (parameters
    (parameter
      pattern: (identifier) @param :: string
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
export type Type =
  | { $tag: "Simple"; $data: { name: string } }
  | { $tag: "Generic"; $data: { name: string; args: Type[] } };

export interface Func {
  name: string;
  params: { param: string; type: Type }[];
}

export interface Funcs {
  funcs: Func[];
}
```

Run the query against `lib.rs` to extract structured JSON:

```sh
❯ plotnik exec query.ptk lib.rs
{
  "funcs": [
    {
      "name": "get",
      "params": [{
        "param": "key",
        "type": {
          "$tag": "Generic",
          "$data": {
            "name": "Option",
            "args": [{
              "$tag": "Generic",
              "$data": {
                "name": "Vec",
                "args": [{ "$tag": "Simple", "$data": { "name": "String" } }]
              }
            }]
          }
        }
      }]
    },
    {
      "name": "set",
      "params": [
        { "param": "key", "type": { "$tag": "Simple", "$data": { "name": "String" } } },
        { "param": "val", "type": { "$tag": "Simple", "$data": { "name": "i32" } } }
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
