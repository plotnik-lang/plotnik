# Bytecode Dump Format

`plotnik dump` prints a loaded module in a stable, human-readable form. It is
intended for golden fixtures and debugging compiler output.

## Sections

The dump follows bytecode section order:

```text
[strings]
[regex]        ; only when regex predicates exist
[type_defs]
[type_members]
[type_names]
[entrypoints]
[transitions]
```

Indexes are printed with prefixes:

| Prefix | Section      |
| ------ | ------------ |
| `S`    | strings      |
| `R`    | regex        |
| `T`    | type defs    |
| `M`    | type members |
| `N`    | type names   |

## Transition Lines

Each transition line uses fixed columns:

```text
  step  nav  content                         successors
```

Examples:

```text
  00  -ε-  [StructOpen]                     02
  02       (@18)                            18 : 03
  03  -ε-  [StructClose]                    05
  05                                        ▶
  08   !   (number) [Null Set(M1) Node Set(M0)]  07
```

Instruction forms:

| Instruction | Format                                   |
| ----------- | ---------------------------------------- |
| Match       | `step nav field: (kind) [effects] succs` |
| Epsilon     | `step -ε- [effects] succs`               |
| Call        | `step nav field: (@target) target : ret` |
| Return      | `step ▶`                                 |
| Padding     | `step ...`                               |

An empty navigation column means `Stay`. `-ε-` means `Nav::Epsilon`, a distinct
mode with no cursor movement or node check.

Effects are shown in one bracket group in execution order. The group appears
after the node/predicate column and before successors.

## Navigation Symbols

| Nav            | Symbol |
| -------------- | :----: |
| Epsilon        |  -ε-   |
| Stay           |        |
| StayExact      |   !    |
| Down           |  └‣─   |
| DownSkip       |  └•─   |
| DownSkipExtras |  └◦─   |
| DownExact      |  └─!   |
| Next           |  ─‣─   |
| NextSkip       |  ─•─   |
| NextSkipExtras |  ─◦─   |
| NextExact      |  ──!   |
| Up             |  ─‣┘   |
| UpSkipTrivia   |  ─•┘   |
| UpSkipExtras   |  ─◦┘   |
| UpExact        |  !─┘   |

`Up*` levels greater than one use superscript suffixes, for example `─‣┘²`.

## Example

Query:

```plotnik
Value = (document [Num: (number) @n Str: (string) @s])
```

Dump:

```text
[strings]
S0 "Beauty will save the world"
S1 "n"
S2 "s"
S3 "Value"
S4 "document"
S5 "number"
S6 "string"

[type_defs]
T0 = <Node>
T1 = Struct  M0:2  ; { n, s }
T2 = Optional(T0)  ; <Node>?

[type_members]
M0: S1 → T2  ; n: T2
M1: S2 → T2  ; s: T2

[type_names]
N0: S3 → T1  ; Value

[entrypoints]
Value = 00 :: T1

[transitions]
Value:
  00  -ε-  [StructOpen]                     02
  02       (@18)                            18 : 03
  03  -ε-  [StructClose]                    05
  05                                        ▶
  06                                        ▶
  07  ─‣┘  _                                06
  08   !   (number) [Null Set(M1) Node Set(M0)]  07
  11   !   (string) [Null Set(M0) Node Set(M1)]  07
  14  ─‣─  _                                08, 11, 14
  16  └‣─  _                                08, 11, 14
  18   !   (document)                       16
```
