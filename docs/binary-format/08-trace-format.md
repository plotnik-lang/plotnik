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

**Text budget**: Node text is truncated to 20 characters with `…` when displayed.

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

Each instruction appears exactly as in the `dump` output:

```
  10   ▽   left: (identifier) [Node Set(M6)]      12
```

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
| `  ◀  ` | Return: back from definition      |

Navigation sub-lines show the node kind we arrived at. Match sub-lines follow, showing success (`●`) or failure (`○`) for type/field checks.

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
Ident = (identifier) @name :: string
Expression = [
    Literal: (number) @value
    Variable: (identifier) @name
]
Assignment = (assignment_expression
    left: (identifier) @target
    right: (Expression) @value)
```

### Bytecode Reference

```
[entrypoints]
Assignment = 12 :: T06
Expression = 09 :: T05
Ident      = 01 :: T02

[transitions]
  00   ε                                    ◼

Ident:
  01   ε                                    02
  02   ε   [Obj]                            04
  04       (identifier) [Text Set(M0)]      06
  06   ε   [EndObj]                         08
  08                                        ▶

Expression:
  09   ε                                    10
  10   ε                                    30, 36

Assignment:
  12   ε                                    13
  13   ε   [Obj]                            15
  15       (assignment_expression)          16
  16   ▽   left: (identifier) [Node Set(M6)]  18
  18   ▷   right: (Expression)              19 ⯇
  19   ε   [Set(M5)]                        21
  21   △                                    22
  22   ε   [EndObj]                         24
  24                                        ▶
  25                                        ▶
  26   ε   [EndEnum]                        25
  28       (number) [Node Set(M1)]          26
  30   ε   [Enum(M3)]                       28
  32   ε   [EndEnum]                        25
  34       (identifier) [Node Set(M2)]      32
  36   ε   [Enum(M4)]                       34
```

---

**Note**: The following trace examples are illustrative and use simplified step numbers for clarity. The actual step numbers in your output may differ based on the current bytecode generation. The format and sub-line conventions remain the same.

## Trace 1: Successful Match with Backtracking (`-v`)

**Entrypoint:** `Assignment`
**Source:** `x = y`

```
(assignment_expression              ; root
  left: (identifier)                ; "x"
  right: (identifier))              ; "y"
```

### Execution Trace

```
Assignment:
  08   ε                                          09
  09       (assignment_expression)                10
       ●   assignment_expression "x = y"
  10   ▽   left: (identifier) [Node Set(M6)]      12
       ▽   identifier
       ●   left:
       ●   identifier "x"
       ⬥   Node
       ⬥   Set "target"
  12   ▷   right: (Expression)                    13 ⯇
       ▷   identifier
       ●   right:
       ▶   Expression

Expression:
  05   ε                                          06
  06   ε                                          22, 28
  22   ε   [Enum(M3)]                             20
       ⬥   Enum "Literal"
  20       (number) [Node Set(M1)]                18
       ○   identifier "y"
  06  ❮❮❮
  28   ε   [Enum(M4)]                             26
       ⬥   Enum "Variable"
  26       (identifier) [Node Set(M2)]            24
       ●   identifier "y"
       ⬥   Node
       ⬥   Set "name"
  24   ε   [EndEnum]                              17
       ⬥   EndEnum
  17                                              ▶
       ◀   Expression

Assignment:
  13   ε   [Set(M5)]                              15
       ⬥   Set "value"
  15   △                                          16
       △   assignment_expression
  16                                              ▶
       ◀   Assignment
```

### Execution Summary

1. **08→09**: Epsilon entry
2. **09→10**: Match `(assignment_expression)` at root
3. **10→12**: Navigate ▽, match `left: (identifier)`, capture "x" as `@target`
4. **12→05**: Navigate ▷, check `right:`, call Expression
5. **05→06→22**: Expression entry, checkpoint at 28
6. **22→20**: Start Literal variant, try `(number)`
7. **20**: `(identifier)` found, type mismatch, backtrack to checkpoint
8. **28→26**: Start Variable variant, try `(identifier)`
9. **26→24**: `(identifier) "y"` matches, capture as `@name`
10. **24→17**: EndEnum, return from Expression
11. **13→15**: Set `@value` field
12. **15→16**: Navigate △ to root
13. **16**: Return from Assignment

---

## Trace 2: Successful Match without Backtracking (`-v`)

**Entrypoint:** `Assignment`
**Source:** `x = 1`

```
(assignment_expression
  left: (identifier)                ; "x"
  right: (number))                  ; "1"
