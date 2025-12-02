Based on my research, here are **additional projects** that use a similar AST-over-CST pattern with rowan or similar libraries:

## 1. **RSLint** (JavaScript/TypeScript Linter)

**URL:** https://github.com/rslint/rslint/blob/master/xtask/src/ast.rs

- Uses **code generation** approach with macros: `ast_nodes!` and `ast_enums!`
- Generates typed AST wrappers from metadata definitions
- Supports ~180+ JavaScript/TypeScript node types
- Field system with Token/Node/Optional/Many variants
- Very similar philosophy to your approach but at much larger scale

## 2. **Biome** (Rome fork - JS/TS/JSON formatter & linter)

**URL:** https://github.com/biomejs/biome

Key file: `crates/biome_js_syntax/src/generated/nodes.rs`

Pattern used:

```rust
pub struct JsAccessorModifier {
    pub(crate) syntax: SyntaxNode,
}

pub const unsafe fn new_unchecked(syntax: SyntaxNode) -> Self {
    Self { syntax }
}

pub fn as_fields(&self) -> JsAccessorModifierFields {
    JsAccessorModifierFields {
        modifier_token: self.modifier_token(),
    }
}
```

- Uses `as_fields()` pattern for structured access
- Implements `Serialize` for all nodes
- Production-scale implementation with thousands of nodes

## 3. **cstree** (Rowan fork with improvements)

**URL:** https://github.com/domenicquirl/cstree

- Direct fork/evolution of rowan
- **Persistent red nodes** (better performance)
- Thread-safe with `Send + Sync` support
- String interning for deduplication
- Same cast pattern but with optimizations

## 4. **lelwel** (LL(1) Parser Generator)

**URL:** https://github.com/0x2a-42/lelwel

- Generates **lossless CST** from grammar
- Provides node control operators for AST construction
- Users build typed AST over generated CST
- Similar two-layer approach

## 5. **Taplo** (TOML Parser & Formatter)

**URL:** https://github.com/tamasfe/taplo
**Docs:** https://docs.rs/taplo

- Uses rowan for syntax tree
- 1.2M+ downloads on crates.io
- Preserves layout and token positions
- Production TOML toolkit with typed accessors

## 6. **Starlark-rust** (Facebook)

**URL:** https://github.com/facebook/starlark-rust

- `starlark_syntax` crate with AST definitions
- Python-like language parser
- Used in Buck2 build system
- Large-scale production usage

## Summary

Your `ast.rs` implementation follows the **exact same pattern** as:

- **rowan's s_expressions example** (closest match - teaching example)
- **rust-analyzer's syntax crate** (production implementation)
- **Biome/Rome** (very large scale)
- **RSLint** (with code generation layer)

The pattern is well-established in the Rust ecosystem for parsers that need:

- Lossless syntax trees (preserve all source info)
- Typed AST layer for ergonomic traversal
- Editor/IDE tooling support
