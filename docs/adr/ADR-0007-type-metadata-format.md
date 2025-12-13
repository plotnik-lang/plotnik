# ADR-0007: Type Metadata Format

- **Status**: Accepted
- **Date**: 2025-01-13

## Context

Query execution produces structured values via the effect stream ([ADR-0006](ADR-0006-dynamic-query-execution.md)). Type metadata enables:

- **Code generation**: Emit Rust structs, TypeScript interfaces, Python dataclasses
- **Validation**: Verify effect stream output matches expected shape (debug/test builds)
- **Tooling**: IDE completions, documentation generation

Type metadata is descriptive, not prescriptive. Transitions define execution semantics; types describe what transitions produce.

**Cache efficiency goal**: Proc macro compilation inlines query logic as native instructions (I-cache), leaving D-cache exclusively for tree-sitter cursor traversal. Type metadata is consumed at compile time, not runtime.

## Decision

### TypeId

```rust
type TypeId = u16;

const TYPE_VOID: TypeId = 0;       // definition captures nothing
const TYPE_NODE: TypeId = 1;       // AST node reference (see "Node Semantics" below)
const TYPE_STR: TypeId = 2;        // extracted source text (:: string)
// 3..0xFFFE: composite types (index into type_defs + 3)
const TYPE_INVALID: TypeId = 0xFFFF;  // error sentinel during inference
```

Type alias declared in [ADR-0004](ADR-0004-query-ir-binary-format.md); constants and semantics here.

Primitives exist only as TypeId values—no TypeDef entries. Composite types start at ID 3.

### Node Semantics

`TYPE_NODE` represents a platform-dependent handle to a tree-sitter AST node:

| Context    | Representation                                             |
| ---------- | ---------------------------------------------------------- |
| Rust       | `tree_sitter::Node<'tree>` (lifetime-bound reference)      |
| TypeScript | Binding-provided object with `startPosition`, `text`, etc. |
| Text/JSON  | Unique node identifier (e.g., `"node:42"` or path-based)   |

The handle provides access to node metadata (kind, span, text) without copying the source. Lifetime management is platform-specific—Rust enforces it statically, bindings may use reference counting or arena allocation.

### TypeDef

```rust
#[repr(C)]
struct TypeDef {
    kind: TypeKind,              // 1
    _pad: u8,                    // 1
    name: StringId,              // 2 - synthetic or explicit, 0xFFFF for wrappers
    members: Slice<TypeMember>,  // 6 - see interpretation below
    _pad2: u16,                  // 2
}
// 12 bytes, align 4
```

The `members` field has dual semantics based on `kind`:

| Kind                               | `members.start_index`   | `members.len` |
| ---------------------------------- | ----------------------- | ------------- |
| Wrappers (Optional/Array\*/Array+) | Inner `TypeId` (as u32) | 0             |
| Composites (Record/Enum)           | Index into type_members | Member count  |

This reuses `Slice<T>` for consistency with [ADR-0005](ADR-0005-transition-graph-format.md), while keeping TypeDef compact.

### TypeKind

```rust
#[repr(C, u8)]
enum TypeKind {
    Optional = 0,   // T?  — members.start = inner TypeId
    ArrayStar = 1,  // T*  — members.start = element TypeId
    ArrayPlus = 2,  // T+  — members.start = element TypeId
    Record = 3,     // struct — members = slice into type_members
    Enum = 4,       // tagged union — members = slice into type_members
}
```

| Kind      | Query Syntax        | Semantics                        |
| --------- | ------------------- | -------------------------------- |
| Optional  | `expr?`             | Nullable wrapper                 |
| ArrayStar | `expr*`             | Zero or more elements            |
| ArrayPlus | `expr+`             | One or more elements (non-empty) |
| Record    | `{ ... } @name`     | Named fields                     |
| Enum      | `[ A: ... B: ... ]` | Tagged union (discriminated)     |

### TypeMember

Shared structure for Record fields and Enum variants:

```rust
#[repr(C)]
struct TypeMember {
    name: StringId,  // 2 - field name or variant tag
    ty: TypeId,      // 2 - field type or variant payload (TYPE_VOID for unit)
}
// 4 bytes, align 2
```

### Synthetic Naming

When no explicit `:: TypeName` annotation exists, names are synthesized:

| Context              | Pattern         | Example                                  |
| -------------------- | --------------- | ---------------------------------------- |
| Definition           | Definition name | `Func`                                   |
| Captured sequence    | `{Def}{Field}`  | `FuncParams` for `@params` in `Func`     |
| Captured alternation | `{Def}{Field}`  | `FuncBody` for `@body` in `Func`         |
| Variant payload      | `{Parent}{Tag}` | `FuncBodyStmt` for `Stmt:` in `FuncBody` |

Collisions resolved by numeric suffix: `FuncBody`, `FuncBody2`, etc.

### Example

Query:

```
Func = (function_declaration
    name: (identifier) @name :: string
    body: [
        Stmt: (statement) @stmt
        Expr: (expression) @expr
    ] @body
)
```

Type graph:

```
T3: Record "Func"         → [name: Str, body: T4]
T4: Enum "FuncBody"       → [Stmt: T5, Expr: T6]
T5: Record "FuncBodyStmt" → [stmt: Node]
T6: Record "FuncBodyExpr" → [expr: Node]

Entrypoint: Func → result_type: T3
```

Generated TypeScript:

```typescript
interface Func {
  name: string;
  body:
    | { $tag: "Stmt"; $data: { stmt: Node } }
    | { $tag: "Expr"; $data: { expr: Node } };
}
```

Generated Rust:

```rust
struct Func {
    name: String,
    body: FuncBody,
}

enum FuncBody {
    Stmt { stmt: Node },
    Expr { expr: Node },
}
```

### Validation

Optional runtime check for debugging:

```rust
fn validate(value: &Value, expected: TypeId, query: &CompiledQuery) -> Result<(), TypeError>;
```

Walk the `Value` tree, verify shape matches `TypeId`. Mismatch indicates IR construction bug—panic in debug, skip in release.

## Consequences

**Positive**:

- Single IR serves interpreter, proc macro codegen, and external tooling
- Language-agnostic: same metadata generates Rust, TypeScript, Python, etc.
- Self-contained queries enable caching by input hash (`~/.cache/plotnik/`)

**Negative**:

- Synthetic names can be verbose for deeply nested structures
- KB-scale overhead for complex queries (acceptable)

## References

- [ADR-0004: Query IR Binary Format](ADR-0004-query-ir-binary-format.md)
- [ADR-0005: Transition Graph Format](ADR-0005-transition-graph-format.md)
- [ADR-0006: Dynamic Query Execution](ADR-0006-dynamic-query-execution.md)
