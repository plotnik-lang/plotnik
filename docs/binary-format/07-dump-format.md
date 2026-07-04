# Bytecode Dump Format

The `dump` command displays compiled bytecode in a human-readable format.

## Example Query

```
Value = (document [
    Num: (number) @n
    Str: (string) @s
])
```

Run: `plotnik dump -q '<query>' -l json`

## Bytecode Dump

**Epsilon transitions** (`-ε-`) succeed unconditionally without cursor interaction.
They are identified by `nav == Epsilon` — a distinct navigation mode (not Stay).

**Capture effect consolidation**: Scalar capture effects (`Node`, `Set`) are
placed directly on match instructions rather than in separate epsilon steps. Structural
effects (`StructOpen`, `StructClose`, `ArrayOpen`, `ArrayClose`, `EnumOpen`, `EnumClose`) may appear in epsilons or
consolidated into match instructions.

```
[strings]
S0 "Beauty will save the world"
S1 "n"
S2 "s"
S3 "Num"
S4 "Str"
S5 "Value"
S6 "document"
S7 "number"
S8 "string"

[type_defs]
T0 = <Node>
T1 = Struct  M0:1  ; { n }
T2 = Struct  M1:1  ; { s }
T3 = Enum    M2:2  ; Num | Str

[type_members]
M0: S1 → T0  ; n: <Node>
M1: S2 → T0  ; s: <Node>
M2: S3 → T1  ; Num: T1
M3: S4 → T2  ; Str: T2

[type_names]
N0: S5 → T3  ; Value

[entrypoints]
Value = 06 :: T3

[transitions]
_StructWrap:
  00  -ε-  [StructOpen]                     02
  02       Trampoline                       03
  03  -ε-  [StructClose]                    05
  05                                        ▶

Value:
  06   !   (document)                       08
  07  ...
  08  └‣─  _                                11, 16, 19
  10                                        ▶
  11   !   [EnumOpen(M2)] (number) [Node Set(M0) EnumClose]  14
  14  ─‣┘  _                                10
  15  ...
  16   !   [EnumOpen(M3)] (string) [Node Set(M1) EnumClose]  14
  19  ─‣─  _                                11, 16, 19
```

### Sections Explained

- **`_StructWrap`**: Universal entry preamble. Wraps all entrypoints with `StructOpen`/`StructClose` and dispatches via `Trampoline`.
- **`Value`**: The compiled query definition. Step 08 searches the document children, tries `Num` (step 11) or `Str` (step 16), and uses step 19 to advance to the next candidate on backtracking.
- **`...`**: Padding slots (multi-step instructions occupy consecutive step IDs).

### Regex Section

When the query contains regex predicates (`=~` or `!~`), a `[regex]` section appears after `[strings]`:

```
[regex]
R1 /pattern/
R2 /another.*/
```

Format: `R<id> /<pattern>/`

- Index 0 is reserved, so regex IDs start at 1
- Patterns are displayed from the string table for readability
- In transitions, predicates reference patterns inline: `(identifier) =~ /foo/`

## Files

- `crates/plotnik-lib/src/bytecode/dump.rs` — Dump formatting logic
- `crates/plotnik-lib/src/bytecode/format.rs` — Shared formatting utilities

## Instruction Format

Each line follows a fixed column layout:

```
| 2 | step | 1 |   5   | 1 | content              | 1 | succ |
|   | pad  |   | (sym) |   |                      |   |      |
```

| Column  | Width    | Description                                               |
| ------- | -------- | --------------------------------------------------------- |
| indent  | 2        | Leading spaces                                            |
| step    | variable | Step number, zero-padded to max step width                |
| gap     | 1        | Space separator                                           |
| symbol  | 5+       | Nav symbol, usually 5 chars; multi-digit up counts extend |
| gap     | 1        | Space separator                                           |
| content | variable | Instruction content                                       |
| gap     | 1        | Space separator                                           |
| succ    | variable | Successors/markers, right-aligned                         |

**Step padding**: Dynamic based on max step in graph. Steps 0–9 use 1 digit, 0–99 use 2 digits, etc.

**Symbol column**:

```
| entry | policy | exit |
```

Navigation symbols have three slots:

- **Entry**: `└` enters from a parent (down); `─` is lateral movement; `!` marks the pre-ascent exact check for `UpExact`.
- **Policy**: `‣` skips any node, `•` skips trivia, `◦` skips extras only, and `─` means no skip policy.
- **Exit**: `┘` exits to a parent (up); `!` marks exact adjacency at the destination for `DownExact` and `NextExact`.
- **Superscript suffix**: Real superscript digits extend `Up(n)` symbols when `n >= 2`.

