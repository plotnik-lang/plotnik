//! Tests for bytecode dump functionality.

use crate::Query;
use indoc::indoc;

#[test]
fn dump_minimal() {
    let input = "Test = (identifier) @id";

    let res = Query::expect_valid_linked_bytecode(input);

    insta::assert_snapshot!(res, @r#"
    [header]
    linked = true

    [strings]
    S00 "Beauty will save the world"
    S01 "id"
    S02 "Test"
    S03 "identifier"

    [types.defs]
    T00 = void
    T01 = Node
    T02 = str
    T03 = Struct(M0, 1)  ; { id }

    [types.members]
    M0 = (S01, T01)  ; id: Node

    [types.names]
    N0 = (S02, T03)  ; Test

    [entry]
    Test = 01 :: T03

    [code]
      00  𝜀                                     ◼

    Test:
      01  𝜀                                     02
      02       (identifier) [Node Set(M0)]      04
      04                                        ▶
    "#);
}

#[test]
fn dump_multiple_entrypoints() {
    let input = indoc! {r#"
        Expression = [(identifier) @name (number) @value]
        Root = (function_declaration name: (identifier) @name)
    "#};

    let res = Query::expect_valid_linked_bytecode(input);

    insta::assert_snapshot!(res, @r#"
    [header]
    linked = true

    [strings]
    S00 "Beauty will save the world"
    S01 "name"
    S02 "value"
    S03 "Expression"
    S04 "Root"
    S05 "identifier"
    S06 "number"
    S07 "function_declaration"

    [types.defs]
    T00 = void
    T01 = Node
    T02 = str
    T03 = Struct(M0, 2)  ; { name, value }
    T04 = Struct(M2, 1)  ; { name }
    T05 = Optional(T01)  ; Node?

    [types.members]
    M0 = (S01, T05)  ; name: T05
    M1 = (S02, T05)  ; value: T05
    M2 = (S01, T01)  ; name: Node

    [types.names]
    N0 = (S03, T03)  ; Expression
    N1 = (S04, T04)  ; Root

    [entry]
    Expression = 01 :: T03
    Root       = 04 :: T04

    [code]
      00  𝜀                                     ◼

    Expression:
      01  𝜀                                     02
      02  𝜀                                     13, 17

    Root:
      04  𝜀                                     05
      05       (function_declaration)           06
      06  ↓*   name: (identifier) [Node Set(M2)]08
      08 *↑¹                                    09
      09                                        ▶
      10                                        ▶
      11       (identifier) [Node Set(M0)]      10
      13  𝜀    [Null Set(M1)]                   11
      15       (number) [Node Set(M1)]          10
      17  𝜀    [Null Set(M0)]                   15
    "#);
}

#[test]
fn dump_with_field_constraints() {
    let input = indoc! {r#"
        Test = (binary_expression
            left: (_) @left
            right: (_) @right)
    "#};

    let res = Query::expect_valid_linked_bytecode(input);

    insta::assert_snapshot!(res, @r#"
    [header]
    linked = true

    [strings]
    S00 "Beauty will save the world"
    S01 "left"
    S02 "right"
    S03 "Test"
    S04 "binary_expression"

    [types.defs]
    T00 = void
    T01 = Node
    T02 = str
    T03 = Struct(M0, 2)  ; { left, right }

    [types.members]
    M0 = (S01, T01)  ; left: Node
    M1 = (S02, T01)  ; right: Node

    [types.names]
    N0 = (S03, T03)  ; Test

    [entry]
    Test = 01 :: T03

    [code]
      00  𝜀                                     ◼

    Test:
      01  𝜀                                     02
      02       (binary_expression)              03
      03  ↓*   left: _ [Node Set(M0)]           05
      05  *    right: _ [Node Set(M1)]          07
      07 *↑¹                                    08
      08                                        ▶
    "#);
}

#[test]
fn dump_with_quantifier() {
    let input = "Test = (identifier)* @items";

    let res = Query::expect_valid_linked_bytecode(input);

    insta::assert_snapshot!(res, @r#"
    [header]
    linked = true

    [strings]
    S00 "Beauty will save the world"
    S01 "items"
    S02 "Test"
    S03 "identifier"

    [types.defs]
    T00 = void
    T01 = Node
    T02 = str
    T03 = ArrayStar(T01)  ; Node*
    T04 = Struct(M0, 1)  ; { items }

    [types.members]
    M0 = (S01, T03)  ; items: T03

    [types.names]
    N0 = (S02, T04)  ; Test

    [entry]
    Test = 01 :: T04

    [code]
      00  𝜀                                     ◼

    Test:
      01  𝜀                                     02
      02  𝜀    [Arr]                            04
      04  𝜀                                     09, 07
      06                                        ▶
      07  𝜀    [EndArr Set(M0)]                 06
      09       (identifier) [Push]              13
      11       (identifier) [Push]              13
      13  𝜀                                     11, 07
    "#);
}

#[test]
fn dump_with_alternation() {
    let input = "Test = [(identifier) @id (string) @str]";

    let res = Query::expect_valid_linked_bytecode(input);

    insta::assert_snapshot!(res, @r#"
    [header]
    linked = true

    [strings]
    S00 "Beauty will save the world"
    S01 "id"
    S02 "str"
    S03 "Test"
    S04 "identifier"
    S05 "string"

    [types.defs]
    T00 = void
    T01 = Node
    T02 = str
    T03 = Struct(M0, 2)  ; { id, str }
    T04 = Optional(T01)  ; Node?

    [types.members]
    M0 = (S01, T04)  ; id: T04
    M1 = (S02, T04)  ; str: T04

    [types.names]
    N0 = (S03, T03)  ; Test

    [entry]
    Test = 01 :: T03

    [code]
      00  𝜀                                     ◼

    Test:
      01  𝜀                                     02
      02  𝜀                                     07, 11
      04                                        ▶
      05       (identifier) [Node Set(M0)]      04
      07  𝜀    [Null Set(M1)]                   05
      09       (string) [Node Set(M1)]          04
      11  𝜀    [Null Set(M0)]                   09
    "#);
}

#[test]
fn dump_comprehensive() {
    // A query that exercises most features:
    // - Multiple definitions (entrypoints)
    // - Field constraints (node_fields)
    // - Multiple node types (node_types)
    // - Captures with types (type_defs, type_members)
    // - Alternation (branching in code)
    let input = indoc! {r#"
        Ident = (identifier) @name :: string
        Expression = [
            Literal: (number) @value
            Variable: (identifier) @name
        ]
        Assignment = (assignment_expression
            left: (identifier) @target
            right: (Expression) @value)
    "#};

    let res = Query::expect_valid_linked_bytecode(input);

    insta::assert_snapshot!(res, @r#"
    [header]
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

    [types.defs]
    T00 = void
    T01 = Node
    T02 = str
    T03 = Struct(M0, 1)  ; { name }
    T04 = Struct(M1, 1)  ; { value }
    T05 = Struct(M2, 1)  ; { name }
    T06 = Enum(M3, 2)  ; Literal | Variable
    T07 = Struct(M5, 2)  ; { value, target }

    [types.members]
    M0 = (S01, T02)  ; name: str
    M1 = (S02, T01)  ; value: Node
    M2 = (S01, T01)  ; name: Node
    M3 = (S03, T04)  ; Literal: T04
    M4 = (S04, T05)  ; Variable: T05
    M5 = (S02, T06)  ; value: Expression
    M6 = (S05, T01)  ; target: Node

    [types.names]
    N0 = (S06, T03)  ; Ident
    N1 = (S07, T06)  ; Expression
    N2 = (S08, T07)  ; Assignment

    [entry]
    Assignment = 08 :: T07
    Expression = 05 :: T06
    Ident      = 01 :: T03

    [code]
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
      10  ↓*   left: (identifier) [Node Set(M6)]12
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
    "#);
}

/// Regression test for nested bubble captures.
///
/// Previously, deeply nested bubble captures (3+ levels) produced incorrect
/// member indices. The innermost captures were offset incorrectly because
/// each bubble capture pushed an intermediate type as the scope, but those
/// types weren't emitted to bytecode. The fix ensures bubble captures don't
/// push new scopes, so all member lookups reference the root struct.
#[test]
fn dump_nested_bubble_captures_three_levels() {
    // 3 levels of nested bubble captures: @a wraps @b wraps @c
    let input = "Test = (a (b (c) @c) @b) @a";

    let res = Query::expect_valid_bytecode(input);

    // Verify member indices: @c should use M2, @b should use M1, @a should use M0
    // Previously this would incorrectly produce: @c→M1, @b→M0, @a→M0
    insta::assert_snapshot!(res, @r#"
    [header]
    linked = false

    [strings]
    S00 "Beauty will save the world"
    S01 "a"
    S02 "b"
    S03 "c"
    S04 "Test"

    [types.defs]
    T00 = void
    T01 = Node
    T02 = str
    T03 = Struct(M0, 3)  ; { a, b, c }

    [types.members]
    M0 = (S01, T01)  ; a: Node
    M1 = (S02, T01)  ; b: Node
    M2 = (S03, T01)  ; c: Node

    [types.names]
    N0 = (S04, T03)  ; Test

    [entry]
    Test = 01 :: T03

    [code]
      00  𝜀                                     ◼

    Test:
      01  𝜀                                     02
      02       (a) [Node Set(M0)]               04
      04  ↓*   (b) [Node Set(M1)]               06
      06  ↓*   (c) [Node Set(M2)]               08
      08 *↑¹                                    09
      09 *↑¹                                    10
      10                                        ▶
    "#);
}

#[test]
fn dump_nested_bubble_captures_four_levels() {
    // 4 levels of nested bubble captures
    let input = "Test = (a (b (c (d) @d) @c) @b) @a";

    let res = Query::expect_valid_bytecode(input);

    // Verify all member indices are correct
    insta::assert_snapshot!(res, @r#"
    [header]
    linked = false

    [strings]
    S00 "Beauty will save the world"
    S01 "a"
    S02 "b"
    S03 "c"
    S04 "d"
    S05 "Test"

    [types.defs]
    T00 = void
    T01 = Node
    T02 = str
    T03 = Struct(M0, 4)  ; { a, b, c, d }

    [types.members]
    M0 = (S01, T01)  ; a: Node
    M1 = (S02, T01)  ; b: Node
    M2 = (S03, T01)  ; c: Node
    M3 = (S04, T01)  ; d: Node

    [types.names]
    N0 = (S05, T03)  ; Test

    [entry]
    Test = 01 :: T03

    [code]
      00  𝜀                                     ◼

    Test:
      01  𝜀                                     02
      02       (a) [Node Set(M0)]               04
      04  ↓*   (b) [Node Set(M1)]               06
      06  ↓*   (c) [Node Set(M2)]               08
      08  ↓*   (d) [Node Set(M3)]               10
      10 *↑¹                                    11
      11 *↑¹                                    12
      12 *↑¹                                    13
      13                                        ▶
    "#);
}

/// Regression test for quantifier repeat navigation.
///
/// Previously, repeat iterations in */+ quantifiers used None for navigation,
/// defaulting to Stay. This caused infinite loops because the VM would match
/// the same node forever instead of advancing to the next sibling.
#[test]
fn dump_quantifier_repeat_navigation() {
    let input = "Test = (function_declaration (decorator)* @decs)";

    let res = Query::expect_valid_linked_bytecode(input);

    // The first iteration should use Down (↓*), repeat should use Next (*)
    // Previously, the repeat iteration had no navigation (stayed on same node)
    insta::assert_snapshot!(res, @r#"
    [header]
    linked = true

    [strings]
    S00 "Beauty will save the world"
    S01 "decs"
    S02 "Test"
    S03 "function_declaration"
    S04 "decorator"

    [types.defs]
    T00 = void
    T01 = Node
    T02 = str
    T03 = ArrayStar(T01)  ; Node*
    T04 = Struct(M0, 1)  ; { decs }

    [types.members]
    M0 = (S01, T03)  ; decs: T03

    [types.names]
    N0 = (S02, T04)  ; Test

    [entry]
    Test = 01 :: T04

    [code]
      00  𝜀                                     ◼

    Test:
      01  𝜀                                     02
      02       (function_declaration)           03
      03  𝜀    [Arr]                            05
      05  𝜀                                     13, 11
      07  𝜀    [EndArr Set(M0)]                 09
      09 *↑¹                                    10
      10                                        ▶
      11  𝜀    [EndArr Set(M0)]                 10
      13  ↓*   (decorator) [Push]               17
      15  *    (decorator) [Push]               17
      17  𝜀                                     15, 07
    "#);
}

/// Regression test for Null effect emission in alternations.
///
/// When a capture appears in some alternation branches but not others,
/// the compiler must inject Null Set(member) for the missing fields.
/// This ensures the output object always has all fields defined.
#[test]
fn dump_alternation_null_injection() {
    let input = "Test = [(identifier) @x (number) @y]";

    let res = Query::expect_valid_linked_bytecode(input);

    // Branch 1 (identifier @x) should inject Null Set(M1) for missing @y
    // Branch 2 (number @y) should inject Null Set(M0) for missing @x
    insta::assert_snapshot!(res, @r#"
    [header]
    linked = true

    [strings]
    S00 "Beauty will save the world"
    S01 "x"
    S02 "y"
    S03 "Test"
    S04 "identifier"
    S05 "number"

    [types.defs]
    T00 = void
    T01 = Node
    T02 = str
    T03 = Struct(M0, 2)  ; { x, y }
    T04 = Optional(T01)  ; Node?

    [types.members]
    M0 = (S01, T04)  ; x: T04
    M1 = (S02, T04)  ; y: T04

    [types.names]
    N0 = (S03, T03)  ; Test

    [entry]
    Test = 01 :: T03

    [code]
      00  𝜀                                     ◼

    Test:
      01  𝜀                                     02
      02  𝜀                                     07, 11
      04                                        ▶
      05       (identifier) [Node Set(M0)]      04
      07  𝜀    [Null Set(M1)]                   05
      09       (number) [Node Set(M1)]          04
      11  𝜀    [Null Set(M0)]                   09
    "#);
}

/// Regression test for captured tagged alternation effect placement.
///
/// When a tagged alternation has an outer capture like `[A: (x) @a] @item`,
/// the outer capture's Set effect must go on the EndEnum step, not on the
/// branch body. Otherwise the field receives a Node instead of the enum.
#[test]
fn dump_captured_tagged_alternation() {
    let input = "Q = [A: (identifier) @a  B: (number) @b] @item";

    let res = Query::expect_valid_linked_bytecode(input);

    // The Set(M3) for @item should be on the EndEnum step, not with Node Set(M0/M1)
    insta::assert_snapshot!(res, @r#"
    [header]
    linked = true

    [strings]
    S00 "Beauty will save the world"
    S01 "a"
    S02 "b"
    S03 "A"
    S04 "B"
    S05 "item"
    S06 "Q"
    S07 "identifier"
    S08 "number"

    [types.defs]
    T00 = void
    T01 = Node
    T02 = str
    T03 = Struct(M0, 1)  ; { a }
    T04 = Struct(M1, 1)  ; { b }
    T05 = Enum(M2, 2)  ; A | B
    T06 = Struct(M4, 1)  ; { item }

    [types.members]
    M0 = (S01, T01)  ; a: Node
    M1 = (S02, T01)  ; b: Node
    M2 = (S03, T03)  ; A: T03
    M3 = (S04, T04)  ; B: T04
    M4 = (S05, T05)  ; item: T05

    [types.names]
    N0 = (S06, T06)  ; Q

    [entry]
    Q = 01 :: T06

    [code]
      00  𝜀                                     ◼

    Q:
      01  𝜀                                     02
      02  𝜀                                     09, 15
      04                                        ▶
      05  𝜀    [EndEnum Set(M4)]                04
      07       (identifier) [Node Set(M0)]      05
      09  𝜀    [Enum(M2)]                       07
      11  𝜀    [EndEnum Set(M4)]                04
      13       (number) [Node Set(M1)]          11
      15  𝜀    [Enum(M3)]                       13
    "#);
}

/// Regression test for captured tagged alternation inside quantifier.
///
/// Same issue as above, but the outer capture is inside a row quantifier.
/// The Set effect for @item must be on EndEnum, not on the branch body.
#[test]
fn dump_captured_tagged_alternation_in_quantifier() {
    let input =
        "Q = (object { [A: (pair) @a  B: (shorthand_property_identifier) @b] @item }* @items)";

    let res = Query::expect_valid_linked_bytecode(input);

    // The Set(M4) for @item should be on the EndEnum step
    insta::assert_snapshot!(res, @r#"
    [header]
    linked = true

    [strings]
    S00 "Beauty will save the world"
    S01 "a"
    S02 "b"
    S03 "A"
    S04 "B"
    S05 "item"
    S06 "items"
    S07 "Q"
    S08 "object"
    S09 "pair"
    S10 "shorthand_property_identifier"

    [types.defs]
    T00 = void
    T01 = Node
    T02 = str
    T03 = Struct(M0, 1)  ; { a }
    T04 = Struct(M1, 1)  ; { b }
    T05 = Enum(M2, 2)  ; A | B
    T06 = Struct(M4, 1)  ; { item }
    T07 = ArrayStar(T06)  ; T06*
    T08 = Struct(M5, 1)  ; { items }

    [types.members]
    M0 = (S01, T01)  ; a: Node
    M1 = (S02, T01)  ; b: Node
    M2 = (S03, T03)  ; A: T03
    M3 = (S04, T04)  ; B: T04
    M4 = (S05, T05)  ; item: T05
    M5 = (S06, T07)  ; items: T07

    [types.names]
    N0 = (S07, T08)  ; Q

    [entry]
    Q = 01 :: T08

    [code]
      00  𝜀                                     ◼

    Q:
      01  𝜀                                     02
      02       (object)                         03
      03  𝜀    [Arr]                            05
      05  𝜀                                     29, 11
      07  𝜀    [EndArr Set(M5)]                 09
      09 *↑¹                                    10
      10                                        ▶
      11  𝜀    [EndArr Set(M5)]                 10
      13  𝜀    [EndObj Push]                    49
      15  𝜀    [EndEnum Set(M4)]                13
      17  ↓*   (pair) [Node Set(M0)]            15
      19  𝜀    [Enum(M2)]                       17
      21  𝜀    [EndEnum Set(M4)]                13
      23  ↓*   (shorthand_property_identifier) [Node Set(M1)]21
      25  𝜀    [Enum(M3)]                       23
      27  𝜀                                     19, 25
      29  𝜀    [Obj]                            27
      31  𝜀    [EndObj Push]                    49
      33  𝜀    [EndEnum Set(M4)]                31
      35  *    (pair) [Node Set(M0)]            33
      37  𝜀    [Enum(M2)]                       35
      39  𝜀    [EndEnum Set(M4)]                31
      41  *    (shorthand_property_identifier) [Node Set(M1)]39
      43  𝜀    [Enum(M3)]                       41
      45  𝜀                                     37, 43
      47  𝜀    [Obj]                            45
      49  𝜀                                     47, 07
    "#);
}

/// Regression test: alternation capture without internal captures needs Node effect.
///
/// Bug: `[(identifier) (number)] @x` was missing the Node effect, producing
/// just `[Set(M0)]` instead of `[Node Set(M0)]`. The alternation doesn't have
/// internal captures, so it produces a Node value, not a Struct.
#[test]
fn regression_alternation_capture_node_effect() {
    let input = "Q = (program [(identifier) (number)] @x)";

    let res = Query::expect_valid_bytecode(input);

    insta::assert_snapshot!(res, @r#"
    [header]
    linked = false

    [strings]
    S00 "Beauty will save the world"
    S01 "x"
    S02 "Q"
    S03 "program"
    S04 "identifier"
    S05 "number"

    [types.defs]
    T00 = void
    T01 = Node
    T02 = str
    T03 = Struct(M0, 1)  ; { x }

    [types.members]
    M0 = (S01, T01)  ; x: Node

    [types.names]
    N0 = (S02, T03)  ; Q

    [entry]
    Q = 01 :: T03

    [code]
      00  𝜀                                     ◼

    Q:
      01  𝜀                                     02
      02       (program)                        03
      03  𝜀                                     06, 08
      05                                        ▶
      06  ↓*   (identifier) [Node Set(M0)]      10
      08  ↓*   (number) [Node Set(M0)]          10
      10 *↑¹                                    05
    "#);
}

/// Regression test: optional first-child skip path needs Down navigation.
///
/// Bug: When `(parent (child)? @a (sibling) @b)` skips the optional, the sibling
/// was compiled with Next navigation, but we never descended into parent's children.
/// The skip path must use Down navigation for the first non-skipped child.
#[test]
fn regression_optional_first_child_skip_navigation() {
    let input = "Q = (program (identifier)? @id (number) @n)";

    let res = Query::expect_valid_bytecode(input);

    insta::assert_snapshot!(res, @r#"
    [header]
    linked = false

    [strings]
    S00 "Beauty will save the world"
    S01 "id"
    S02 "n"
    S03 "Q"
    S04 "program"
    S05 "number"
    S06 "identifier"

    [types.defs]
    T00 = void
    T01 = Node
    T02 = str
    T03 = Struct(M0, 2)  ; { id, n }
    T04 = Optional(T01)  ; Node?

    [types.members]
    M0 = (S01, T04)  ; id: T04
    M1 = (S02, T01)  ; n: Node

    [types.names]
    N0 = (S03, T03)  ; Q

    [entry]
    Q = 01 :: T03

    [code]
      00  𝜀                                     ◼

    Q:
      01  𝜀                                     02
      02       (program)                        03
      03  𝜀                                     12, 10
      05                                        ▶
      06  ↓*   (number) [Node Set(M1)]          14
      08  *    (number) [Node Set(M1)]          14
      10  𝜀    [Null Set(M0)]                   06
      12  ↓*   (identifier) [Node Set(M0)]      08
      14 *↑¹                                    05
    "#);
}

/// Regression test: optional skip path needs Null injection for captures.
///
/// Bug: When `(parent (child)? @a)` skips the optional, the field @a was never
/// set to null. Alternations correctly inject `[Null Set(Mx)]` for missing
/// captures, but optionals didn't.
#[test]
fn regression_optional_skip_null_injection() {
    let input = "Q = (function_declaration (decorator)? @dec)";

    let res = Query::expect_valid_bytecode(input);

    insta::assert_snapshot!(res, @r#"
    [header]
    linked = false

    [strings]
    S00 "Beauty will save the world"
    S01 "dec"
    S02 "Q"
    S03 "function_declaration"
    S04 "decorator"

    [types.defs]
    T00 = void
    T01 = Node
    T02 = str
    T03 = Struct(M0, 1)  ; { dec }
    T04 = Optional(T01)  ; Node?

    [types.members]
    M0 = (S01, T04)  ; dec: T04

    [types.names]
    N0 = (S02, T03)  ; Q

    [entry]
    Q = 01 :: T03

    [code]
      00  𝜀                                     ◼

    Q:
      01  𝜀                                     02
      02       (function_declaration)           03
      03  𝜀                                     05, 09
      05  ↓*   (decorator) [Node Set(M0)]       07
      07 *↑¹                                    08
      08                                        ▶
      09  𝜀    [Null Set(M0)]                   08
    "#);
}

/// Regression test: star first-child with array capture preserves Arr/EndArr.
///
/// Bug: When compiling `(parent (child)* @arr (sibling) @s)` as first-child,
/// the array scope (Arr/EndArr) was being stripped. The fix must preserve
/// array semantics while also handling skip navigation.
#[test]
fn regression_star_first_child_array_capture() {
    let input = "Q = (array (identifier)* @ids (number) @n)";

    let res = Query::expect_valid_bytecode(input);

    insta::assert_snapshot!(res, @r#"
    [header]
    linked = false

    [strings]
    S00 "Beauty will save the world"
    S01 "ids"
    S02 "n"
    S03 "Q"
    S04 "array"
    S05 "number"
    S06 "identifier"

    [types.defs]
    T00 = void
    T01 = Node
    T02 = str
    T03 = ArrayStar(T01)  ; Node*
    T04 = Struct(M0, 2)  ; { ids, n }

    [types.members]
    M0 = (S01, T03)  ; ids: T03
    M1 = (S02, T01)  ; n: Node

    [types.names]
    N0 = (S03, T04)  ; Q

    [entry]
    Q = 01 :: T04

    [code]
      00  𝜀                                     ◼

    Q:
      01  𝜀                                     02
      02       (array)                          03
      03  𝜀    [Arr]                            05
      05  𝜀                                     16, 14
      07                                        ▶
      08  ↓*   (number) [Node Set(M1)]          22
      10  *    (number) [Node Set(M1)]          22
      12  𝜀    [EndArr Set(M0)]                 10
      14  𝜀    [EndArr Set(M0)]                 08
      16  ↓*   (identifier) [Push]              20
      18  *    (identifier) [Push]              20
      20  𝜀                                     18, 12
      22 *↑¹                                    07
    "#);
}

/// Regression test: struct array with internal captures needs Obj/EndObj.
///
/// Bug: When `{(a) @a (b) @b}* @items` is compiled, the struct boundaries
/// (Obj/EndObj) and Set effects are missing. Each iteration should produce:
/// Obj → Node Set(M0) → Node Set(M1) → EndObj Push
#[test]
fn regression_struct_array_internal_captures() {
    let input = "Q = (array {(identifier) @a (number) @b}* @items)";

    let res = Query::expect_valid_bytecode(input);

    insta::assert_snapshot!(res, @r#"
    [header]
    linked = false

    [strings]
    S00 "Beauty will save the world"
    S01 "a"
    S02 "b"
    S03 "items"
    S04 "Q"
    S05 "array"
    S06 "number"
    S07 "identifier"

    [types.defs]
    T00 = void
    T01 = Node
    T02 = str
    T03 = Struct(M0, 2)  ; { a, b }
    T04 = ArrayStar(T03)  ; T03*
    T05 = Struct(M2, 1)  ; { items }

    [types.members]
    M0 = (S01, T01)  ; a: Node
    M1 = (S02, T01)  ; b: Node
    M2 = (S03, T04)  ; items: T04

    [types.names]
    N0 = (S04, T05)  ; Q

    [entry]
    Q = 01 :: T05

    [code]
      00  𝜀                                     ◼

    Q:
      01  𝜀                                     02
      02       (array)                          03
      03  𝜀    [Arr]                            05
      05  𝜀                                     19, 11
      07  𝜀    [EndArr Set(M2)]                 09
      09 *↑¹                                    10
      10                                        ▶
      11  𝜀    [EndArr Set(M2)]                 10
      13  𝜀    [EndObj Push]                    29
      15  *    (number) [Node Set(M1)]          13
      17  ↓*   (identifier) [Node Set(M0)]      15
      19  𝜀    [Obj]                            17
      21  𝜀    [EndObj Push]                    29
      23  *    (number) [Node Set(M1)]          21
      25  *    (identifier) [Node Set(M0)]      23
      27  𝜀    [Obj]                            25
      29  𝜀                                     27, 07
    "#);
}
