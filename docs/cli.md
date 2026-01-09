# Plotnik CLI Guide

> Query language for tree-sitter ASTs with type inference.

## Quick Start

```sh
# Explore a source file's tree-sitter AST
plotnik ast app.ts

# Validate a query against a grammar
plotnik check -q 'Func = (function_declaration) @fn' -l typescript

# Generate TypeScript types
plotnik infer -q 'Func = (function_declaration) @fn' -l typescript

# List supported languages
plotnik lang list

# Dump grammar for a language
plotnik lang dump typescript
```

---

## Commands

| Command     | Input | Purpose                         | `-l` flag                             |
| ----------- | ----- | ------------------------------- | ------------------------------------- |
| `ast`       | both  | Show AST of query and/or source | Inferred from extension               |
| `dump`      | query | Show bytecode                   | Optional (enables linking)            |
| `check`     | query | Validate query                  | Optional (enables grammar validation) |
| `infer`     | query | Generate type definitions       | Required                              |
| `exec`      | both  | Run query, output JSON          | Inferred from extension               |
| `trace`     | both  | Trace query execution           | Inferred from extension               |
| `lang list` | —     | List supported languages        | —                                     |
| `lang dump` | —     | Dump grammar for a language     | —                                     |

---

### ast

Show AST of query and/or source file. Use this to discover node kinds and structure.

```sh
# Show tree-sitter AST of source file
plotnik ast app.ts

# Show query AST
plotnik ast query.ptk

# Show both query and source AST
plotnik ast query.ptk app.ts

# Include anonymous nodes (literals, punctuation)
plotnik ast app.ts --raw
```

**Flags:**

| Flag    | Purpose                                         |
| ------- | ----------------------------------------------- |
| `--raw` | Include anonymous nodes (literals, punctuation) |

---

### dump

Show compiled bytecode. For debugging the compiler.

```sh
# Unlinked bytecode (parse + analyze + emit)
plotnik dump -q 'Q = (identifier) @id'

# Linked bytecode (+ link against grammar)
plotnik dump -q 'Q = (identifier) @id' -l typescript
```

Without `-l`: node kinds are unresolved symbols.
With `-l`: node kinds are resolved to grammar IDs.

---

### check

Validate a query. Like `cargo check`.

```sh
# Validate syntax, types, recursion (no grammar)
plotnik check -q 'Q = (identifier) @id'

# Also validate node kinds and fields against grammar
plotnik check -q 'Q = (function_declaration) @fn' -l typescript
```

Without `-l`: validates syntax, type inference, recursion rules, alt consistency.
With `-l`: also validates node kinds and field names exist in grammar.

**Flags:**

| Flag         | Purpose                   |
| ------------ | ------------------------- |
| `-l, --lang` | Enable grammar validation |
| `--strict`   | Treat warnings as errors  |

On success: silent, exits 0.
On error: prints diagnostics, exits 1.

---

### infer

Generate type definitions from a query. Currently supports TypeScript.

```sh
# Generate TypeScript types
plotnik infer -q 'Func = (function_declaration) @fn' -l javascript

# Write to file
plotnik infer -q 'Func = (function_declaration) @fn' -l javascript -o types.d.ts

# Verbose node shape (includes line/column)
plotnik infer -q 'Func = (function_declaration) @fn' -l javascript --verbose-nodes

# Skip boilerplate (Node/Point types, exports)
plotnik infer -q 'Q = (identifier) @id' -l js --no-node-type --no-export
```

**Flags:**

| Flag                | Purpose                                       |
| ------------------- | --------------------------------------------- |
| `-l, --lang LANG`   | Target language grammar (required)            |
| `-o, --output FILE` | Write output to file                          |
| `--format FORMAT`   | Output format (`typescript`, `ts`)            |
| `--verbose-nodes`   | Include line/column in Node type              |
| `--no-node-type`    | Don't emit Node/Point definitions             |
| `--no-export`       | Don't add `export` keyword                    |
| `--void-type TYPE`  | Type for void results (`undefined` or `null`) |

### lang

Language information and grammar tools.

#### lang list

List all supported tree-sitter languages with their aliases.

```sh
plotnik lang list
```

```
bash (sh, shell)
c
cpp (c++, cxx, cc)
javascript (js, jsx, ecmascript, es)
typescript (ts)
...
```

#### lang dump

Dump a language's grammar in Plotnik-like syntax. Useful for learning how to write queries against a grammar.

```sh
plotnik lang dump json
plotnik lang dump typescript
```

The output uses a syntax similar to Plotnik queries:

- `(node_kind)` — named node (queryable)
- `"literal"` — anonymous node (queryable)
- `(_hidden ...)` — hidden rule (not queryable, children inline)
- `{...}` — sequence (ordered children)
- `[...]` — alternation (first match)
- `? * +` — quantifiers
- `"x"!` — immediate token (no whitespace before)
- `field: ...` — named field
- `T :: supertype` — supertype declaration

### exec

Execute a query against source code and output JSON matches.

```sh
# Two positional arguments: QUERY SOURCE
plotnik exec query.ptk app.js

# Inline query + positional source (most common)
plotnik exec -q 'Q = (identifier) @id' app.js

# All inline (requires -l)
plotnik exec -q 'Q = (identifier) @id' -s 'let x = 1' -l javascript

# Include source positions in output
plotnik exec -q 'Q = (identifier) @id' app.ts --verbose-nodes

# Start from a specific definition
plotnik exec query.ptk app.js --entry FunctionDef
```

