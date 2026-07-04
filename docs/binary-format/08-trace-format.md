# Execution Trace Format

`plotnik trace` prints the instruction stream as it executes. It reuses the dump
line format and adds sub-lines for navigation, match results, effects, calls,
and backtracking.

## Command

```sh
plotnik trace query.ptk source.js
plotnik trace -q 'Q = (program)' -s 'x;' -l javascript -v
plotnik trace query.ptk source.js --max-steps 10000
```

## Verbosity

| Level   | Sub-lines                     | Node Text         |
| ------- | ----------------------------- | ----------------- |
| default | match, backtrack, call/return | kind only         |
| `-v`    | all                           | on match/failure  |
| `-vv`   | all                           | on all navigation |

## Instruction Lines

Instruction lines are the same shape as `dump`:

```text
  18       (document)                       16
  02       (?)                              18 : 03
  06   ◀   (?)
```

`(?)` is a call to an internal wrapper/body label that has no user definition
name. Returns show `◀`; top-level return shows `◼`.

## Sub-Lines

Sub-lines leave the step column blank and use the symbol column for the event:

| Symbol | Meaning                   |
| ------ | ------------------------- |
| blank  | Stayed at position        |
| `└‣─`  | Descended to child        |
| `─‣─`  | Moved to sibling          |
| `─‣┘`  | Ascended to parent        |
| `●`    | Match success             |
| `○`    | Match failure             |
| `⬥`    | Effect emitted            |
| `⬦`    | Effect suppressed by `@_` |
| `▶`    | Entered a call            |

Backtracking is an instruction-level line:

```text
  08  ❮❮❮
```

## Example

Query:

```plotnik
Value = (document [Num: (number) @n Str: (string) @s])
```

Source:

```json
42
```

Trace with `-v --no-result`:

```text
Value:
  00  -ε-  [StructOpen]                     02
       ⬥   StructOpen
  02       (?)                              18 : 03
       ▶   (?)

?:
  --------------------------------------------
  18       (document)                       16
       !   document
       ●   document 42
  16       _                                08, 11, 14
      └‣─  number
       ●   number 42
  --------------------------------------------
  08       (number) [Null Set(M1) Node Set(M0)]  07
       !   number
       ●   number 42
       ⬥   Null
       ⬥   Set "s"
       ⬥   Node
       ⬥   Set "n"
  --------------------------------------------
  07       _                                06
      ─‣┘  document
       ●   document 42
  06   ◀   (?)

Value:
  03  -ε-  [StructClose]                    05
       ⬥   StructClose
  05   ◀   (Value)                          ◼
```

Default verbosity hides navigation and effect sub-lines but keeps match
success/failure, calls, returns, and backtracking.
