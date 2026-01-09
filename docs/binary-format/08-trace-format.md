# Execution Trace Format

The `trace` command provides step-by-step execution visualization for debugging queries.

## Command Usage

```sh
plotnik trace query.ptk source.js
plotnik trace -q 'Pattern = (identifier) @id' -l javascript --source 'let x = 1'
plotnik trace query.ptk source.js -v          # moderate verbosity
plotnik trace query.ptk source.js -vv         # maximum verbosity
plotnik trace query.ptk source.js --fuel 10000
```

## Verbosity Levels

| Level     | Sub-lines                     | Node Text          | Audience       |
| --------- | ----------------------------- | ------------------ | -------------- |
| (default) | match, backtrack, call/return | kind only          | LLM, CI        |
| `-v`      | all                           | on match/failure   | Developer      |
| `-vv`     | all                           | on all (incl. nav) | Deep debugging |

**Text budget**: Node text fills the line up to the successors column (minimum 12 characters). Truncated text ends with `…`.

---

## Trace Format

The trace output reuses the bytecode dump format, adding sub-lines that show execution dynamics: navigation results, type checks, effect emissions, and branching decisions.

### Column Layout

Same as dump format:

```
| 2 | step | 1 |   5   | 1 | content              | 1 | succ |
|   | pad  |   | (ctr) |   |                      |   |      |
```

- **Step padding**: Dynamic based on max step in graph
- **Symbol column**: All symbols centered in 5 characters

### Instruction Line

Each instruction shows a simplified view compared to `dump`:

```
  12       (program)                        13
  13       (B)                              01 : 14
  08   ◀   (B)
```

**Match instructions** show empty symbol column (nav appears in sublines). **Call instructions** show `(Name)` with `target : return` successors. **Return instructions** show `◀` with `(Name)`.

### Sub-Lines

Below each instruction, sub-lines show what happened during execution. Each sub-line uses the same column layout with the step number area blank:

| Symbol  | Meaning                           |
| ------- | --------------------------------- |
| (blank) | Navigation: stayed at position    |
| `  ▽  ` | Navigation: descended to child    |
| `  ▷  ` | Navigation: moved to sibling      |
| `  △  ` | Navigation: ascended to parent    |
| `  ●  ` | Match: success                    |
| `  ○  ` | Match: failure                    |
| `  ⬥  ` | Effect: data capture or structure |
| `  ⬦  ` | Effect: suppressed (inside @\_)   |
| `  ▶  ` | Call: entering definition         |

Navigation symbols (`▽`, `▷`, `△`) appear only in sub-lines, not on instruction lines. Match sub-lines show success (`●`) or failure (`○`) for type/field checks.

### Return Line

Return is an instruction-level line showing the definition being returned from:

```
  08   ◀   (B)
  17   ◀   (A)                              ◼
```

The `◀` symbol appears in the symbol column with the definition name in parentheses. Top-level returns (empty call stack) show `◼` as successor; mid-stack returns have no successor.

### Definition Labels

Definition labels (`Name:`) appear at:

