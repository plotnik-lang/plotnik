use crate::Query;
use indoc::indoc;

#[test]
fn multiple_definitions_all_emitted() {
    let input = indoc! {r#"
    Id = (identifier) @id
    Foo = (function_declaration name: (Id))
    Bar = (class_declaration name: (Id))
    "#};

    let res = Query::expect_valid_types(input);

    // All three definitions emitted: Id as primary, Foo and Bar as aliases
    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export interface Id {
      id: Node;
    }

    export type Foo = Id;

    export type Bar = Id;
    ");
}

#[test]
fn multiple_definitions_distinct_types() {
    let input = indoc! {r#"
    Name = (identifier) @name
    Value = (number) @value
    Both = (pair (identifier) @key (number) @val)
    "#};

    let res = Query::expect_valid_types(input);

    // All three definitions emitted with their own types
    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export interface Both {
      key: Node;
      val: Node;
    }

    export interface Value {
      value: Node;
    }

    export interface Name {
      name: Node;
    }
    ");
}

#[test]
fn capture_single_node() {
    let input = "Q = (identifier) @name";

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export interface Q {
      name: Node;
    }
    ");
}

#[test]
fn capture_with_string_annotation() {
    let input = "Q = (identifier) @name :: string";

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Q {
      name: string;
    }
    ");
}

#[test]
fn capture_with_custom_type() {
    let input = "Q = (identifier) @name :: Identifier";

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export type Identifier = Node;

    export interface Q {
      name: Identifier;
    }
    ");
}

#[test]
fn named_node_with_field_capture() {
    let input = "Q = (function_declaration name: (identifier) @name)";

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export interface Q {
      name: Node;
    }
    ");
}

#[test]
fn named_node_multiple_field_captures() {
    let input = indoc! {r#"
    Q = (function_declaration
      name: (identifier) @name
      body: (statement_block) @body
    )
    "#};

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export interface Q {
      body: Node;
      name: Node;
    }
    ");
}

#[test]
fn named_node_captured_with_internal_captures() {
    // Capturing a named node does NOT create a scope boundary.
    // Internal captures bubble up alongside the outer capture.
    let input = indoc! {r#"
    Q = (function_declaration
      name: (identifier) @name :: string
      body: (statement_block) @body
    ) @func :: FunctionInfo
    "#};

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export type FunctionInfo = Node;

    export interface Q {
      body: Node;
      func: FunctionInfo;
      name: string;
    }
    ");
}

#[test]
fn nested_named_node_captures() {
    let input = indoc! {r#"
    Q = (call_expression
      function: (member_expression object: (identifier) @target)
    )
    "#};

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export interface Q {
      target: Node;
    }
    ");
}

#[test]
fn scalar_list_zero_or_more() {
    let input = "Q = (decorator)* @decorators";

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export interface Q {
      decorators: Node[];
    }
    ");
}

#[test]
fn scalar_list_one_or_more() {
    let input = "Q = (identifier)+ @names";

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export interface Q {
      names: [Node, ...Node[]];
    }
    ");
}

#[test]
fn scalar_list_with_string_annotation_zero_or_more() {
    let input = "Q = (identifier)* @names :: string";

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Q {
      names: string[];
    }
    ");
}

#[test]
fn scalar_list_with_string_annotation_one_or_more() {
    let input = "Q = (identifier)+ @names :: string";

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Q {
      names: [string, ...string[]];
    }
    ");
}

#[test]
fn row_list_basic() {
    let input = indoc! {r#"
    Q = {(identifier) @k (number) @v}* @rows
    "#};

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export interface QRows {
      k: Node;
      v: Node;
    }

    export interface Q {
      rows: QRows[];
    }
    ");
}

#[test]
fn row_list_non_empty() {
    let input = indoc! {r#"
    Q = {(identifier) @k (number) @v}+ @rows
    "#};

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export interface QRows {
      k: Node;
      v: Node;
    }

    export interface Q {
      rows: [QRows, ...QRows[]];
    }
    ");
}

#[test]
fn optional_single_capture() {
    let input = "Q = (decorator)? @dec";

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export interface Q {
      dec?: Node;
    }
    ");
}

#[test]
fn optional_group_bubbles_fields() {
    let input = indoc! {r#"
    Q = {(identifier) @mod (decorator) @dec}?
    "#};

    let res = Query::expect_valid_types(input);
    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export interface Q {
      dec?: Node;
      mod?: Node;
    }
    ");
}

#[test]
fn sequence_merges_fields() {
    let input = indoc! {r#"
    Q = {(identifier) @a (number) @b}
    "#};

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export interface Q {
      a: Node;
      b: Node;
    }
    ");
}

#[test]
fn captured_sequence_creates_struct() {
    let input = indoc! {r#"
    Q = {(identifier) @a (number) @b} @row
    "#};

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export interface QRow {
      a: Node;
      b: Node;
    }

    export interface Q {
      row: QRow;
    }
    ");
}

#[test]
fn untagged_alt_same_capture_all_branches() {
    let input = "Q = [(identifier) @x (number) @x]";

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export interface Q {
      x: Node;
    }
    ");
}

#[test]
fn untagged_alt_different_captures() {
    let input = "Q = [(identifier) @a (number) @b]";

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export interface Q {
      a?: Node;
      b?: Node;
    }
    ");
}

#[test]
fn untagged_alt_partial_overlap() {
    let input = indoc! {r#"
    Q = [
      {(identifier) @x (number) @y}
      {(identifier) @x}
    ]
    "#};

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export interface Q {
      x: Node;
      y?: Node;
    }
    ");
}

#[test]
fn tagged_alt_basic() {
    let input = indoc! {r#"
    Q = [
      Str: (string) @s
      Num: (number) @n
    ]
    "#};

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r#"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export interface QStr {
      $tag: "Str";
      $data: { s: Node };
    }

    export interface QNum {
      $tag: "Num";
      $data: { n: Node };
    }

    export type Q = QStr | QNum;
    "#);
}

#[test]
fn tagged_alt_with_type_annotation() {
    let input = indoc! {r#"
    Q = [
      Str: (string) @s :: string
      Num: (number) @n
    ]
    "#};

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r#"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export interface QStr {
      $tag: "Str";
      $data: { s: string };
    }

    export interface QNum {
      $tag: "Num";
      $data: { n: Node };
    }

    export type Q = QStr | QNum;
    "#);
}

#[test]
fn tagged_alt_captured() {
    let input = indoc! {r#"
    Q = [
      Str: (string) @s
      Num: (number) @n
    ] @result
    "#};

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r#"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export interface QResultStr {
      $tag: "Str";
      $data: { s: Node };
    }

    export interface QResultNum {
      $tag: "Num";
      $data: { n: Node };
    }

    export type QResult = QResultStr | QResultNum;

    export interface Q {
      result: QResult;
    }
    "#);
}

#[test]
fn nested_captured_group() {
    let input = indoc! {r#"
    Q = {
      (identifier) @name
      {(string) @k (number) @v} @pair
    }
    "#};

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export interface QPair {
      k: Node;
      v: Node;
    }

    export interface Q {
      name: Node;
      pair: QPair;
    }
    ");
}

#[test]
fn error_star_with_internal_captures_no_row() {
    let input = indoc! {r#"
    Bad = {(identifier) @a (number) @b}*
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: quantifier `*` contains captures (`@a`, `@b`) but has no struct capture
      |
    1 | Bad = {(identifier) @a (number) @b}*
      |       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    help: add a struct capture: `{...}* @name`
    ");
}

#[test]
fn error_plus_with_internal_capture_no_row() {
    let input = indoc! {r#"
    Bad = {(identifier) @c}+
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: quantifier `+` contains captures (`@c`) but has no struct capture
      |
    1 | Bad = {(identifier) @c}+
      |       ^^^^^^^^^^^^^^^^^^
      |
    help: add a struct capture: `{...}+ @name`
    ");
}

#[test]
fn error_named_node_with_capture_quantified() {
    let input = indoc! {r#"
    Bad = (array (identifier) @name)*
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: quantifier `*` contains captures (`@name`) but has no struct capture
      |
    1 | Bad = (array (identifier) @name)*
      |       ^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    help: add a struct capture: `{...}* @name`
    ");
}

#[test]
fn error_multi_element_sequence_no_captures() {
    let input = "Bad = {(identifier) (number)}* @items";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: sequence with `*` matches multiple nodes but has no internal captures
      |
    1 | Bad = {(identifier) (number)}* @items
      |       ^^^^^^^^^^^^^^^^^^^^^^^^
      |
    help: add internal captures: `{(a) @a (b) @b}* @items`
    ");
}

#[test]
fn error_multi_element_alternation_branch() {
    // Alternation where one branch is multi-element
    let input = "Bad = [(identifier) {(number) (string)}]* @items";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: sequence with `*` matches multiple nodes but has no internal captures
      |
    1 | Bad = [(identifier) {(number) (string)}]* @items
      |       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    help: add internal captures: `{(a) @a (b) @b}* @items`
    ");
}

#[test]
fn recursive_type_with_alternation() {
    let input = indoc! {r#"
    Expr = [
      Lit: (number) @value ::string
      Binary: (binary_expression
        left: (Expr) @left
        right: (Expr) @right)
    ]
    "#};

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r#"
    export interface ExprLit {
      $tag: "Lit";
      $data: { value: string };
    }

    export interface ExprBinary {
      $tag: "Binary";
      $data: { left: Expr; right: Expr };
    }

    export type Expr = ExprLit | ExprBinary;
    "#);
}

#[test]
fn recursive_type_optional_self_ref() {
    let input = indoc! {r#"
    NestedCall = (call_expression
      function: [
        (identifier) @name
        (NestedCall) @inner
      ]
    )
    "#};

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export interface NestedCall {
      inner?: NestedCall;
      name?: Node;
    }
    ");
}

#[test]
fn recursive_type_in_quantified_context() {
    let input = indoc! {r#"
    Item = (array (Item)* @children)
    "#};

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Item {
      children: Item[];
    }
    ");
}

#[test]
fn recursive_type_uncaptured_propagates() {
    // Regression test: Q = (Rec) should inherit Rec's enum type, not infer as void.
    // The recursive definition Rec is a tagged alternation, so its type propagates
    // through the uncaptured reference.
    let input = indoc! {r#"
    Rec = [A: (program (expression_statement (Rec) @inner)) B: (identifier) @id]
    Q = (Rec)
    "#};

    let res = Query::expect_valid_types(input);

    // Q should have type Rec (aliased to the enum)
    insta::assert_snapshot!(res, @r#"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export interface RecA {
      $tag: "A";
      $data: { inner: Rec };
    }

    export interface RecB {
      $tag: "B";
      $data: { id: Node };
    }

    export type Rec = RecA | RecB;

    export type Q = Rec;
    "#);
}

#[test]
fn scalar_propagates_through_named_node() {
    let input = indoc! {r#"
    A = [X: (identifier) @x Y: (number) @y]
    Q = (program (A))
    "#};

    let res = Query::expect_valid_types(input);

    // Q should inherit the enum type from A
    insta::assert_snapshot!(res, @r#"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export interface AX {
      $tag: "X";
      $data: { x: Node };
    }

    export interface AY {
      $tag: "Y";
      $data: { y: Node };
    }

    export type A = AX | AY;

    export type Q = A;
    "#);
}

#[test]
fn scalar_array_propagates_through_named_node() {
    let input = indoc! {r#"
    A = [X: (identifier) @x Y: (number) @y]
    Q = (program (A)+)
    "#};

    let res = Query::expect_valid_types(input);

    // Q should be A[] (non-empty array of the enum)
    insta::assert_snapshot!(res, @r#"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export interface AX {
      $tag: "X";
      $data: { x: Node };
    }

    export interface AY {
      $tag: "Y";
      $data: { y: Node };
    }

    export type A = AX | AY;

    export type Q = [A, ...A[]];
    "#);
}

#[test]
fn scalar_propagates_through_sequence() {
    let input = indoc! {r#"
    A = [X: (identifier) @x Y: (number) @y]
    Q = {(A)}
    "#};

    let res = Query::expect_valid_types(input);

    // Q should inherit the enum type from A
    insta::assert_snapshot!(res, @r#"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export interface AX {
      $tag: "X";
      $data: { x: Node };
    }

    export interface AY {
      $tag: "Y";
      $data: { y: Node };
    }

    export type A = AX | AY;

    export type Q = A;
    "#);
}

#[test]
fn error_multiple_uncaptured_outputs() {
    let input = indoc! {r#"
    A = [X: (identifier)]
    B = [Y: (number)]
    Q = (program (A) (B))
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: multiple expressions produce output without capture: 2 expressions produce output without capture
      |
    3 | Q = (program (A) (B))
      |     ^^^^^^^^^^^^^^^^^
      |
    help: capture each expression explicitly: `(X) @x (Y) @y`
    ");
}

#[test]
fn error_uncaptured_output_with_captures() {
    let input = indoc! {r#"
    A = [X: (identifier)]
    Q = (program (A) (number) @name)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: output-producing expression requires capture when siblings have captures
      |
    2 | Q = (program (A) (number) @name)
      |              ^^^
      |
    help: add `@name` to capture the output
    ");
}

#[test]
fn output_captured_with_bubbles_ok() {
    let input = indoc! {r#"
    A = [X: (identifier) Y: (number)]
    Q = (program (A) @a (string) @name)
    "#};

    let res = Query::expect_valid_types(input);

    // Q should be { a: A, name: Node }
    // Note: enum variants without captures have no $data field (void payload)
    insta::assert_snapshot!(res, @r#"
    export interface Node {
      kind: string;
      text: string;
      span: [number, number];
    }

    export interface AX {
      $tag: "X";
    }

    export interface AY {
      $tag: "Y";
    }

    export type A = AX | AY;

    export interface Q {
      a: A;
      name: Node;
    }
    "#);
}
