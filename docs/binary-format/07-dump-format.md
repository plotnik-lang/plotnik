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

**Epsilon transitions** (`𝜀`) succeed unconditionally without cursor interaction.
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
linked = true

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
S12 "left"
S13 "right"

[type_defs]
T00 = void
T01 = Node
T02 = str
T03 = Struct  M0[1]  ; { name }
T04 = Struct  M1[1]  ; { value }
T05 = Struct  M2[1]  ; { name }
T06 = Enum    M3[2]  ; Literal | Variable
T07 = Struct  M5[2]  ; { value, target }

[type_members]
M0: S01 → T02  ; name: str
M1: S02 → T01  ; value: Node
M2: S01 → T01  ; name: Node
M3: S03 → T04  ; Literal: T04
M4: S04 → T05  ; Variable: T05
M5: S02 → T06  ; value: Expression
M6: S05 → T01  ; target: Node

[type_names]
N0: S06 → T03  ; Ident
N1: S07 → T06  ; Expression
N2: S08 → T07  ; Assignment

[entrypoints]
Assignment = 08 :: T07
Expression = 05 :: T06
Ident      = 01 :: T03

[transitions]
  00  𝜀                                     ◼

Ident:
  01  𝜀                                     02
  02       (identifier) [Text Set(M0)]      04
  04                                        ▶

Expression:
  05  𝜀                                     06
  06  𝜀                                     22, 28

Assignment:
  08  𝜀                                     09
  09       (assignment_expression)          10
  10  ↓*   left: (identifier) [Node Set(M6)]  12
  12  *  ▶ right: (Expression)              13
  13  𝜀    [Set(M5)]                        15
  15 *↑¹                                    16
  16                                        ▶
  17                                        ▶
  18  𝜀    [EndEnum]                        17
  20       (number) [Node Set(M1)]          18
  22  𝜀    [Enum(M3)]                       20
  24  𝜀    [EndEnum]                        17
  26       (identifier) [Node Set(M2)]      24
  28  𝜀    [Enum(M4)]                       26
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

Each line follows the column layout: `<indent><step><gap><nav><marker><content>...<successors>`

| Column     | Width | Description                              |
| ---------- | ----- | ---------------------------------------- |
| indent     | 2     | Leading spaces                           |
| step       | var   | Step number, zero-padded                 |
| gap        | 1     | Space separator                          |
| nav        | 3     | Navigation symbol (↓\*, \*↑¹, etc.) or 𝜀 |
| marker     | 3     | Call marker ( ▶ ) or spaces              |
| content    | var   | Instruction content                      |
| successors | -     | Right-aligned at column 44               |

| Instruction      | Format                                        |
| ---------------- | --------------------------------------------- |
| Match (terminal) | `step nav    [pre] (type) [post]      ◼`      |
| Match            | `step nav    [pre] field: (type) [post] succ` |
| Match (branch)   | `step nav    [pre] (type) [post]      s1, s2` |
| Epsilon          | `step  𝜀    [effects]                 succ`   |
| Call             | `step nav ▶ field: (Name)             return` |
| Return           | `step  𝜀                              ▶`      |

Successors aligned in right column. Omit empty `[pre]`, `[post]`, `(type)`, `field:`.

Effects in `[pre]` execute before match attempt; effects in `[post]` execute after successful match. Any effect can appear in either position.

## Nav Symbols

| Nav             | Symbol     | Notes                                    |
| --------------- | ---------- | ---------------------------------------- |
| Stay            | (3 spaces) | No movement                              |
| Stay (epsilon)  | 𝜀          | Only when no type/field constraints      |
| Down            | ↓\*        | First child, skip any                    |
| DownSkip        | ↓~         | First child, skip trivia                 |
| DownExact       | ↓.         | First child, exact                       |
| Next            | \*         | Next sibling, skip any                   |
| NextSkip        | ~          | Next sibling, skip trivia                |
| NextExact       | .          | Next sibling, exact                      |
| Up(n)           | \*↑ⁿ       | Ascend n levels, skip any                |
| UpSkipTrivia(n) | ~↑ⁿ        | Ascend n, must be last non-trivia        |
| UpExact(n)      | .↑ⁿ        | Ascend n, must be last child             |

**Note**: `𝜀` only appears when all three conditions are met: Stay nav, no type constraint, no field constraint. A step matching `(identifier)` at current position shows spaces, not `𝜀`.

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

| Prefix | Section       | Description |
| ------ | ------------- | ----------- |
| S##    | strings       | StringId    |
| T##    | type_defs    | TypeId      |
| M##    | type_members | MemberIndex |
| N##    | type_names   | NameIndex   |

## Type Format

### type_defs

| Kind     | Format              | Example                  |
| -------- | ------------------- | ------------------------ |
| void     | `void`              | `T00 = void`             |
| Node     | `Node`              | `T01 = Node`             |
| str      | `str`               | `T02 = str`              |
| Struct   | `Struct  Mxx[n]`    | `T03 = Struct  M0[1]`    |
| Enum     | `Enum    Mxx[n]`    | `T06 = Enum    M3[2]`    |
| Optional | `Optional(Txx)`     | `T07 = Optional(T05)`    |
| Array\*  | `ArrayStar(Txx)`    | `T03 = ArrayStar(T01)`   |
| Array+   | `ArrayPlus(Txx)`    | `T10 = ArrayPlus(T09)`   |
| Alias    | `Alias(Txx)`        | `T03 = Alias(T01)`       |

### type_members

Format: `Mx: Sxx → Txx  ; comment`

### type_names

Format: `Nx: Sxx → Txx  ; comment`