```

### Execution Trace

```
Assignment:
  08   ε                                          09
  09       (assignment_expression)                10
       ●   assignment_expression "x = 1"
  10   ▽   left: (identifier) [Node Set(M6)]      12
       ▽   identifier
       ●   left:
       ●   identifier "x"
       ⬥   Node
       ⬥   Set "target"
  12   ▷   right: (Expression)                    13 ⯇
       ▷   number
       ●   right:
       ▶   Expression

Expression:
  05   ε                                          06
  06   ε                                          22, 28
  22   ε   [Enum(M3)]                             20
       ⬥   Enum "Literal"
  20       (number) [Node Set(M1)]                18
       ●   number "1"
       ⬥   Node
       ⬥   Set "value"
  18   ε   [EndEnum]                              17
       ⬥   EndEnum
  17                                              ▶
       ◀   Expression

Assignment:
  13   ε   [Set(M5)]                              15
       ⬥   Set "value"
  15   △                                          16
       △   assignment_expression
  16                                              ▶
       ◀   Assignment
```

First branch (Literal) matches immediately—checkpoint at 28 is never used.

---

## Trace 3: Failed Match (`-v`)

**Entrypoint:** `Expression`
**Source:** `"hello"`

```
(string)                            ; string literal, not number or identifier
```

### Execution Trace

```
Expression:
  05   ε                                          06
  06   ε                                          22, 28
  22   ε   [Enum(M3)]                             20
       ⬥   Enum "Literal"
  20       (number) [Node Set(M1)]                18
       ○   string "hello"
  06  ❮❮❮
  28   ε   [Enum(M4)]                             26
       ⬥   Enum "Variable"
  26       (identifier) [Node Set(M2)]            24
       ○   string "hello"
```

Both branches fail. No more checkpoints—query does not match. The CLI exits with code 1.

---

## Trace 4: Text Effect (String Capture) (`-v`)

**Entrypoint:** `Ident`
**Source:** `foo`

```
(identifier)                        ; "foo"
```

### Execution Trace

```
Ident:
  01   ε                                          02
  02       (identifier) [Text Set(M0)]            04
       ●   identifier "foo"
       ⬥   Text
       ⬥   Set "name"
  04                                              ▶
```

The `Text` effect extracts the node's source text as a string (from `@name :: string`).

---

## Trace 5: Search with Skipping (`-v`)

To demonstrate skip behavior, consider a different query:

```
ReturnVal = (statement_block (return_statement) @ret)
```

**Bytecode:**

```
ReturnVal:
  01   ε                                          02
  02       (statement_block)                      03
  03   ▽   (return_statement) [Node Set(M0)]      04
  04   △                                          05
  05                                              ◼
```

**Entrypoint:** `ReturnVal`
**Source:** `{ x; return 1; }`

```
(statement_block
  (expression_statement)           ; "x;"
  (return_statement))              ; "return 1;"
```

### Execution Trace

```
ReturnVal:
  01   ε                                          02
  02       (statement_block)                      03
       ●   statement_block "{ x; return 1; }"
  03   ▽   (return_statement) [Node Set(M0)]      04
       ▽   expression_statement
       ○   expression_statement "x;"
       ▷   return_statement
       ●   return_statement "return 1;"
       ⬥   Node
       ⬥   Set "ret"
  04   △                                          05
       △   statement_block
  05                                              ◼
```

The `▽` lands on `(expression_statement)`, type mismatch, skip `▷` to next sibling, find `(return_statement)`.

---

## Trace 6: Immediate Failure (`-v`)

**Entrypoint:** `Assignment`
**Source:** `42`

```
(number)                           ; just a number literal
```

### Execution Trace

```
Assignment:
  08   ε                                          09
  09       (assignment_expression)                10
       ○   number "42"
```

Type check fails at root—no navigation occurs. The CLI exits with code 1.

---

## Trace 7: Suppressive Capture (`-v`)

Suppressive captures (`@_`) match structurally but don't emit effects. The trace shows:

- `⬥ SuppressBegin` / `⬥ SuppressEnd` when entering/exiting suppression
- `⬦` for data effects that are suppressed
- `⬦ SuppressBegin` / `⬦ SuppressEnd` for nested suppression (already inside another `@_`)

**Query:**

```
Pair = (pair key: (string) @_ value: (number) @value)
```

**Entrypoint:** `Pair`
**Source:** `"x": 1`

```
(pair
  key: (string)                    ; "x"
  value: (number))                 ; 1
```

### Execution Trace

```
Pair:
  01   ε                                          02
  02   ε   [Obj]                                  04
       ⬥   Obj
  04       (pair)                                 05
       ●   pair "\"x\": 1"
  05   ▽   key: (string) [SuppressBegin]          06
       ▽   string
       ●   key:
       ●   string "\"x\""
       ⬥   SuppressBegin
  06   ε   [SuppressEnd]                          08
       ⬦   Node
       ⬦   Set "key"
       ⬥   SuppressEnd
  08   ▷   value: (number) [Node Set(M0)]         10
       ▷   number
       ●   value:
       ●   number "1"
       ⬥   Node
       ⬥   Set "value"
  10   △                                          12
       △   pair
  12   ε   [EndObj]                               14
       ⬥   EndObj
  14                                              ◼
