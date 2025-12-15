# ADR-0011: Workspace and Entrypoints

- **Status**: Accepted
- **Date**: 2025-01-14
- **Supersedes**: "Last expression is root" convention

## Context

Previous iterations of Plotnik treated input as a single source file. The entrypoint was implicitly defined as the last expression in the file. This approach is brittle for larger projects and creates ambiguity in compilation (non-deterministic entry points in multifile scenarios).

We need a module system that supports composition without the boilerplate of explicit imports (e.g., `import "../common.ptk"`), similar to Terraform's module system.

## Decision

### 1. The Workspace

A **Workspace** is defined as a directory containing Plotnik source files (`*.ptk`).

- **Single Compilation Unit**: All files in the workspace are loaded and treated as a single scope.
- **Flat Namespace**: A definition `Foo = ...` in `a.ptk` is visible in `b.ptk` without imports.
- **No Subdirectories**: The workspace is non-recursive. Subdirectories are ignored (or treated as separate workspaces).

### 2. Visibility and Entrypoints

We distinguish between **Internal Definitions** (Mixins) and **Public Entrypoints** via the `pub` keyword.

#### Internal Definitions (`Def = ...`)

- **Scope**: Visible to all files in the workspace.
- **Role**: Reusable logic, fragments, mixins.
- **Collision**: Global uniqueness is enforced. Defining `Foo` in both `a.ptk` and `b.ptk` is a compilation error.
- **Output**: Not exposed in the compiled binary's entrypoints table. They exist only to support `pub` definitions.
- **Recursion**: Fully supported. The compiler detects cycles and generates function calls (Enter/Exit transitions) instead of inlining, enabling deep recursive patterns without `pub` or explicit captures.

#### Public Entrypoints (`pub Def = ...`)

- **Scope**: Visible to all files in the workspace (same as internal).
- **Role**: The API surface of the query. These are the roots for compilation.
- **Output**: Each `pub` definition creates an entry in the compiled binary's `entrypoints` table.

### 3. Language Inference

The workspace language is inferred from the directory name to support "Convention over Configuration".

- **Strategy**: The directory name is split by delimiters (`.`, `-`, `_`).
- **Matching**: If any token matches a known language ID or alias (e.g., `ts`, `typescript`, `rust`, `rs`), that language is selected.
- **Priority**: If multiple tokens match (ambiguous), compilation fails.
- **Fallback**: If no tokens match, the user must provide the `-l` / `--lang` CLI flag.

**Examples:**

- `queries.ts/` -> TypeScript
- `java-checks/` -> Java
- `lint_python/` -> Python
- `rust/` -> Rust

### 4. Removal of Implicit Roots

In Workspace files (`*.ptk`), top-level anonymous expressions are no longer allowed or treated as entrypoints.

**Old (Invalid):**

```plotnik
// a.ptk
(function_declaration) @fn
```

**New (Valid):**

```plotnik
// a.ptk
pub Main = (function_declaration) @fn
```

### 5. Compilation Logic

1.  **Discovery**: Scan `*.ptk` in the target directory.
2.  **Parsing**: Parse all files into a unified definition table.
3.  **Resolution**: Resolve references. Error on duplicate names.
4.  **Reachability**: Identify all `pub` definitions as roots. Prune any internal definitions not reachable from a `pub` root (Dead Code Elimination).
5.  **Emission**: Generate a single `CompiledQuery` binary.
    - The `entrypoints` table (ADR-0004) is populated with all `pub` definitions.
    - Internal definitions are compiled efficiently: acyclic parts are inlined, while recursive cycles use `RefTransition` jumps.

### 6. CLI Implications

The CLI supports two modes of operation: **Module Mode** (files) and **Script Mode** (CLI arguments).

#### Module Mode (Files)

When loading a workspace or a file (`*.ptk`):

- **Strictness**: Only named definitions (`Def = ...` or `pub Def = ...`) are allowed.
- **No Anonymous Expressions**: Top-level `(identifier)` is a syntax error.
- **Entrypoint**: Determined by `pub` definitions and `--entry` flag.
- **Root Validation**: Public entrypoints must match the language root node (e.g., `(source_file)` in Rust). Mismatches trigger a diagnostic suggesting the correct wrapper.

#### Script Mode (`-q` flag)

When providing a query string via `--query` / `-q`:

- **Relaxed**: Anonymous expressions are allowed.
- **Auto-Rooting**: The CLI wraps the query in a synthetic entrypoint anchored to the language root (e.g., `pub Query = (program <USER_QUERY>)` for JS). This simplifies one-liners.
- **Workspace Access**: The script can reference definitions from the current workspace (if loaded).

#### Execution

The `exec` command requires knowing _which_ entrypoint to run.

- **Single Entrypoint**: If the workspace contains exactly one `pub Def`, it is the default.
- **Multiple Entrypoints**: The user must specify `--entry <Name>`.
- **No Entrypoints**: Compilation error (nothing to run).

## Consequences

**Positive**:

- **Zero Boilerplate**: Easy to split logic across files (`common.ptk`, `java_rules.ptk`) without managing imports.
- **Explicit API**: `pub` clearly marks what is intended for execution vs. what is internal implementation detail.
- **Determinism**: Compilation output is deterministic regardless of file scan order.
- **Tree Shaking**: Unused mixins are automatically stripped.
- **Recursive Flattening**: Recursive mixins combined with Universal Bubbling allow constructing "grep-like" deep searches that produce flat lists of results (e.g., finding all identifiers nested arbitrarily deep).

**Negative**:

- **Namespace Pollution**: As projects grow, the flat namespace might lead to naming conflicts (mitigated by long names like `JavaMethod`, `JsFunction`).
- **Verbosity**: Simple one-liner scripts now require `pub Main = ...`.

## Example

`helpers.ptk`:

```plotnik
// Internal mixin, not runnable directly
Ident = (identifier)

// Recursive search pattern
// Matches any node that IS an identifier, OR any node that CONTAINS identifiers
DeepSearch = [
    (Ident) @target
    (_ (DeepSearch)*)
]
```

`main.ptk`:

```plotnik
// Public entrypoint
pub AllIdentifiers = (program (DeepSearch)*)
```

Generated Binary:

- **Entrypoints**: `["Functions"]`
- **Graph**: Contains nodes for `Functions` and the inlined/referenced logic of `Ident`.
