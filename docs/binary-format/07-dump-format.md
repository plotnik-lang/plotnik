# Bytecode Dump Implementation

## Example Query

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

## Bytecode Dump

**Epsilon transitions** (`ε`) succeed unconditionally without cursor interaction.
They require all three conditions:

- `nav == Stay` (no cursor movement)
- `node_type == None` (no type constraint)
- `node_field == None` (no field constraint)

A step with `nav == Stay` but with a type constraint (e.g., `(identifier)`) is NOT
epsilon—it matches at the current cursor position.

**Capture effect consolidation**: Scalar capture effects (`Node`, `Text`, `Set`) are
placed directly on match instructions rather than in separate epsilon steps. Structural
effects (`Obj`, `EndObj`, `Arr`, `EndArr`, `Enum`, `EndEnum`) remain in epsilons.

```
[flags]
linked = false

[strings]
S00 "Beauty will save the world"
S01 "name"
S02 "value"
S03 "Literal"
S04 "Variable"
S05 "target"
S06 "Ident"
S07 "Expression"
S08 "Assignment"
S09 "identifier"
S10 "number"
S11 "assignment_expression"
S12 "right"
S13 "left"

[type_defs]
T00 = <Node>
T01 = <String>
T02 = Struct  M0:1  ; { name }
T03 = Struct  M1:1  ; { value }
T04 = Struct  M2:1  ; { name }
T05 = Enum    M3:2  ; Literal | Variable
T06 = Struct  M5:2  ; { value, target }
T07 = Struct  M7:1  ; { target }
T08 = Struct  M8:1  ; { value }

[type_members]
M0: S01 → T01  ; name: <String>
M1: S02 → T00  ; value: <Node>
M2: S01 → T00  ; name: <Node>
M3: S03 → T03  ; Literal: T03
M4: S04 → T04  ; Variable: T04
M5: S02 → T05  ; value: Expression
M6: S05 → T00  ; target: <Node>
M7: S05 → T00  ; target: <Node>
M8: S02 → T05  ; value: Expression

[type_names]
N0: S06 → T02  ; Ident
N1: S07 → T05  ; Expression
N2: S08 → T06  ; Assignment

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

## Files

- `crates/plotnik-lib/src/bytecode/dump.rs` (new)
- `crates/plotnik-lib/src/bytecode/dump_tests.rs` (new)
- `crates/plotnik-lib/src/bytecode/mod.rs` (add exports)

## API

```rust
pub fn dump(module: &Module) -> String
```

Future: options for verbosity levels, hiding sections, etc.

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

| Instruction      | Format                                        |
| ---------------- | --------------------------------------------- |
| Match (terminal) | `step nav    [pre] (type) [post]      ◼`      |
| Match            | `step nav    [pre] field: (type) [post] succ` |
| Match (branch)   | `step nav    [pre] (type) [post]      s1, s2` |
| Epsilon          | `step  ε     [effects]                succ`   |
| Call             | `step nav    field: (Name)         return ⯇`  |
| Return           | `step                                 ▶`      |

Successors aligned in right column. Omit empty `[pre]`, `[post]`, `(type)`, `field:`.

Effects in `[pre]` execute before match attempt; effects in `[post]` execute after successful match. Any effect can appear in either position.

## Nav Symbols

| Nav             | Symbol  | Notes                               |
| --------------- | ------- | ----------------------------------- |
| Stay            | (blank) | No movement, 5 spaces               |
| Stay (epsilon)  | ε       | Only when no type/field constraints |
| StayExact       | !!!     | No movement, exact match only       |
| Down            | ▽       | First child, skip any               |
| DownSkip        | !▽      | First child, skip trivia            |
| DownExact       | !!▽     | First child, exact                  |
| Next            | ▷       | Next sibling, skip any              |
| NextSkip        | !▷      | Next sibling, skip trivia           |
| NextExact       | !!▷     | Next sibling, exact                 |
| Up(1)           | △       | Ascend 1 level (no superscript)     |
| Up(n≥2)         | △ⁿ      | Ascend n levels, skip any           |
| UpSkipTrivia(n) | !△ⁿ     | Ascend n, must be last non-trivia   |
| UpExact(n)      | !!△ⁿ    | Ascend n, must be last child        |

**Note**: `ε` only appears when all three conditions are met: Stay nav, no type constraint, no field constraint. A step matching `(identifier)` at current position shows spaces, not `ε`.

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