- Entry to a definition (initial or via call)
- After returning from a call (showing the caller's name)

```
A:
  09   ε                                    10
  ...
  13       (B)                              01 : 14
       ▶   (B)
B:
  01   ε                                    02
  ...
  08   ◀   (B)
A:
  14                                        15
```

### Backtrack Line

Backtrack is an instruction-level line (not a sub-line) showing checkpoint restoration:

```
  06  ❮❮❮
```

The step number indicates _where_ we're restoring to. `❮❮❮` is centered in the 5-char symbol column (`❮❮❮`).

---

## Example Query

From `07-dump-format.md`:

```
Value = (document [
    Num: (number) @n
    Str: (string) @s
])
```

Run: `plotnik trace -q '<query>' -s '<source>' -l json -v`

### Bytecode Reference

```
[entrypoints]
Value = 06 :: T3

[transitions]
_ObjWrap:
  00   ε   [Obj]                            02
  02       Trampoline                       03
  03   ε   [EndObj]                         05
  05                                        ▶

Value:
  06   ε                                    07
  07   !   (document)                       08
  08   ε                                    11, 16
  10                                        ▶
  11 !!▽   [Enum(M2)] (number) [Node Set(M0) EndEnum]  19
  14  ...
  15  ...
  16 !!▽   [Enum(M3)] (string) [Node Set(M1) EndEnum]  19
  19   △   _                                10
```

---

## Trace 1: Successful Match on First Branch (`-v`)

**Source:** `42` (JSON number)

```
(document
  (number "42"))
```

### Execution Trace

```
_ObjWrap:
  00   ε   [Obj]                            02
       ⬥   Obj
  02       Trampoline                       03
       ▶   (Value)

Value:
  06   ε                                    07
  07       (document)                       08
           document
       ●   document 42
  --------------------------------------------
  08   ε                                    11, 16
  11       [Enum(M2)] (number) [Node Set(M0) EndEnum]  19
       ⬥   Enum "Num"
       ▽   number
       ●   number 42
       ⬥   Node
       ⬥   Set "n"
       ⬥   EndEnum
  --------------------------------------------
  19       _                                10
       △   document
       ●   document 42
  --------------------------------------------
  10   ◀   (Value)

_ObjWrap:
  --------------------------------------------
  03   ε   [EndObj]                         05
       ⬥   EndObj
  05   ◀   _ObjWrap                         ◼
```

First branch (`Num`) matches — checkpoint at step 16 is never used.

---

## Trace 2: Successful Match with Backtracking (`-v`)

**Source:** `"hello"` (JSON string)

```
(document
  (string "\"hello\""))
```

### Execution Trace

```
_ObjWrap:
  00   ε   [Obj]                            02
       ⬥   Obj
  02       Trampoline                       03
       ▶   (Value)

Value:
  06   ε                                    07
  07       (document)                       08
           document
       ●   document "hello"
  --------------------------------------------
  08   ε                                    11, 16
  11       [Enum(M2)] (number) [Node Set(M0) EndEnum]  19
       ⬥   Enum "Num"
       ▽   string
       ○   string "hello"
  08  ❮❮❮
  --------------------------------------------
  16       [Enum(M3)] (string) [Node Set(M1) EndEnum]  19
       ⬥   Enum "Str"
       ▽   string
       ●   string "hello"
       ⬥   Node
       ⬥   Set "s"
       ⬥   EndEnum
  19       _                                10
       △   document
       ●   document "hello"
  --------------------------------------------
  10   ◀   (Value)

_ObjWrap:
  --------------------------------------------
  03   ε   [EndObj]                         05
       ⬥   EndObj
  05   ◀   _ObjWrap                         ◼
```

### Execution Summary

1. **00→02**: Preamble starts, emit `Obj`
2. **02→Value**: `Trampoline` dispatches to entrypoint
3. **07→08**: Match `(document)` succeeds
4. **08**: Branch — create checkpoint at 16, try 11 first
5. **11**: Try `Num` branch, navigate down, find `string` — type mismatch (`○`)
6. **08 ❮❮❮**: Backtrack to checkpoint
7. **16**: Try `Str` branch, navigate down, find `string` — match (`●`)
8. **19→10**: Navigate up, return from `Value`
9. **03→05**: Preamble cleanup, emit `EndObj`, accept (`◼`)

---

## Trace 3: Failed Match (`-v`)

**Source:** `true` (JSON boolean — neither number nor string)

```
(document
  (true "true"))
```

### Execution Trace

```
_ObjWrap:
  00   ε   [Obj]                            02
       ⬥   Obj
  02       Trampoline                       03
       ▶   (Value)

Value:
  06   ε                                    07
  07       (document)                       08
           document
       ●   document true
  --------------------------------------------
  08   ε                                    11, 16
  11       [Enum(M2)] (number) [Node Set(M0) EndEnum]  19
       ⬥   Enum "Num"
       ▽   true
       ○   true true
  08  ❮❮❮
  --------------------------------------------
  16       [Enum(M3)] (string) [Node Set(M1) EndEnum]  19
       ⬥   Enum "Str"
       ▽   true
       ○   true true
```

Both branches fail. No more checkpoints — query does not match. The CLI exits with code 1.

---

## Trace 4: Default Verbosity (Compact)

Same as Trace 2 but with default verbosity (no `-v` flag). Navigation and effect sub-lines are hidden:

```
_ObjWrap:
  00   ε   [Obj]                            02
  02       Trampoline                       03
       ▶   (Value)

Value:
  06   ε                                    07
  07       (document)                       08
       ●   document
  08   ε                                    11, 16
  11       [Enum(M2)] (number) [Node Set(M0) EndEnum]  19
       ○   string
  08  ❮❮❮
  16       [Enum(M3)] (string) [Node Set(M1) EndEnum]  19
       ●   string
  19       _                                10
       ●   document
  10   ◀   (Value)

_ObjWrap:
  03   ε   [EndObj]                         05
  05   ◀   _ObjWrap                         ◼
```

Default shows:

- Match results (`●`, `○`) with kind only, no text
- Backtrack (`❮❮❮`)
- Call (`▶`) and return (`◀`)

Hidden:

- Navigation sub-lines (`▽`, `▷`, `△`)
- Effect sub-lines (`⬥`, `⬦`)

---

## Sub-Line Reference

| Symbol  | Format              | Example                      |
| ------- | ------------------- | ---------------------------- |
| (blank) | `     kind`         | `     identifier`            |
| `  ▽  ` | `▽   kind`          | `▽   identifier`             |
| `  ▽  ` | `▽   kind text`     | `▽   identifier foo`         |
| `  ▷  ` | `▷   kind`          | `▷   return_statement`       |
| `  △  ` | `△   kind`          | `△   assignment_expression`  |
| `  ●  ` | `●   kind`          | `●   identifier`             |
| `  ●  ` | `●   kind text`     | `●   identifier foo`         |
| `  ●  ` | `●   field:`        | `●   left:`                  |
| `  ○  ` | `○   kind`          | `○   string`                 |
| `  ⬥  ` | `⬥   Effect`        | `⬥   Node`                   |
| `  ⬥  ` | `⬥   Set "field"`   | `⬥   Set "target"`           |
| `  ⬥  ` | `⬥   Enum "var"`    | `⬥   Enum "Literal"`         |
| `  ⬥  ` | `⬥   SuppressBegin` | `⬥   SuppressBegin`          |
| `  ⬥  ` | `⬥   SuppressEnd`   | `⬥   SuppressEnd`            |
| `  ⬦  ` | `⬦   Effect`        | `⬦   Node` (suppressed)      |
| `  ⬦  ` | `⬦   SuppressBegin` | `⬦   SuppressBegin` (nested) |
| `  ▶  ` | `▶   (Name)`        | `▶   (Expression)`           |

### Backtrack (Instruction-Level)

```
  NN  ❮❮❮
```

Step number `NN` is the checkpoint we're restoring to. Appears as an instruction line, not a sub-line.

## Nav Symbols

In trace output, navigation symbols are **simplified** — skip/exact variants are not distinguished:

| Nav                          | Symbol  | Meaning                      |
| ---------------------------- | ------- | ---------------------------- |
| Epsilon                      | ε       | Pure control flow, no cursor |
| Stay, StayExact              | (space) | No movement                  |
| Down, DownSkip, DownExact    | ▽       | Descended to child           |
| Next, NextSkip, NextExact    | ▷       | Moved to sibling             |
| Up(n), UpSkipTrivia, UpExact | △       | Ascended to parent           |

> **Note**: For detailed nav symbols with mode modifiers (`!▽`, `!!▽`, etc.), see [07-dump-format.md](07-dump-format.md#nav-symbols). Trace format simplifies these for readability.

## Effects

| Effect         | Description                    |
| -------------- | ------------------------------ |
| Node           | Capture matched node           |
| Text           | Extract node text as string    |
| Set "field"    | Assign to struct field         |
| Enum "variant" | Start tagged union variant     |
| EndEnum        | End tagged union variant       |
| Arr            | Start array                    |
| Push           | Push to array                  |
| EndArr         | End array                      |
| Obj            | Start object                   |
| EndObj         | End object                     |
| Null           | Null value                     |
| Clear          | Clear pending value            |
| SuppressBegin  | Enter suppression scope (`@_`) |
| SuppressEnd    | Exit suppression scope         |

## Command Options

| Option         | Description                             |
| -------------- | --------------------------------------- |
| `-v`           | Moderate verbosity (all sub-lines)      |
| `-vv`          | Maximum verbosity (text on navigation)  |
| `--fuel N`     | Set execution fuel limit (default: 1M)  |
| `--entry NAME` | Select entrypoint for multi-def queries |

## Files

- `crates/plotnik-cli/src/commands/trace.rs` — Command implementation
- `crates/plotnik-lib/src/engine/trace.rs` — Tracer trait and PrintTracer
