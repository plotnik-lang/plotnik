<br/>

<p align="center">
  <img width="400" alt="The logo: a curled wood shaving on a workbench" src="https://github.com/user-attachments/assets/8f1162aa-5769-415d-babe-56b962256747" />
</p>

<h1><p align="center">Plotnik</p></h1>

<br/>

<p align="center">
  A type-safe query language for <a href="https://tree-sitter.github.io">Tree-sitter</a><br/>
</p>

<br/>

<p align="center">
  <a href="https://github.com/plotnik-lang/plotnik/actions/workflows/stable.yml"><img src="https://github.com/plotnik-lang/plotnik/actions/workflows/stable.yml/badge.svg" alt="stable"></a>
  <a href="https://github.com/plotnik-lang/plotnik/actions/workflows/nightly.yml"><img src="https://github.com/plotnik-lang/plotnik/actions/workflows/nightly.yml/badge.svg" alt="nightly"></a>
  <a href="https://codecov.io/gh/plotnik-lang/plotnik"><img src="https://codecov.io/gh/plotnik-lang/plotnik/graph/badge.svg?token=071HXJIY3E"/></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue.svg" alt="Apache-2.0 License"></a>
</p>

<br/>
<br/>

Plotnik is the tool for working with Tree-sitter reliably:
- Write queries with [familiar syntax](https://tree-sitter.github.io/tree-sitter/using-parsers/queries/1-syntax.html).
- Generate extractors in your language (Rust + more to come).
- Get structured results without walking the tree or assembling flat captures.
- Bump grammar versions without fear: the static checker has your back.

<br/>
<br/>

<p align="center"><b>Input</b></p>

```javascript
async function fetchUser(userId, options) {
  ...
}
```

<br/>
<p align="center"><b>Query</b></p>

```clojure
Function = (program
  (function_declaration
    "async"? @async :: bool
    name: (identifier) @name :: str
    parameters: (formal_parameters
      (identifier)* @args :: str
    )
    body: (statement_block) @body
  )
)
```

<br/>
<p align="center"><b>Output</b></p>

```ts
{
  "async": true,
  "name": "fetchUser",
  "args": ["userId", "options"],
  "body": <Node>
}
```

<br/>
<br/>

## Acknowledgments

[Max Brunsfeld](https://github.com/maxbrunsfeld) created Tree-sitter; [Amaan Qureshi](https://github.com/amaanq) and other contributors maintain the parser ecosystem that makes this project possible.

## License

This project is licensed under the [Apache License (Version 2.0)].

[Apache License (Version 2.0)]: LICENSE
