# Plotnik CLI Guide

The CLI compiler commands read a Tree-sitter `grammar.json` supplied with `--grammar`. The CLI does not bundle language grammars or infer one from a name or file extension.

## Quick start

```sh
plotnik check query.ptk --grammar path/to/grammar.json
plotnik infer query.ptk --grammar path/to/grammar.json
plotnik gen query.ptk --grammar path/to/grammar.json --target rust
plotnik dump query.ptk --grammar path/to/grammar.json
```

Use the `grammar.json` from the parser package that will parse your source. `gen` records the grammar name, the SHA-256 of the exact input bytes, and the supplied path in its output. Generated matchers verify the node and field IDs they use against the runtime tree.

| Command                                   | Current behavior                                                       |
| ----------------------------------------- | ---------------------------------------------------------------------- |
| `check`                                   | Validate a query against the supplied grammar                          |
| `infer`                                   | Generate TypeScript declarations                                       |
| `gen`                                     | Generate a Rust matcher module                                         |
| `dump`                                    | Show the compiled bytecode as text                                     |
| `completions`                             | Generate shell completions                                             |
| `run`, `trace`, `inspect`, `tree`, `lang` | Retained command and argument definitions, with unimplemented handlers |

`generate` is an alias for `gen`. `exec` is an alias for `run`. A bare `.ptk` invocation still routes to `run`, which is currently unimplemented.

## Query input

The four compiler commands accept a query file, a directory of `.ptk` files, `-` for standard input, or `-q/--query` for inline text. A directory is compiled as one flat namespace. Its immediate `.ptk` files are read in sorted order, and subdirectories are not included.

```sh
plotnik check queries/ --grammar path/to/grammar.json
plotnik check -q 'Q = (program)' --grammar path/to/grammar.json
echo 'Q = (program)' | plotnik check - --grammar path/to/grammar.json
```

A query may still contain a shebang line as query syntax. The compiler commands do not use a shebang to select a language or entry point. Supply `--grammar` even when a query contains one.

If the grammar file cannot be read, decoded, parsed, or validated, the error identifies its supplied path.

## check

`check` parses, analyzes, binds, lowers, and validates the query without selecting an output target. A clean query is silent. Warnings are printed unless `--json` is used. `--strict` treats warnings as errors.

```sh
plotnik check query.ptk --grammar path/to/grammar.json --strict
plotnik check query.ptk --grammar path/to/grammar.json --json
```

With `--json`, exit 0 or 1 writes an array of diagnostics to standard output, including `[]` for a clean query. An input or grammar error exits 2 and writes text to standard error without a JSON array.

## infer

`infer` emits TypeScript declarations. Use `-o/--output` to write a file. Other options are `--format typescript|ts`, `--include-points`, `--no-node-type`, `--no-export`, and `--match-only-type undefined|null`.

```sh
plotnik infer query.ptk --grammar path/to/grammar.json -o types.d.ts
```

## gen

`gen` emits a Rust module with typed results and a compiled matcher. It requires `--target rust` and accepts `-o/--output` and `--debug`.

```sh
plotnik gen query.ptk --grammar path/to/grammar.json --target rust -o query.rs
```

The generated module imports `plotnik_rt`. Depend on `plotnik-rt` for Tree-sitter or `plotnik-rt-arborium` for Arborium. Both expose that crate name and runtime API.

## dump

`dump` prints the transient compiled bytecode for inspection. It does not create a bytecode file.

```sh
plotnik dump -q 'Q = (program)' --grammar path/to/grammar.json
```

## Retained commands

`run`, `trace`, `inspect`, `tree`, and `lang` are currently unimplemented. Their command names, subcommands, and argument definitions remain available in `--help`, including the runtime commands' `--lang`, `--entry`, and limit flags. Calling a handler reaches a literal `unimplemented!()` stub.

## Completions

```sh
plotnik completions bash
plotnik completions zsh
```

## Exit codes

For the functional compiler commands, exit 0 means success, exit 1 means an invalid query or a target emission failure, and exit 2 means the CLI could not answer because of usage, input, or grammar errors.
