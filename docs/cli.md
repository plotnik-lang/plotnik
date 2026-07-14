# Plotnik CLI Guide

> Query language for Tree-sitter syntax trees with type inference.

## Quick Start

```sh
# Explore a source file's syntax tree
plotnik tree app.ts

# Validate a query against a grammar
plotnik check -q 'Func = (function_declaration) @fn' -l typescript

# Generate TypeScript types
plotnik infer -q 'Func = (function_declaration) @fn' -l typescript

# Generate a compiled Rust matcher module
plotnik generate query.ptk --target rust -l typescript -o query.rs

# List supported languages
plotnik lang list

# Dump grammar for a language
plotnik lang dump typescript
```

---

## Commands

| Command       | Input | Purpose                         | `-l` flag                             |
| ------------- | ----- | ------------------------------- | ------------------------------------- |
| `run`         | both  | Run query, output JSON          | Shebang or extension                  |
| `check`       | query | Validate query                  | Optional (enables grammar validation) |
| `tree`        | both  | Show query and/or source trees  | Shebang or extension                  |
| `infer`       | query | Generate type definitions       | Required                              |
| `generate`    | query | Generate a compiled matcher     | Required unless `--grammar` is used   |
| `dump`        | query | Show bytecode                   | Optional (enables grammar binding)    |
| `trace`       | both  | Trace query execution           | Shebang or extension                  |
| `inspect`     | both  | Emit playground inspection JSON | Shebang or extension                  |
| `lang list`   | —     | List supported languages        | —                                     |
| `lang dump`   | —     | Dump grammar for a language     | —                                     |
| `completions` | —     | Generate shell completions      | —                                     |

`exec` is kept as a hidden alias for `run`.