**Flags:**

| Flag              | Purpose                                |
| ----------------- | -------------------------------------- |
| `-q, --query`     | Inline query text                      |
| `-s, --source`    | Inline source text                     |
| `-l, --lang`      | Language (inferred from file ext)      |
| `--compact`       | Output compact JSON                    |
| `--verbose-nodes` | Include line/column in nodes           |
| `--check`         | Validate output against inferred types |
| `--entry NAME`    | Start from specific definition         |

---

### trace

Trace query execution for debugging.

```sh
# Inline query + positional source
plotnik trace -q 'Q = (identifier) @id' app.js

# Two positional arguments
plotnik trace query.ptk app.js

# All inline
plotnik trace -q 'Q = (identifier) @id' -s 'let x = 1' -l js

# Skip result, show only effects
plotnik trace query.ptk app.js --no-result

# Increase verbosity
plotnik trace query.ptk app.js -v   # verbose
plotnik trace query.ptk app.js -vv  # very verbose
```

**Flags:**

| Flag          | Purpose                        |
| ------------- | ------------------------------ |
| `-v`          | Verbose output                 |
| `-vv`         | Very verbose output            |
| `--no-result` | Skip materialization           |
| `--fuel N`    | Execution fuel limit           |
| `--entry`     | Start from specific definition |

---

## Input Modes

### Query-Only Commands (check, dump, infer)

These commands take a single input. Use either:

- **Positional**: `plotnik dump query.ptk`
- **Flag**: `plotnik dump -q 'Q = ...'`

### Query+Source Commands (ast, exec, trace)

These commands can take query, source, or both inputs. Use any combination:

| Pattern                          | Query from     | Source from    |
| -------------------------------- | -------------- | -------------- |
| `exec QUERY SOURCE`              | 1st positional | 2nd positional |
| `exec -q '...' SOURCE`           | `-q` flag      | positional     |
| `exec -s '...' QUERY -l lang`    | positional     | `-s` flag      |
| `exec -q '...' -s '...' -l lang` | `-q` flag      | `-s` flag      |

**Key rule**: When `-q` is provided with one positional, it becomes SOURCE.

### Language Detection

| Input type    | Language             |
| ------------- | -------------------- |
| File          | Inferred from `.ext` |
| Inline (`-s`) | Requires `-l`        |

---

## Reading from Stdin

Use `-` as the file argument:

```sh
# Query from stdin
echo 'Q = (identifier) @id' | plotnik dump -

# Source from stdin
cat app.ts | plotnik ast -

# Exec: query from stdin, source from file
echo 'Q = (identifier) @id' | plotnik exec - app.js
```

---

## Workflow Examples

### Developing a Query

1. **Explore the source AST** to understand node structure:

   ```sh
   plotnik ast example.ts
   ```

2. **Write a query and validate** against the grammar:

   ```sh
   plotnik check -q 'Func = (function_declaration name: (identifier) @name)' -l typescript
   ```

3. **Generate TypeScript types**:

   ```sh
   plotnik infer -q 'Func = (function_declaration name: (identifier) @name)' \
     -l typescript -o func.d.ts
   ```

### Query Files

For complex queries, use files with `.ptk` extension:

```
; queries/functions.ptk
; Match function declarations with their parameters

Func = (function_declaration
  name: (identifier) @name
  parameters: (formal_parameters
    {(required_parameter
      pattern: (identifier) @param_name
      type: (_) @param_type
    ) @param}* @params
  )
)
```

```sh
plotnik infer queries/functions.ptk -l typescript -o types.d.ts
```

---

## Color Output

Color is auto-detected based on terminal capability.

| Option           | Behavior             |
| ---------------- | -------------------- |
| `--color auto`   | Detect TTY (default) |
| `--color always` | Force colors         |
| `--color never`  | Disable colors       |

Respects `NO_COLOR` environment variable.

---

## Error Messages

Plotnik provides detailed diagnostics with source context:

```
error: unknown node type 'function_decl'
 --> query.ptk:3:5
  |
3 |     (function_decl name: (identifier) @name)
  |      ^^^^^^^^^^^^^ not a valid TypeScript node type
  |
help: did you mean 'function_declaration'?
```

Common errors:

| Error                             | Cause                                | Fix                                      |
| --------------------------------- | ------------------------------------ | ---------------------------------------- |
| `unknown node type`               | Typo in node kind                    | Check `plotnik ast file` for valid types |
| `missing closing )`               | Unclosed tree pattern                | Match parentheses                        |
| `expected expression`             | Invalid syntax                       | Check query syntax                       |
| `strict dimensionality violation` | Quantified captures need row wrapper | Use `{...}* @rows` pattern               |

---

## Exit Codes

| Code | Meaning                           |
| ---- | --------------------------------- |
| 0    | Success                           |
| 1    | Error (parse, validation, or I/O) |

---

## Tips

1. **Start with `ast`** to explore unfamiliar codebases
2. **Use `--raw`** to see all tokens including literals
3. **Run `check`** before `infer` to catch grammar errors early
4. **Use `dump`** to debug query parsing or bytecode
5. **Use query files** for anything beyond one-liners
6. **Match `--verbose-nodes`** between `infer` and `exec` for consistent shapes
