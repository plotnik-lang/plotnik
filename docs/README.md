# Plotnik Documentation

Plotnik is a strongly-typed pattern matching language for tree-sitter syntax trees.

## Quick Links by Audience

### Users

- [Language Reference](lang-reference.md) — Complete syntax and semantics
- [Type System](type-system.md) — How output types are inferred from queries

### Contributors & LLM Agents

- [AGENTS.md](../AGENTS.md) — Project rules, coding standards, testing patterns
- [Runtime Engine](runtime-engine.md) — VM execution model
- [Binary Format](binary-format/01-overview.md) — Compiled query format

## Document Map

```
AGENTS.md              # Project constitution (coding rules, testing, ADRs)
docs/
├── README.md          # You are here
├── lang-reference.md  # Query language syntax and semantics
├── type-system.md     # Type inference rules and output shapes
├── runtime-engine.md  # VM state, backtracking, effects
└── binary-format/     # Compiled bytecode specification
    ├── 01-overview.md   # Header, sections, alignment
    ├── 02-strings.md    # String pool and table
    ├── 03-symbols.md    # Node types, fields, trivia
    ├── 04-types.md      # Type metadata format
    ├── 05-entrypoints.md # Public definition table
    └── 06-transitions.md # VM instructions and data blocks
```

## Reading Order

New to Plotnik:

1. `lang-reference.md` — Learn the query syntax
2. `type-system.md` — Understand output shapes

Building tooling:

1. `binary-format/01-overview.md` → through `06-transitions.md`
2. `runtime-engine.md`

Contributing:

1. `AGENTS.md` — Required reading
2. ADRs in `docs/adr/` — Architectural context
