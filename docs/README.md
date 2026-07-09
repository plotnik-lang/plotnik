# Plotnik Documentation

Plotnik is a strongly-typed pattern matching language for tree-sitter syntax trees.

## Quick Links by Audience

### Users

- [CLI Guide](cli.md) — Command-line tool usage
- [Language Reference](lang-reference.md) — Complete syntax and semantics
- [Type System](type-system.md) — How output types are inferred from queries

### Contributors & LLM Agents

- [AGENTS.md](../AGENTS.md) — Project rules, coding standards, testing patterns
- [Generated Runtime Interface](runtime-interface.md) — Cross-language codegen runtime contract
- [Runtime Engine](runtime-engine.md) — VM execution model
- [Tree Navigation](tree-navigation.md) — Cursor walk implementation
- [Binary Format](binary-format/01-overview.md) — Compiled query format

## Document Map

```
AGENTS.md              # Project constitution (coding rules, testing)
docs/
├── README.md          # You are here
├── cli.md             # CLI tool usage guide
├── lang-reference.md  # Query language syntax and semantics
├── type-system.md     # Type inference rules and output shapes
├── runtime-interface.md # Generated matcher/runtime contract
├── runtime-engine.md  # VM state, backtracking, effects
├── tree-navigation.md # Cursor walk, search loop, anchor lowering
└── binary-format/     # Compiled bytecode specification
    ├── 01-overview.md   # Header, sections, alignment
    ├── 02-strings.md    # String pool and table
    ├── 03-symbols.md    # Node kinds, fields, trivia
    ├── 04-types.md      # Type metadata format
    ├── 05-entrypoints.md # Callable definition table
    ├── 06-transitions.md # VM instructions and data blocks
    ├── 07-spans.md      # Inspection spans section
    ├── 08-dump-format.md # Bytecode dump output format
    └── 09-trace-format.md # Execution trace output format
```

## Reading Order

New to Plotnik:

1. `cli.md` — Get started with the CLI
2. `lang-reference.md` — Learn the query syntax
3. `type-system.md` — Understand output shapes

Building tooling:

1. `runtime-interface.md` — Cross-language generated runtime contract
2. `binary-format/01-overview.md` → through `06-transitions.md`
3. `runtime-engine.md`
4. `tree-navigation.md`
5. `binary-format/08-dump-format.md` — Understanding bytecode dumps
6. `binary-format/09-trace-format.md` — Debugging with execution traces

Contributing:

1. `AGENTS.md` — Required reading
