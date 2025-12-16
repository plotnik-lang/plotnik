<br/>
<br/>

<p align="center">
  <img width="400" alt="The logo: a curled wood shaving on a workbench" src="https://github.com/user-attachments/assets/1fcef0a9-20f8-4500-960b-f31db3e9fd94" />
</p>

<h1><p align="center">Plotnik</p></h1>

<br/>

<p align="center">
  Plotnik is a query language for source code.<br/>
  Queries extract relevant structured data.<br/>
  Transactions allow granular, atomic edits.
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
  ⚠️ <a href="#roadmap">ALPHA STAGE</a>: not for production use ⚠️<br/>
</p>

<br/>
<br/>

## The problem

Tree-sitter solved parsing. It powers syntax highlighting and code navigation at GitHub, drives the editing experience in Zed, Helix, and Neovim. It gives you a fast, accurate, incremental syntax tree for virtually any language.

The hard problem now is what comes _after_ parsing: extraction of meaning from the tree, and safe transformation back to source:

```typescript
function extractFunction(node: SyntaxNode): FunctionInfo | null {
  if (node.type !== "function_declaration") {
    return null;
  }
  const name = node.childForFieldName("name");
  const body = node.childForFieldName("body");
  if (!name || !body) {
    return null;
  }
  return {
    name: name.text,
    body,
  };
}
```

Every extraction requires a new function, each one a potential source of bugs that won't surface until production. And once you've extracted what you need, applying changes back to the source requires careful span tracking, validation, and error handling—another layer of brittle code.

## The solution

Plotnik extends Tree-sitter queries with type annotations:

```clojure
(function_declaration
  name: (identifier) @name :: string
  body: (statement_block) @body
) @func :: FunctionInfo
```

The query describes structure, and Plotnik infers the output type:

```typescript
interface FunctionInfo {
  name: string;
  body: SyntaxNode;
}
```

This structure is guaranteed by the query engine. No defensive programming needed.

Once extraction is complete, Plotnik will support **transactions** to apply validated changes back to the source. The same typed nodes used for extraction become targets for transformation—completing the loop from source to structured data and back to source.

## But what about Tree-sitter queries?

Tree-sitter already has queries:

```clojure
(function_declaration
  name: (identifier) @name
  body: (statement_block) @body)
```

The result is a flat capture list:

```typescript
query.matches(tree.rootNode);
// → [{ captures: [{ name: "name", node }, { name: "body", node }] }, ...]
```

The assembly layer is up to you:

```typescript
const name = match.captures.find((c) => c.name === "name")?.node;
const body = match.captures.find((c) => c.name === "body")?.node;
if (!name || !body) throw new Error("Missing capture");
return { name: name.text, body };
```

This means string-based lookup, null checks, and manual type definitions kept in sync by convention.

Tree-sitter queries are designed for matching. Plotnik adds the typing layer: the query _is_ the type definition.

## Why Plotnik?

| Hand-written extraction    | Plotnik                      |
| -------------------------- | ---------------------------- |
| Manual navigation          | Declarative pattern matching |
| Runtime type errors        | Compile-time type inference  |
| Repetitive extraction code | Single-query extraction      |
| Ad-hoc data structures     | Generated structs/interfaces |

Plotnik extends Tree-sitter's query syntax with:

- **Named expressions** for composition and reuse
- **Recursion** for arbitrarily nested structures
- **Type annotations** for precise output shapes
- **Alternations**: untagged for simplicity, tagged for precision (discriminated unions)

## Use cases

- **Scripting:** Count patterns, extract metrics, audit dependencies
- **Custom linters:** Encode your business rules and architecture constraints
- **LLM Pipelines:** Extract signatures and types as structured data for RAG
- **Code Intelligence:** Outline views, navigation, symbol extraction across grammars

## Language design

Plotnik builds on Tree-sitter's query syntax, extending it with the features needed for typed extraction:

```clojure
Statement = [
  Assign: (assignment_expression
    left: (identifier) @target :: string
    right: (Expression) @value)
  Call: (call_expression
    function: (identifier) @func :: string
    arguments: (arguments (Expression)* @args))
]

Expression = [
  Ident: (identifier) @name :: string
  Num: (number) @value :: string
]

TopDefinitions = (program (Statement)+ @statements)
```

This produces:

```typescript
type Statement =
  | { $tag: "Assign"; $data: { target: string; value: Expression } }
  | { $tag: "Call"; $data: { func: string; args: Expression[] } };

type Expression =
  | { $tag: "Ident"; $data: { name: string } }
  | { $tag: "Num"; $data: { value: string } };

type TopDefinitions = {
  statements: [Statement, ...Statement[]];
};
```

Then process the results:

```typescript
for (const stmt of result.statements) {
  switch (stmt.$tag) {
    case "Assign":
      console.log(`Assignment to ${stmt.$data.target}`);
      break;
    case "Call":
      console.log(
        `Call to ${stmt.$data.func} with ${stmt.$data.args.length} args`,
      );
      break;
  }
}
```

For the detailed specification, see the [Language Reference](docs/REFERENCE.md).

## Supported Languages

Plotnik is bundled with 26 languages:

> Bash, C, C++, C#, CSS, Elixir, Go, Haskell, HCL, HTML, Java, JavaScript, JSON, Kotlin, Lua, Nix, PHP, Python, Ruby, Rust, Scala, Solidity, Swift, TypeScript, TSX, YAML

Additional languages and dynamic loading are planned.

## Roadmap

### Ignition: the parser ✓

The foundation is complete: a resilient parser that recovers from errors and keeps going.

- [x] Resilient parser
- [x] Rich diagnostics
- [x] Name resolution
- [x] Recursion validation
- [x] Structural validation

### Liftoff: type inference

The schema infrastructure is built. Type inference is next.

- [x] `node-types.json` parsing and schema representation
- [x] Proc macro for compile-time schema embedding
- [x] Statically bundled languages with node type info
- [x] Query validation against language schemas (unstable)
- [x] Type inference

### Acceleration: query engine

- [x] Runtime execution with backtracking cursor walker
- [x] Query IR
- [ ] Advanced validation powered by `grammar.json` (production rules, precedence)
- [ ] Match result API with typed accessors

### Orbit: developer experience

The CLI foundation exists. The full developer experience is ahead.

- [x] CLI framework with `debug`, `docs`, `langs`, `exec`, `types` commands
- [x] Query inspection: AST dump, symbol table, node arities, spans, transition graph, inferred types
- [x] Source inspection: Tree-sitter parse tree visualization
- [x] Execute queries against source code and output JSON (`exec`)
- [x] Generate TypeScript types from queries (`types`)
- [ ] CLI distribution: Homebrew, cargo-binstall, npm wrapper
- [ ] Compiled queries via Rust proc macros (zero-cost: query → native code)
- [ ] Language bindings: TypeScript (WASM), Python, Ruby
- [ ] LSP server: diagnostics, completions, hover, go-to-definition
- [ ] Editor extensions: VS Code, Zed, Neovim

## Acknowledgments

[Max Brunsfeld](https://github.com/maxbrunsfeld) created Tree-sitter; [Amaan Qureshi](https://github.com/amaanq) and other contributors maintain the parser ecosystem that makes this project possible.

## License

This project is licensed under the [Apache License (Version 2.0)].

[Apache License (Version 2.0)]: LICENSE
