# ADR-0010: Type System v2 (Transparent Graph Model)

- **Status**: Accepted
- **Date**: 2025-01-14
- **Supersedes**: ADR-0009

## Context

The previous type system (ADR-0009) relied on implicit behaviors like "Quantifier-Induced Scope" (QIS) and "Single-Capture Unwrap" to reduce verbosity. While well-intentioned, these rules created "Wrapper Hell," where extracting logic into a reusable definition inadvertently changed the output structure.

We need a model that supports **Mixin-like composition** (logic reuse without structural nesting) while maintaining strict type safety and data integrity.

## Decision

We adopt the **Transparent Graph Model**.

### 1. Universal Bubbling ("Let It Bubble")

Captures (`@name`) always bubble up to the nearest **Explicit Scope Boundary**.

- **Private Definitions (`Def =`) are Transparent.** They act as macros or fragments.
- **Uncaptured Containers (`{...}`, `[...]`) are Transparent.**
- **References (`(Def)`) are Transparent.**

This enables compositional patterns where a definition contributes fields to its caller's struct.

### 2. Explicit Scope Boundaries

A new data structure (Scope) is created **only** by explicit intent.

1.  **Public Roots:** `pub Def = ...` (The API Contract).
2.  **Explicit Wrappers:**
    - `{...} @name` (Nested Group).
    - `[...] @name` (Nested Union).
    - `[ L: ... ] @name` (Tagged Union).

**Payload Rule**:

- **0 Captures**: `Void` (Logic-only matcher).
- **1..N Captures**: `Struct { field_1, ..., field_N }`.
- **No Implicit Unwrap**: A single capture `(node) @x` produces `{ x: Node }`. It is never unwrapped to `Node`.
  - _Benefit:_ Adding a second capture is non-breaking (`res.x` remains valid).

### 3. Parallel Arrays (Columnar Output)

Quantifiers (`*`, `+`) do **not** create implicit "Row Structs." instead, they change the cardinality of the bubbled fields to `Array`.

**Example**: `( (A) @a (B) @b )*`
**Output**: `{ a: Array<Node>, b: Array<Node> }` (Struct of Arrays).

This optimizes for the common case of data extraction (where SoA is often preferred) and avoids the complexity of implicit row creation.

### 4. Row Integrity (Safety Check)

To prevent **Data Desynchronization** (where `a[i]` no longer corresponds to `b[i]`), the Inference Pass enforces **Row Integrity**.

**Rule**: A quantified scope cannot mix **Synchronized** and **Desynchronized** fields.

- **Synchronized**: Field is strictly required (`1`) in the loop body.
- **Desynchronized**: Field is optional (`?`), repeated (`*`, `+`), or in an alternation.

| Pattern                | Fields         | Status       | Result          |
| :--------------------- | :------------- | :----------- | :-------------- |
| `(A) @a (B) @b`        | `a: 1`, `b: 1` | **Aligned**  | ✅ OK (Columns) |
| `[ (A) @a \| (B) @b ]` | `a: ?`, `b: ?` | **Disjoint** | ✅ OK (Buckets) |
| `(A) @a (B)? @b`       | `a: 1`, `b: ?` | **Mixed**    | ❌ **Error**    |

**Error Message**: _"Field `@b` is optional while `@a` is required. Parallel arrays will not align. Wrap in `{...} @row` to enforce structure."_

### 5. Definition Roles

| Feature            | `Def` (Private)               | `pub Def` (Public)      |
| :----------------- | :---------------------------- | :---------------------- |
| **Concept**        | **Fragment / Mixin**          | **API Contract / Root** |
| **Graph Behavior** | Inlined (Copy-Paste)          | Entrypoint              |
| **Scoping**        | Transparent (Captures bubble) | **Scope Boundary**      |
| **Output Type**    | Merges into parent            | Named Interface         |

## Mental Model Migration

| Old Way (Opaque)  | New Way (Transparent)                        |
| :---------------- | :------------------------------------------- | -------------------------------------------------- |
| **Extract Def**   | Broken `res.x`. Must rewrite as `res.def.x`. | Safe. `res.x` remains `res.x`.                     |
| **List of Items** | Implicit `RowStruct`. Hard to desync.        | Explicit `Array<x>, Array<y>`. Enforced integrity. |
| **Collision**     | Silent (Data Loss).                          | Compiler Error ("Duplicate Capture").              |
| **Fix Collision** | Manual re-capture.                           | Wrap: `{ (Def) } @alias`.                          |

## Edge Cases

### Recursive Definitions

Since private definitions inline their contents, infinite recursion is structurally impossible for inlining.

**Solution**:

- Recursive definitions must be `pub` (creating a stable API boundary) OR wrapped in a capture at the call site `(Recurse) @next`.
- _Note: This is a natural constraint. Recursion implies a tree structure, so the output type must naturally reflect that tree structure._

### Collision Handling

`A(B) = (node (B) (B))`

- **Issue**: `B` captures `@id`. Using it twice causes "Duplicate Capture".
- **Solution**: User must disambiguate: `(node (B) @left (B) @right)`.
- **Benefit**: The output shape `{ left: {id}, right: {id} }` matches the semantic intent.

## Consequences

**Positive**:

- **Refactoring Safety**: Extracting logic into a `Def` never changes the output shape.
- **Performance**: Parallel arrays (SoA) are cache-friendly and often what is needed for analysis.
- **Robustness**: The Row Integrity check prevents silent data corruption.
- **Simplicity**: No magic rules (QIS, Implicit Unwrap).

**Negative**:

- **Verbosity**: Must explicitly wrap `{...} @row` for list-of-structs.
- **Strictness**: "Mixed" optionality in loops is now a hard error, requiring explicit handling.
