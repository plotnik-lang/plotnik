# Bytecode Dump Format

The `dump` command displays compiled bytecode in a human-readable format.

## Example Query

```
Value = (document [
    Num: (number) @n
    Str: (string) @s
])
```

Run: `plotnik dump -q '<query>'`

## Bytecode Dump

**Epsilon transitions** (`ε`) succeed unconditionally without cursor interaction.
They are identified by `nav == Epsilon` — a distinct navigation mode (not Stay).

**Capture effect consolidation**: Scalar capture effects (`Node`, `Text`, `Set`) are
placed directly on match instructions rather than in separate epsilon steps. Structural
effects (`Obj`, `EndObj`, `Arr`, `EndArr`, `Enum`, `EndEnum`) may appear in epsilons or
consolidated into match instructions.

```
[flags]
linked = false

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

### Sections Explained

- **`_ObjWrap`**: Universal entry preamble. Wraps all entrypoints with `Obj`/`EndObj` and dispatches via `Trampoline`.
- **`Value`**: The compiled query definition. Step 08 branches to try `Num` (step 11) or `Str` (step 16).
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
|   | pad  |   | (ctr) |   |                      |   |      |
```

| Column  | Width    | Description                                        |
| ------- | -------- | -------------------------------------------------- |
| indent  | 2        | Leading spaces                                     |
| step    | variable | Step number, zero-padded to max step width         |
| gap     | 1        | Space separator                                    |
| symbol  | 5        | Nav symbol centered (e.g., `  ε  `, `  ▽  `, `△¹`) |
| gap     | 1        | Space separator                                    |
| content | variable | Instruction content                                |
| gap     | 1        | Space separator                                    |
| succ    | variable | Successors/markers, right-aligned                  |

**Step padding**: Dynamic based on max step in graph. Steps 0–9 use 1 digit, 0–99 use 2 digits, etc.

**Symbol column** (5 characters):

```
| left | center | right |
|  2   |   1    |   2   |
```

- **Center**: Direction (ε, ▽, ▷, △)
- **Left**: Mode modifier (`!` skip trivia, `!!` exact)
- **Right**: Level suffix (¹, ², ³... for Up)

Examples:

- `  ε  ` — epsilon (no movement)
- `  ▽  ` — down, skip any
- `  ▷  ` — next, skip any
- `  △  ` — up 1 level (no superscript)
- `!▽ ` — down, skip trivia
- `!!▷ ` — next, exact

| Instruction      | Format                                          |
| ---------------- | ----------------------------------------------- |
| Match (terminal) | `step nav    [pre] (type) [post]      ◼`        |
| Match            | `step nav    [pre] field: (type) [post] succ`   |
| Match (branch)   | `step nav    [pre] (type) [post]      s1, s2`   |
| Epsilon          | `step  ε     [effects]                succ`     |
| Call             | `step nav    field: (Name)        target : ret` |
| Return           | `step                                 ▶`        |
| Trampoline       | `step        Trampoline               succ`     |

Successors aligned in right column. Omit empty `[pre]`, `[post]`, `(type)`, `field:`.

Effects in `[pre]` execute before match attempt; effects in `[post]` execute after successful match. Any effect can appear in either position.

## Nav Symbols

| Nav             | Symbol  | Notes                              |
| --------------- | ------- | ---------------------------------- |
| Epsilon         | ε       | Pure control flow, no cursor check |
| Stay            | (blank) | No movement, 5 spaces              |
| StayExact       | !       | No movement, exact match only      |
| Down            | ▽       | First child, skip any              |
| DownSkip        | !▽      | First child, skip trivia           |
| DownExact       | !!▽     | First child, exact                 |
| Next            | ▷       | Next sibling, skip any             |
| NextSkip        | !▷      | Next sibling, skip trivia          |
| NextExact       | !!▷     | Next sibling, exact                |
| Up(1)           | △       | Ascend 1 level (no superscript)    |
| Up(n≥2)         | △ⁿ      | Ascend n levels, skip any          |
| UpSkipTrivia(n) | !△ⁿ     | Ascend n, must be last non-trivia  |
| UpExact(n)      | !!△ⁿ    | Ascend n, must be last child       |

**Note**: `ε` appears for `Nav::Epsilon` — a distinct mode from `Stay`. A step with `nav == Stay` but with type constraints (e.g., `(identifier)`) shows blank, not `ε`.

## Effects

| Effect    | Description            |
| --------- | ---------------------- |
| Obj       | Start struct           |
| EndObj    | End struct             |
| Arr       | Start array            |
| EndArr    | End array              |
| Push      | Push to array          |
| Enum(Mxx) | Start enum variant Mxx |
| EndEnum   | End enum variant       |
| Node      | Capture matched node   |
| Text      | Convert node to string |
| Set(Mxx)  | Set field/member Mxx   |
| Null      | Null value             |
| Clear     | Clear current          |

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
| String   | `<String>`      | `T1 = <String>`       |
| Void     | `<Void>`        | `T2 = <Void>`         |
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
