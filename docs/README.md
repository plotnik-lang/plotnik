# Plotnik Documentation

Plotnik is a strongly-typed pattern matching language for tree-sitter syntax trees.

## Compiler and emission pipeline

Compilation stops at a target-neutral semantic NFA:

```text
Parse → Analyze → Link → Lower → CompiledQuery
                                   ├─ emit bytes → validate/load → Module
                                   ├─ emit(RustCodegenConfig) → Rust module
                                   ├─ emit_types(RustCodegenConfig) → Rust types
                                   └─ emit_types(TypeScriptCodegenConfig) → .d.ts
```

Emission is pure and never writes files. `CompiledQuery` contains no eager
bytecode. `BytecodeConfig` serves the in-process VM and compiler diagnostics;
the compiler emits bytecode and immediately validates it as raw input before
constructing a `Module`.
It is not accepted or persisted as a user-facing artifact. Inspection explicitly
re-lowers the bytecode with span effects.

```rust,ignore
use plotnik_lib::{BytecodeConfig, QueryBuilder, RustCodegenConfig};

let compiled = QueryBuilder::from_inline(query).compile(&grammar)?;
let module = compiled.emit(BytecodeConfig::new())?.into_artifact();
let rust = compiled.emit(RustCodegenConfig::new())?.into_artifact();
let types = compiled.emit_types(RustCodegenConfig::new())?.into_artifact();
```

Emission implementation lives under one subsystem:

```text
compiler/emit/
  plan.rs, matcher.rs, replay.rs, sink.rs, ansi.rs
  targets/{bytecode,rust,typescript}/
```

Emission snapshots mirror that taxonomy under `04-emit/bytecode`,
`04-emit/types`, and `04-emit/rust/module`; VM semantics remain under `06-vm`.

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
- [Bytecode](bytecode/00-intro.md): Encoding, validation, and execution

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
└── bytecode/          # Bytecode design and implementation
    ├── 00-intro.md        # Overview and contents
    ├── 01-layout.md       # Sections, alignment, integrity
    ├── 02-instructions.md # Matching, navigation, calls
    ├── 03-types.md        # Result shapes
    ├── 04-effects.md      # Result construction
    ├── 05-spans.md        # Inspection metadata and occurrences
    └── 06-tables.md       # Shared constants and regex encoding
```

## Reading Order

New to Plotnik:

1. `cli.md` — Get started with the CLI
2. `lang-reference.md` — Learn the query syntax
3. `type-system.md` — Understand output shapes

Contributing to the runtime or debugging the compiler:

1. `runtime-interface.md` — Cross-language generated runtime contract
2. [Bytecode](bytecode/00-intro.md): Encoding and validation requirements
3. `runtime-engine.md`
4. `tree-navigation.md`

Contributing:

1. `AGENTS.md` — Required reading
