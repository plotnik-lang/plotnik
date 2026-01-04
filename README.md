<br/>
<br/>

<p align="center">
  <img width="400" alt="The logo: a curled wood shaving on a workbench" src="https://github.com/user-attachments/assets/8f1162aa-5769-415d-babe-56b962256747" />
</p>

<h1><p align="center">Plotnik</p></h1>

<br/>

<p align="center">
  A type-safe query language for source code.<br/>
  Query in, typed data out.
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
  ⚠️ <a href="#status">ALPHA STAGE</a>: not for production use ⚠️<br/>
</p>

<br/>
<br/>

## The problem

Tree-sitter solved parsing. It powers syntax highlighting and code navigation at GitHub, drives the editing experience in Zed, Helix, and Neovim. It gives you a fast, accurate, incremental syntax tree for virtually any language.

The hard problem now is what comes _after_ parsing: extracting structured data from the tree:

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

Every extraction requires a new function, each one a potential source of bugs that won't surface until production.

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

Start simple—extract all function names from a file:

```clojure
Functions = (program
  {(function_declaration name: (identifier) @name :: string)}* @functions)
```

Plotnik infers the output type:

```typescript
type Functions = {
  functions: { name: string }[];
};
```

Scale up to tagged unions for richer structure:

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

For the detailed specification, see the [Language Reference](docs/lang-reference.md).

## Documentation

- [CLI Guide](docs/cli.md) — Command-line tool usage
- [Language Reference](docs/lang-reference.md) — Complete syntax and semantics
- [Type System](docs/type-system.md) — How output types are inferred from queries
- [Runtime Engine](docs/runtime-engine.md) — VM execution model (for contributors)

## Supported Languages

Plotnik bundles 15 languages out of the box: Bash, C, C++, CSS, Go, HTML, Java, JavaScript, JSON, Python, Rust, TOML, TSX, TypeScript, and YAML. The underlying [arborium](https://github.com/bearcove/arborium) collection includes 60+ permissively-licensed grammars—additional languages can be enabled as needed.

## Status

**Working now:** Parser with error recovery, type inference, query execution, CLI tools (`check`, `dump`, `infer`, `exec`, `trace`, `tree`, `langs`).

**Next up:** CLI distribution (Homebrew, npm), language bindings (TypeScript/WASM, Python), LSP server, editor extensions.

⚠️ Alpha stage—API may change. Not for production use.

## Acknowledgments

[Max Brunsfeld](https://github.com/maxbrunsfeld) created Tree-sitter; [Amaan Qureshi](https://github.com/amaanq) and other contributors maintain the parser ecosystem that makes this project possible.

## License

This project is licensed under the [Apache License (Version 2.0)].

[Apache License (Version 2.0)]: LICENSE
