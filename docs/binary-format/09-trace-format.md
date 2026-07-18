# Execution Trace Format

`plotnik trace` prints the instruction stream as it executes. It reuses the dump
line format and adds sub-lines for navigation, match results, effects, calls,
and backtracking.

## Command

```sh
plotnik trace query.ptk source.js
plotnik trace -q 'Q = (program)' -s 'x;' -l javascript -v
plotnik trace query.ptk source.js --fuel 10000
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

`(?)` is a call to an internal body label that has no user definition name.
Returns show `◀`; top-level return shows `◼`.

## Sub-Lines

Sub-lines leave the address column blank and use the symbol column for the event:

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

Scalar capture types use the same effect sub-lines. Their stable spellings are
`ScalarOpen`, `ScalarMark`, `TextClose`, and `BoolClose(false)` /
`BoolClose(true)`; direct values use `NodeText`, `NodeBool`, and
`BoolValue(false)` / `BoolValue(true)`. A scalar mark's node is available to structured trace and
inspection output even though the concise text trace prints only the effect
name. Backtracking truncates marks and restores scalar-frame depth with the
rest of the checkpointed match journal.

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
       ⬥   RecordOpen
  00       (document)                       01
       !   document
       ●   document 42
  01       _                                04, 08, 11
      └─!  number
       ●   number 42
  04       (number) [Absent RecordSet(M1) Node RecordSet(M0)]  07
       !   number
       ●   number 42
       ⬥   Absent
       ⬥   RecordSet "s"
       ⬥   Node
       ⬥   RecordSet "n"
  07       _                                03
      ─‣┘  document
       ●   document 42
  03   ◀   (Value)                          ◼
       ⬥   RecordClose
```

Default verbosity hides navigation and effect sub-lines but keeps match
success/failure, calls, returns, and backtracking.
