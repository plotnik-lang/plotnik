# Bytecode Dump Format

`plotnik dump` renders the VM's transient bytecode as stable, human-readable
text. It is intended for learning, golden fixtures, and compiler debugging.
The command does not create or consume a bytecode artifact.

## Sections

The dump prints sections in this fixed order (matching the wire layout, except
`[spans]` — the final wire section — which is grouped with the other tables so
`[transitions]` stays last):

```text
[strings]
[regex]        ; only when regex predicates exist
[type_defs]
[type_members]
[type_names]
[entrypoints]
[spans]       ; only when inspection spans exist
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
| `P`    | spans        |

## Span Lines

When present, the `[spans]` section prints one line per span id:

```text
P00 def        0..42  src0  T03
P07 capture    12..17  src0  T04.M02
P09 pattern    4..14  src0
```

The optional `Txx` / `Txx.Myy` suffix is the type or member binding stored in
the bytecode entry. The section is omitted when `spans_count == 0`.

For degraded inspection modules, only admitted span ids are printed. Dropped
detail tiers have no `Pxx` line and no corresponding span effects.

## Transition Lines

Each transition line uses fixed columns:

```text
  step  nav  content                         successors
```

Examples:

```text
  00  -ε-  [RecordOpen]                     02
  02       (@18)                            18 : 03
  03  -ε-  [RecordClose]                    05
  05                                        ▶
  08   !   (number) [Absent RecordSet(M1) Node RecordSet(M0)]  07
```

Instruction forms:

| Instruction | Format                                   |
| ----------- | ---------------------------------------- |
| Match       | `step nav field: (kind) [effects] succs` |
| Epsilon     | `step -ε- [effects] succs`               |
| Call        | `step nav field: (@target) target : ret` |
| RoutedCall  | `step (@target) target : ret`            |
| SplitCall   | `step (@target) target : matched / zero` |
| Return      | `step ▶` or `step ▶ zero`                |
| Padding     | `step ...`                               |

An empty navigation column means `Stay`. `-ε-` means `Nav::Epsilon`, a distinct
mode with no cursor movement or node check.

Effects are shown in one bracket group in execution order. The group appears
after the node/predicate column and before successors.

Inspection effects render as `SpanStartAt#5`, `SpanStart#5`, and `SpanEnd#5`.
Scalar effects use the stable names `ScalarOpen`, `ScalarMark`, `StrClose`, and
`BoolClose(0)` / `BoolClose(1)`; direct node scalars use `NodeStr` and
`NodeBool`; provenance-free booleans use `BoolValue(0)` / `BoolValue(1)`. Primitive type definitions render as
`<Text>` and `<Bool>`.

## Navigation Symbols

| Nav                 | Symbol |
| ------------------- | :----: |
| Epsilon             |  -ε-   |
| Stay                |        |
| StayExact           |   !    |
| Down                |  └‣─   |
| DownSkip            |  └•─   |
| DownSkipExtras      |  └◦─   |
| DownExact           |  └─!   |
| Next                |  ─‣─   |
| NextSkip            |  ─•─   |
| NextSkipExtras      |  ─◦─   |
| NextExact           |  ──!   |
| Up                  |  ─‣┘   |
| UpSkipTrivia        |  ─•┘   |
| UpSkipExtras        |  ─◦┘   |
| UpExact             |  !─┘   |
| ChildlessSkipTrivia |  └•┘   |
| ChildlessSkipExtras |  └◦┘   |
| ChildlessExact      |  └!┘   |

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
T1 = Record  M0:2  ; { n, s }
T2 = Option(T0)  ; <Node>?

[type_members]
M0: S1 → T2  ; n: T2
M1: S2 → T2  ; s: T2

[type_names]
N0: S3 → T1  ; Value

[entrypoints]
Value = 00 :: T1

[transitions]
Value:
  00  -ε-  [RecordOpen]                     02
  02       (@18)                            18 : 03
  03  -ε-  [RecordClose]                    05
  05                                        ▶
  06                                        ▶
  07  ─‣┘  _                                06
  08   !   (number) [Absent RecordSet(M1) Node RecordSet(M0)]  07
  11   !   (string) [Absent RecordSet(M0) Node RecordSet(M1)]  07
  14  ─‣─  _                                08, 11, 14
  16  └‣─  _                                08, 11, 14
  18   !   (document)                       16
```
