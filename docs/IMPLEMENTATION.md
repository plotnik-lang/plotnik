# Implementation Notes: Named Expressions & Type Inference

This document outlines the implementation approach for named expressions, recursion, and type inference in Plotnik.

## Architecture Overview

| Phase    | Input             | Output                      |
| -------- | ----------------- | --------------------------- |
| Parse    | Source            | AST                         |
| Resolve  | AST               | AST + SymbolTable           |
| Infer    | AST + SymbolTable | TypeMap<Name, DataType>     |
| Validate | TypeMap           | Errors (cycles, mismatches) |
| Codegen  | TypeMap           | Rust/TS/Py types            |

---

## 1. Name Resolution (Two-Pass)

**Pass 1: Collect definitions**

- Walk top-level statements
- Build `Map<Name, PatternAST>` for all `Name = pattern` bindings
- No type work yet

**Pass 2: Resolve references**

- When encountering `(Expr)` where `Expr` is capitalized, look it up in the map
- Error if undefined
- Build a dependency graph for later validation

---

## 2. Type Representation

```rust
enum DataType {
    Node,
    String,
    Struct(Vec<Field>),           // from {...} @capture
    Union(Vec<DataType>),         // unlabeled [...]
    Tagged {                      // labeled [A: ... B: ...]
        discriminant: String,     // "kind" by default
        variants: Vec<Variant>,
    },
    Array {
        element: Box<DataType>,
        non_empty: bool,          // true for +, false for *
    },
    Optional(Box<DataType>),      // ?
    Ref(String),                  // reference to named type
}

struct Field {
    name: String,
    ty: DataType,
    optional: bool,
}

struct Variant {
    label: String,
    fields: Vec<Field>,
}
```

The `Ref(String)` variant is key: recursive types are referenced by name, never inlined.

---

## 3. Type Inference Algorithm

The core function:

```
infer(pattern, scope) -> (DataType, Vec<Field>)
```

Traversal is bottom-up. Each AST node returns:

1. Its own type (for when it's captured)
2. Fields it contributes to the current scope

### Rules by AST Node

| Node                       | Type                             | Fields Contributed                              |
| -------------------------- | -------------------------------- | ----------------------------------------------- |
| `(node_type)` uncaptured   | —                                | none                                            |
| `(node_type) @x`           | `Node`                           | `x: Node`                                       |
| `(node_type) @x :: string` | `String`                         | `x: String`                                     |
| `(_) @x`                   | `Node`                           | `x: Node`                                       |
| `(NamedExpr) @x`           | `Ref("NamedExpr")`               | `x: Ref("NamedExpr")`                           |
| `{...}` uncaptured         | —                                | flattened inner fields                          |
| `{...} @x`                 | `Struct(inner_fields)`           | `x: Struct(...)`                                |
| `{...} @x :: T`            | `Struct(inner_fields)` named `T` | `x: T`                                          |
| `[...]` unlabeled          | `Union(...)`                     | merged fields (optional if not in all branches) |
| `[A: ... B: ...]` labeled  | `Tagged(...)`                    | — (each branch has own fields)                  |
| `pattern?`                 | `Optional(inner)`                | field becomes optional                          |
| `pattern*`                 | `Array(inner, false)`            | field becomes array                             |
| `pattern+`                 | `Array(inner, true)`             | field becomes non-empty array                   |

### Scope Rules

- Captures (`@name`) add fields to the **current** scope
- `{...} @x` creates a **new** scope; inner captures become fields of the nested struct
- Without a capture, `{...}` flattens into the parent scope
- `[...]` with a capture also creates a new scope

---

## 4. Handling Recursion

Recursion "just works" because of `Ref`:

```
NestedCall = (call_expression
  function: [(identifier) @name (NestedCall) @inner]
  arguments: (arguments))
```

When inferring `NestedCall`'s body:

- `(identifier) @name` → field `name: Node`
- `(NestedCall) @inner` → field `inner: Ref("NestedCall")`
- Alternation makes both optional

Result: `{ name?: Node, inner?: Ref("NestedCall") }`

Codegen outputs:

```typescript
type NestedCall = { name?: Node; inner?: NestedCall };
```

---

## 5. Validation

Type inference alone doesn't catch all errors. Separate validation passes:

### 5.1 Undefined References

- Any `Ref(name)` where `name` is not in the symbol table

### 5.2 Type Mismatches in Alternations

- Same capture name with different types across branches
- Example: `[(identifier) @x :: string (number) @x :: number]` — error

### 5.3 Infinite Patterns (No Base Case)

- `Bad = (Bad)` — always recurses, never terminates
- `Bad = (foo (Bad))` — same problem

Detection: graph reachability. Can the pattern reach a "terminal" (non-recursive match) without going through a `Ref`?

Algorithm:

1. Build call graph: which named expressions reference which
2. For each named expression, check if there's a path to a terminal node
3. Alternations provide branching — at least one branch must be terminal
4. Quantifiers `*` and `?` are implicitly terminal (can match zero times)

### 5.4 Left Recursion (Optional, for Performance)

- `Expr = (Expr) ...` — left recursion causes performance issues in matching
- May want to warn or require explicit opt-in

---

## 6. Codegen

Each target language has its own emitter:

### TypeScript

```typescript
type NestedCall = { name?: Node; inner?: NestedCall };
type MemberChain =
  | { type: "Base"; name: Node }
  | { type: "Access"; object: MemberChain; property: Node };
```

### Rust

```rust
struct NestedCall {
    name: Option<Node>,
    inner: Option<Box<NestedCall>>,
}

enum MemberChain {
    Base { name: Node },
    Access { object: Box<MemberChain>, property: Node },
}
```

Note: Rust needs `Box` for recursive types.

### Python (with dataclasses)

```python
@dataclass
class NestedCall:
    name: Node | None
    inner: "NestedCall | None"
```

---

## 7. Open Questions

1. **Anonymous vs Named Types**: When should an inferred struct get a generated name vs stay anonymous?
   - Current rule: only `::T` annotations and named expressions produce named types
   - Everything else is structural/anonymous

2. **Type Deduplication**: If two patterns produce identical structures, should they share a type?
   - Probably not — keep them nominal unless explicitly aliased

3. **Error Recovery in Type Inference**: If one named expression has errors, should inference continue for others?
   - Yes, for IDE/LSP support — infer what you can, report errors separately