`-ε-` is the whole symbol for epsilon, not a policy glyph. Exact navigation has no skip glyph: `└─!` and `──!` put `!` at the destination; `!─┘` puts `!` at the pre-ascent position.

Examples:

- `-ε-` — epsilon (no movement)
- `└‣─` — down, skip any
- `─‣─` — next, skip any
- `─‣┘` — up 1 level (no superscript)
- `└•─` — down, skip trivia
- `─◦─` — next, skip extras only
- `──!` — next, exact
- `─‣┘¹²` — up 12 levels

| Instruction      | Format                                          |
| ---------------- | ----------------------------------------------- |
| Match (terminal) | `step nav    [pre] (type) [post]      ◼`        |
| Match            | `step nav    [pre] field: (type) [post] succ`   |
| Match (branch)   | `step nav    [pre] (type) [post]      s1, s2`   |
| Epsilon          | `step -ε-    [effects]                succ`     |
| Call             | `step nav    field: (Name)        target : ret` |
| Return           | `step                                 ▶`        |
| Trampoline       | `step        Trampoline               succ`     |

Successors aligned in right column. Omit empty `[pre]`, `[post]`, `(type)`, `field:`.

Effects in `[pre]` execute before match attempt; effects in `[post]` execute after successful match. Any effect can appear in either position.

## Nav Symbols

| Nav             | Symbol | Notes                                         |
| --------------- | :----: | --------------------------------------------- |
| Epsilon         |  -ε-   | Pure control flow, no cursor check            |
| Stay            |        | No movement, fill with spaces                 |
| StayExact       |   !    | No movement, exact match only                 |
| Down            |  └‣─   | First child, skip any                         |
| DownSkip        |  └•─   | First child, skip trivia                      |
| DownSkipExtras  |  └◦─   | First child, skip extras only                 |
| DownExact       |  └─!   | First child, exact                            |
| Next            |  ─‣─   | Next sibling, skip any                        |
| NextSkip        |  ─•─   | Next sibling, skip trivia                     |
| NextSkipExtras  |  ─◦─   | Next sibling, skip extras only                |
| NextExact       |  ──!   | Next sibling, exact                           |
| Up(1)           |  ─‣┘   | Skip any and ascend 1 level (no superscript)  |
| Up(2)           |  ─‣┘²  | Skip any and ascend 2 levels                  |
| Up(12)          | ─‣┘¹²  | Skip any and ascend 12 levels                 |
| UpSkipTrivia(2) |  ─•┘²  | Ascend 2 levels, each only if trivia remains  |
| UpSkipExtras(2) |  ─◦┘²  | Ascend 2 levels, each only if extras remain   |
| UpExact(2)      |  !─┘²  | Ascend 2 levels, each only if nothing remains |

**Note**: `-ε-` appears for `Nav::Epsilon` — a distinct mode from `Stay`. A step with `nav == Stay` but with type constraints (e.g., `(identifier)`) shows blank, not `-ε-`.

## Effects

| Effect        | Description            |
| ------------- | ---------------------- |
| StructOpen    | Start struct           |
| StructClose   | End struct             |
| ArrayOpen     | Start array            |
| ArrayClose    | End array              |
| Push          | Push to array          |
| EnumOpen(Mxx) | Start enum variant Mxx |
| EnumClose     | End enum variant       |
| Node          | Capture matched node   |
| Set(Mxx)      | Set field/member Mxx   |
| Null          | Null value             |

## Index Prefixes

| Prefix | Section      | Description |
| ------ | ------------ | ----------- |
| S##    | strings      | StringId    |
| R##    | regex        | RegexId     |
| T##    | type_defs    | TypeId      |
| M##    | type_members | MemberIndex |
| N##    | type_names   | NameIndex   |

## Type Format

### type_defs

| Kind     | Format          | Example               |
| -------- | --------------- | --------------------- |
| Node     | `<Node>`        | `T0 = <Node>`         |
| Void     | `<Void>`        | `T1 = <Void>`         |
| Struct   | `Struct  Mx:n`  | `T2 = Struct  M0:1`   |
| Enum     | `Enum    Mx:n`  | `T5 = Enum    M3:2`   |
| Optional | `Optional(Tx)`  | `T7 = Optional(T5)`   |
| Array\*  | `ArrayStar(Tx)` | `T3 = ArrayStar(T1)`  |
| Array+   | `ArrayPlus(Tx)` | `T10 = ArrayPlus(T9)` |
| Alias    | `Alias(Tx)`     | `T3 = Alias(T1)`      |

**Note**: Type indices are zero-padded based on the total count (e.g., `T0` for <10 types, `T00` for <100 types).

### type_members

Format: `Mx: Sxx → Txx  ; comment`

### type_names

Format: `Nx: Sxx → Txx  ; comment`