```

The `@_` capture on `key:` wraps its inner effects with `SuppressBegin`/`SuppressEnd`. Effects between them (`Node`, `Set "key"`) appear as `⬦` (suppressed). The `@value` capture emits normally with `⬥`.

---

## Trace 8: Default Verbosity (Compact)

Same as Trace 1 but with default verbosity (no `-v` flag). Navigation and effect sub-lines are hidden:

```
Assignment:
  08   ε                                          09
  09       (assignment_expression)                10
       ●   assignment_expression
  10   ▽   left: (identifier) [Node Set(M6)]      12
       ●   identifier
  12   ▷   right: (Expression)                    13 ⯇
       ▶   Expression

Expression:
  05   ε                                          06
  06   ε                                          22, 28
  22   ε   [Enum(M3)]                             20
  20       (number) [Node Set(M1)]                18
       ○   identifier
  06  ❮❮❮
  28   ε   [Enum(M4)]                             26
  26       (identifier) [Node Set(M2)]            24
       ●   identifier
  24   ε   [EndEnum]                              17
  17                                              ▶
       ◀   Expression

Assignment:
  13   ε   [Set(M5)]                              15
  15   △                                          16
       ●   assignment_expression
  16                                              ▶
       ◀   Assignment
```

Default shows:

- Match results (`●`, `○`) with kind only, no text
- Backtrack (`❮❮❮`)
- Call/return (`▶`, `◀`)

Hidden:

- Navigation sub-lines (`▽`, `▷`, `△`)
- Effect sub-lines (`⬥`, `⬦`)

---

## Sub-Line Reference

| Symbol  | Format              | Example                      |
| ------- | ------------------- | ---------------------------- |
| (blank) | `     kind`         | `     identifier`            |
| `  ▽  ` | `▽   kind`          | `▽   identifier`             |
| `  ▽  ` | `▽   kind "text"`   | `▽   identifier "foo"`       |
| `  ▷  ` | `▷   kind`          | `▷   return_statement`       |
| `  △  ` | `△   kind`          | `△   assignment_expression`  |
| `  ●  ` | `●   kind`          | `●   identifier`             |
| `  ●  ` | `●   kind "text"`   | `●   identifier "foo"`       |
| `  ●  ` | `●   field:`        | `●   left:`                  |
| `  ○  ` | `○   kind`          | `○   string`                 |
| `  ⬥  ` | `⬥   Effect`        | `⬥   Node`                   |
| `  ⬥  ` | `⬥   Set "field"`   | `⬥   Set "target"`           |
| `  ⬥  ` | `⬥   Enum "var"`    | `⬥   Enum "Literal"`         |
| `  ⬥  ` | `⬥   SuppressBegin` | `⬥   SuppressBegin`          |
| `  ⬥  ` | `⬥   SuppressEnd`   | `⬥   SuppressEnd`            |
| `  ⬦  ` | `⬦   Effect`        | `⬦   Node` (suppressed)      |
| `  ⬦  ` | `⬦   SuppressBegin` | `⬦   SuppressBegin` (nested) |
| `  ▶  ` | `▶   Name`          | `▶   Expression`             |
| `  ◀  ` | `◀   Name`          | `◀   Expression`             |

### Backtrack (Instruction-Level)

```
  NN  ❮❮❮
```

Step number `NN` is the checkpoint we're restoring to. Appears as an instruction line, not a sub-line.

## Nav Symbols

| Nav             | Symbol  | Meaning                         |
| --------------- | ------- | ------------------------------- |
| Stay            | (space) | No movement                     |
| Stay (epsilon)  | ε       | No movement, no constraints     |
| StayExact       | !!!     | Stay at position, exact only    |
| Down            | ▽       | First child, skip any           |
| DownSkip        | !▽      | First child, skip trivia        |
| DownExact       | !!▽     | First child, exact              |
| Next            | ▷       | Next sibling, skip any          |
| NextSkip        | !▷      | Next sibling, skip trivia       |
| NextExact       | !!▷     | Next sibling, exact             |
| Up(1)           | △       | Ascend 1 level (no superscript) |
| Up(n≥2)         | △ⁿ      | Ascend n levels                 |
| UpSkipTrivia(n) | !△ⁿ     | Ascend n, last non-trivia       |
| UpExact(n)      | !!△ⁿ    | Ascend n, last child            |

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