**Bare invocation**: when the first argument is a `.ptk` file, the `run`
subcommand is implied — `plotnik query.ptk app.js` works. Combined with a
shebang line, query files become directly executable (see
[Shebang](#shebang-language-declaration)).

---

### tree

Show the query tree, the source syntax tree, or both. Query input supports AST
and CST views; source input omits anonymous nodes unless requested.

```sh
# Show source syntax tree
plotnik tree app.ts

# Show query AST
plotnik tree query.ptk

# Show query CST, including trivia
plotnik tree query.ptk --query-view cst

# Show both query and source trees
plotnik tree query.ptk app.ts

# Include anonymous nodes (literals, punctuation)
plotnik tree app.ts --include-anonymous

# Source tree as JSON
plotnik tree app.ts --json
```

**Flags:**

| Flag                  | Purpose                                          |
| --------------------- | ------------------------------------------------ |
| `--query-view VIEW`   | Select query AST or CST                          |
| `--include-anonymous` | Include anonymous source nodes and punctuation   |
| `--json`              | Output `query_tree` and/or `source_tree` as JSON |

---

### dump

Show compiled bytecode as text. This command is for learning and
compiler debugging; it neither reads nor writes a bytecode artifact.

```sh
plotnik dump -q 'Q = (identifier) @id' -l typescript
plotnik dump query.ptk -l typescript
```

Requires a language via `-l` or a shebang; node kinds are resolved to grammar IDs.

---

### check

Validate a query. Like `cargo check`, this parses, analyzes, binds, lowers, and
verifies the shared executor contracts without selecting an emission target.

```sh
# Validate syntax, types, recursion (no grammar)
plotnik check -q 'Q = (identifier) @id'

# Also validate node kinds and grammar fields
plotnik check -q 'Q = (function_declaration) @fn' -l typescript
```

Without `-l`: validates syntax, type inference, recursion rules, and alternative-label consistency.
With `-l`: also validates that node kinds and grammar-field names exist.

**Flags:**

| Flag         | Purpose                    |
| ------------ | -------------------------- |
| `-l, --lang` | Enable grammar validation  |
| `--strict`   | Treat warnings as errors   |
| `--json`     | Output diagnostics as JSON |

On success: silent, exits 0. A valid query with warnings prints them and
still exits 0 (`--strict` turns warnings into failures).
On error: prints diagnostics to stderr, exits 1.

`check` does not test bytecode-format capacities. A query can pass `check` and
emit source while a later VM-bytecode emission reports a target limit (for
example, the bytecode type table's per-record field width).

With `--json`, on exit 0 or 1 stdout is a JSON array of diagnostics (`[]`
when the query is clean), each with `code`, `severity`, `message`, `span`
(file/line/column/offset), and optional `related`, `fix`, and `hints`.
Exit 2 means the question couldn't be answered — the error goes to stderr
as text and no JSON is emitted:

```sh
plotnik check query.ptk --json | jq '.[].code'
```

---

### infer

Generate type definitions from a query. Currently supports TypeScript.

```sh
# Generate TypeScript types
plotnik infer -q 'Func = (function_declaration) @fn' -l javascript

# Write to file
plotnik infer -q 'Func = (function_declaration) @fn' -l javascript -o types.d.ts

# Include zero-based row/byte-column points
plotnik infer -q 'Func = (function_declaration) @fn' -l javascript --include-points

# Skip boilerplate (Node type, exports)
plotnik infer -q 'Q = (identifier) @id' -l js --no-node-type --no-export
```

**Flags:**

| Flag                     | Purpose                                             |
| ------------------------ | --------------------------------------------------- |
| `-l, --lang LANG`        | Source language (required)                          |
| `-o, --output FILE`      | Write output to file                                |
| `--format FORMAT`        | Output format (`typescript`, `ts`)                  |
| `--include-points`       | Include row/byte-column points in `Node`            |
| `--no-node-type`         | Don't emit the `Node` definition                    |
| `--no-export`            | Don't add `export` keyword                          |
| `--match-only-type TYPE` | Type for match-only results (`undefined` or `null`) |

### generate

Generate a self-contained compiled matcher module. Rust is the first target;
the generated file contains typed result types, `parse`/`matches` entry points,
and the matcher that runs on `plotnik-rt`.

```sh
# Bind using the bundled language registry
plotnik generate query.ptk --target rust -l typescript -o query.rs

# Bind using the exact grammar shipped by the production parser package
plotnik generate query.ptk --target rust \
  --grammar node_modules/tree-sitter-typescript/typescript/src/grammar.json \
  -o query.rs

# Inline query to stdout
plotnik generate -q 'Q = (program)' --target rust -l javascript
```

The output records the grammar name, SHA-256 of the exact `grammar.json` bytes,
and its source. At runtime, the generated matcher verifies every node-kind and grammar-field
id it uses against the tree's live language. If verification fails, regenerate
with `--grammar` pointing at the parser package used in production.

**Flags:**

| Flag                     | Purpose                                            |
| ------------------------ | -------------------------------------------------- |
| `--target rust`          | Generated-code target (currently Rust)             |
| `-l, --lang LANG`        | Bind using the bundled registry grammar            |
| `--grammar GRAMMAR_JSON` | Bypass the registry and bind using this exact file |
| `-o, --output FILE`      | Write the generated module instead of stdout       |

### lang

Language information and grammar tools.

#### lang list

List all supported Tree-sitter languages with their aliases.

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
- `[...]` — alternation
- `? * +` — quantifiers
- `"x"!` — immediate token (no whitespace before)
- `field: ...` — grammar-field constraint
- `T :: supertype` — supertype declaration

### run

Execute a query against source code and output JSON matches.

```sh
# Two positional arguments: QUERY SOURCE
plotnik run query.ptk app.js

# Bare invocation (run is implied for .ptk files)
plotnik query.ptk app.js

# Inline query + positional source (most common)
plotnik run -q 'Q = (identifier) @id' app.js

# All inline (requires -l)
plotnik run -q 'Q = (identifier) @id' -s 'let x = 1' -l javascript

# Start from a specific definition
plotnik run query.ptk app.js --entry FunctionDef

# Lift the fuel limit for a known-heavy query
plotnik run query.ptk app.js --fuel unbounded
```

**Flags:**

| Flag           | Purpose                                    |
| -------------- | ------------------------------------------ |
| `-q, --query`  | Inline query text                          |
| `-s, --source` | Inline source text                         |
| `-l, --lang`   | Language (inferred from file ext)          |
| `--compact`    | Output compact JSON                        |
| `--entry NAME` | Select a specific selectable definition    |
| `--fuel`       | Matcher work budget (see Execution Limits) |
| `--max-memory` | Memory limit (see Execution Limits)        |
| `--limits`     | Limit preset (`auto`/`unbounded`)          |

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

| Flag           | Purpose                                    |
| -------------- | ------------------------------------------ |
| `-v`           | Verbose output                             |
| `-vv`          | Very verbose output                        |
| `--no-result`  | Skip materialization                       |
| `--fuel`       | Matcher work budget (see Execution Limits) |
| `--max-memory` | Memory limit (see Execution Limits)        |
| `--limits`     | Limit preset (`auto`/`unbounded`)          |
| `--entry`      | Select a specific selectable definition    |

---

### inspect

Compile and execute a query, emitting the playground inspection bundle: query
spans and tokens, diagnostics, TypeScript declarations and bindings, entry
points, the result and its provenance, run statistics, and an optional
execution trace in one JSON document.

```sh
plotnik inspect query.ptk app.js --json

# All inline
plotnik inspect -q 'Q = (program (expression_statement (identifier) @id))' -s 'x' -l js --json

# Include the VM execution trace in the bundle
plotnik inspect query.ptk app.js --json -v
```

**Flags:**

| Flag           | Purpose                                    |
| -------------- | ------------------------------------------ |
| `--json`       | Output the full inspect bundle as JSON     |
| `-v`           | Include the VM execution trace             |
| `--entry NAME` | Select a specific selectable definition    |
| `--fuel`       | Matcher work budget (see Execution Limits) |
| `--max-memory` | Memory limit (see Execution Limits)        |
| `--limits`     | Limit preset (`auto`/`unbounded`)          |

Exit codes follow `run`: `0` match, `1` no match or invalid query (the bundle is
still printed, with diagnostics), `2` couldn't answer.

---

## Execution Limits

`run`, `trace`, and `inspect` bound a run by two orthogonal resources, on by default and
sized from the input so they stay invisible to legitimate queries:

| Flag           | Accepts                              | Default |
| -------------- | ------------------------------------ | ------- |
| `--fuel`       | a fuel amount, `auto`, `unbounded`   | `auto`  |
| `--max-memory` | a binary size, `auto`, `unbounded`   | `auto`  |
| `--limits`     | `auto` or `unbounded` runtime preset | `auto`  |

- **Fuel** bounds matcher work — the guard against catastrophic backtracking.
  One matcher dispatch currently consumes one fuel unit.
- **Memory** bounds the VM's live execution state (frame, checkpoint, and effect
  arenas), sampled every 1,024 matcher dispatches against a fixed ceiling. The
  arenas grow on demand rather than pre-allocating, so a generous default is
  free on small inputs. It meters execution, not the separate output-rendering
  pass (see below).
- `auto` scales each ceiling with the source's node count; `unbounded` opts out.

**Sizes** use binary units only: a bare integer is bytes; `KiB`/`MiB`/`GiB`
scale by 1024. SI units (`MB`, `GB`) are rejected as ambiguous — use `MiB`/`GiB`.

**Precedence** is order-independent: `--limits` sets the baseline for both
runtime resources, and an explicit resource flag overrides that resource. So
`--limits unbounded --fuel 5` means "unbounded runtime limits except fuel = 5".

```sh
plotnik run q.ptk app.js --fuel 5000000           # explicit work ceiling
plotnik run q.ptk app.js --max-memory 256MiB      # explicit memory ceiling
plotnik run q.ptk app.js --limits unbounded       # opt out of both
```

A run that exceeds a limit stops cleanly with exit code `2` and a message
carrying a stable code (`E-out-of-fuel` / `E-limit-memory`); with `--json` the
message is a one-line JSON object instead.

There is no recursion/depth limit: backtracking and output rendering are
iterative, so deep nesting consumes heap, not the native stack. `--max-memory`
meters the VM's execution arenas during the run; output rendering happens
afterward and is not separately metered, though its size tracks the
already-bounded match journal.

---

## Input Modes

### Query-Only Commands (check, dump, infer, generate)

These commands take a single input. Use either:

- **Positional**: `plotnik dump query.ptk`
- **Flag**: `plotnik dump -q 'Q = ...'`

### Query+Source Commands (tree, run, trace, inspect)

These commands can take query, source, or both inputs. Use any combination:

| Pattern                         | Query from     | Source from    |
| ------------------------------- | -------------- | -------------- |
| `run QUERY SOURCE`              | 1st positional | 2nd positional |
| `run -q '...' SOURCE`           | `-q` flag      | positional     |
| `run -s '...' QUERY -l lang`    | positional     | `-s` flag      |
| `run -q '...' -s '...' -l lang` | `-q` flag      | `-s` flag      |

**Key rule**: When `-q` is provided with one positional, it becomes SOURCE.

Supplying the same input both ways is a usage error (exit 2): `-q` with a
query positional, or `-s` with a source positional, is rejected rather than
silently dropping the positional.

### Language Detection

Priority: explicit `-l` (must agree with the shebang) > shebang declaration >
source file extension.

| Input type        | Language                   |
| ----------------- | -------------------------- |
| `.ptk` w/ shebang | Declared in shebang        |
| Source file       | Inferred from `.ext`       |
| Inline (`-s`)     | Requires `-l` (or shebang) |

### Shebang (Language Declaration)

Line 1 of a `.ptk` file may declare its language (and optionally an
entry point) via a shebang:

```
#!/usr/bin/env -S plotnik run -l typescript
Func = (function_declaration name: (identifier) @name)
```

All commands (`run`, `check`, `infer`, `tree`, `trace`, `dump`) read the
declaration; presentation flags in the shebang are ignored unless executing.
An explicit `-l` must agree with the declaration, otherwise the command errors.

With `chmod +x`, the file becomes directly executable:

```sh
./functions.ptk app.ts
```

In a workspace directory, all shebangs must agree on the language.

---

## Reading from Stdin

Use `-` as the file argument:

```sh
# Query from stdin
echo 'Q = (identifier) @id' | plotnik dump -

# Source from stdin
cat app.ts | plotnik tree -

# Run: query from stdin, source from file
echo 'Q = (identifier) @id' | plotnik run - app.js
```

---

## Workflow Examples

### Developing a Query

1. **Explore the source syntax tree** to understand node structure:

   ```sh
   plotnik tree example.ts
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

Respects the `NO_COLOR`, `CLICOLOR_FORCE`, and `TERM=dumb` conventions.

---

## Error Messages

Plotnik provides detailed diagnostics with source context:

```
error: unknown node kind 'function_decl'
 --> query.ptk:3:5
  |
3 |     (function_decl name: (identifier) @name)
  |      ^^^^^^^^^^^^^ not a valid node kind
  |
help: did you mean 'function_declaration'?
```

Common errors:

| Error                                                    | Cause                                     | Fix                                       |
| -------------------------------------------------------- | ----------------------------------------- | ----------------------------------------- |
| `unknown node kind`                                      | Typo in node kind                         | Check `plotnik tree file` for valid kinds |
| `missing closing )`                                      | Unclosed tree pattern                     | Match parentheses                         |
| `expected expression`                                    | Invalid syntax                            | Check query syntax                        |
| `captures under a quantifier must be collected together` | Quantified captures have no item boundary | Capture the quantified group              |

---

## Exit Codes

Uniform across all commands:

| Code | Meaning                                                                   |
| ---- | ------------------------------------------------------------------------- |
| 0    | Yes/success (match found, query valid)                                    |
| 1    | Domain "no" (no match, invalid query, or selected-target capacity limit)  |
| 2    | Couldn't answer (usage, IO, invalid target configuration, internal error) |

---

## Tips

1. **Start with `tree`** to explore unfamiliar codebases
2. **Use `--include-anonymous`** to see literal and punctuation nodes
3. **Run `check`** before `infer` to catch grammar errors early
4. **Use `dump`** to debug query parsing or bytecode
5. **Use query files** for anything beyond one-liners
6. **Declare the language in a shebang** so `-l` is never needed for that file
