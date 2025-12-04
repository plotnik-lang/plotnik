<br/>
<br/>

<p align="center">
  <img width="400" alt="The logo: a curled wood shaving on a workbench" src="https://github.com/user-attachments/assets/1fcef0a9-20f8-4500-960b-f31db3e9fd94" />
</p>

<h1><p align="center">Plotnik</p></h1>  

<p align="center">
  Typed query language for <a href="https://tree-sitter.github.io/">tree-sitter</a>
</p>

<p align="center">
  <a href="https://github.com/plotnik-lang/plotnik/actions/workflows/stable.yml"><img src="https://github.com/plotnik-lang/plotnik/actions/workflows/stable.yml/badge.svg" alt="stable"></a>
  <a href="https://github.com/plotnik-lang/plotnik/actions/workflows/nightly.yml"><img src="https://github.com/plotnik-lang/plotnik/actions/workflows/nightly.yml/badge.svg" alt="nightly"></a>
  <a href="https://codecov.io/gh/plotnik-lang/plotnik"><img src="https://codecov.io/gh/plotnik-lang/plotnik/graph/badge.svg?token=071HXJIY3E"/></a>
  <a href="LICENSE.md"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT License"></a>
</p>

<br/>
<br/>

For more details, see [reference](docs/REFERENCE.md).

## Roadmap 🚀

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
