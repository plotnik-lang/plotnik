# Bytecode Dump Implementation

## Example Query

```
Expression = [
  Ident: (identifier) @name :: string
  Num: (number) @value :: string
]

Statement = [
  Assign: (assignment_expression
    left: (identifier) @target :: string
    right: (Expression) @value)
  Return: (return_statement (Expression)? @value)
]

Root = (program (Statement)+ @statements)
```

## Bytecode Dump

```
[header]
linked = false

[strings]
S00 "Beauty will save the world"
S01 "Assign"
S02 "Expression"
S03 "Ident"
S04 "Num"
S05 "Return"
S06 "Root"
S07 "Statement"
S08 "assignment_expression"
S09 "identifier"
S10 "left"
S11 "name"
S12 "number"
S13 "program"
S14 "return_statement"
S15 "right"
S16 "statements"
S17 "target"
S18 "value"

[types.defs]
T00 = void
T01 = Node
T02 = str
T03 = Struct(M0, 1)  ; { name }
T04 = Struct(M1, 1)  ; { value }
T05 = Enum(M2, 2)  ; Ident | Num
T06 = Struct(M4, 2)  ; { target, value }
T07 = Optional(T05)  ; Expression?
T08 = Struct(M6, 1)  ; { value }
T09 = Enum(M7, 2)  ; Assign | Return
T10 = ArrayPlus(T09)  ; Statement+
T11 = Struct(M9, 1)  ; { statements }

[types.members]
M0 = (S11, T02)  ; name: str
M1 = (S18, T02)  ; value: str
M2 = (S03, T03)  ; Ident => T03
M3 = (S04, T04)  ; Num => T04
M4 = (S17, T02)  ; target: str
M5 = (S18, T05)  ; value: Expression
M6 = (S18, T07)  ; value: Expression?
M7 = (S01, T06)  ; Assign => T06
M8 = (S05, T08)  ; Return => T08
M9 = (S16, T10)  ; statements: Statement+

[types.names]
N0 = (S02, T05)  ; Expression
N1 = (S06, T11)  ; Root
N2 = (S07, T09)  ; Statement

[entry]  ; sorted lexicographically for binary search
Expression = 46 :: T05
Root       = 01 :: T11
Statement  = 14 :: T09

[code]
  00   𝜀                                    ◼

Root:
  01  *↓   [S] (program)                    03
  03   𝜀   [A]                              05
  05   ▶   (Statement)                      06
  06   𝜀   [Push]                           08
  08   𝜀                                    05, 10
  10   𝜀   [EndA Set(M9) EndS]              12
  12  *↑¹                                   ◼

Statement:
  14   𝜀                                    16, 32
  16  *↓   [E(M7) S] (assignment_expression)                      18
  18  *↓   left: (identifier) [Node Text Set(M4)]                 20
  20  *    right: _                         21
  21       ▷(Expression)                    22
  22   𝜀   [Set(M5) EndS EndE]              24
  24  *↑²                                   26
  26       (Statement)                      ▶
  32  *↓   [E(M8) S] (return_statement)     34
  34   𝜀                                    36, 40
  36  *↓                                    37
  37   ▶   (Expression)                     38
  38   𝜀   [Set(M6)]                        42
  40   𝜀   [Null Set(M6)]                   42
  42   𝜀   [EndS EndE]                      44
  44  *↑¹                                   26

Expression:
  46   𝜀                                    48, 54
  48  *↓   [E(M2) S] (identifier) [Node Text Set(M0) EndS EndE]   50
  50  *↑¹                                   52
  52       (Expression)                     ▶
  54  *↓   [E(M3) S] (number) [Node Text Set(M1) EndS EndE]       56
  56  *↑¹                                   52
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

| Instruction      | Format                                        |
| ---------------- | --------------------------------------------- |
| Match (terminal) | `step  nav [pre] (type) [post]        ◼`      |
| Match            | `step  nav [pre] field: (type) [post] succ`   |
| Match (branch)   | `step  nav [pre] (type) [post]        s1, s2` |
| Epsilon          | `step   𝜀   [effects]                 succ`   |
| Call             | `step   ▶  (Name)                     return` |
| Return           | `step      (Name)                     ▶`      |

Successors aligned in right column. Omit empty `[pre]`, `[post]`, `(type)`.

Pre-effects (before match): `S`, `A`, `E(n)`
Post-effects (after match): `EndS`, `EndA`, `EndE`, `Push`, `Node`, `Text`, `Set(n)`, `Null`, `Clear`

## Nav Symbols

| Nav             | Symbol |
| --------------- | ------ |
| Stay            | _omit_ |
| Down            | \*↓    |
| DownSkip        | ~↓     |
| DownExact       | .↓     |
| Next            | \*     |
| NextSkip        | ~      |
| NextExact       | .      |
| Up(n)           | \*↑ⁿ   |
| UpSkipTrivia(n) | ~↑ⁿ    |
| UpExact(n)      | .↑ⁿ    |

## Effects

| Effect   | Description            |
| -------- | ---------------------- |
| S        | Start struct           |
| EndS     | End struct             |
| A        | Start array            |
| EndA     | End array              |
| Push     | Push to array          |
| E(Mxx)   | Start enum variant Mxx |
| EndE     | End enum variant       |
| Node     | Capture matched node   |
| Text     | Convert node to string |
| Set(Mxx) | Set field/member Mxx   |
| Null     | Null value             |
| Clear    | Clear current          |

## Index Prefixes

| Prefix | Section       | Description |
| ------ | ------------- | ----------- |
| S##    | strings       | StringId    |
| T##    | types.defs    | TypeId      |
| M##    | types.members | MemberIndex |
| N##    | types.names   | NameIndex   |

## Type Format

### types.defs

| Kind     | Format               | Example                |
| -------- | -------------------- | ---------------------- |
| void     | `void`               | `T00 = void`           |
| Node     | `Node`               | `T01 = Node`           |
| str      | `str`                | `T02 = str`            |
| Struct   | `Struct(Mxx, count)` | `T03 = Struct(M0, 1)`  |
| Enum     | `Enum(Mxx, count)`   | `T05 = Enum(M2, 2)`    |
| Optional | `Optional(Txx)`      | `T07 = Optional(T05)`  |
| Array\*  | `ArrayStar(Txx)`     | `ArrayStar(T09)`       |
| Array+   | `ArrayPlus(Txx)`     | `T10 = ArrayPlus(T09)` |
| Alias    | `Alias(Txx)`         | `T03 = Alias(T01)`     |

### types.members

Format: `Mx = (Sxx, Txx)  ; comment`

### types.names

Format: `Nx = (Sxx, Txx)  ; comment`
