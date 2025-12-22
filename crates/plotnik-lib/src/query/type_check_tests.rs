use crate::Query;
use indoc::indoc;

#[test]
fn capture_single_node() {
    let input = "Q = (identifier) @name";

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
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
    }

    export type Identifier = Node;

    export interface Q {
      name: Identifier;
    }
    ");
}

#[test]
fn named_node_with_field_capture() {
    let input = "Q = (function name: (identifier) @name)";

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
    }

    export interface Q {
      name: Node;
    }
    ");
}

#[test]
fn named_node_multiple_field_captures() {
    let input = indoc! {r#"
      Q = (function
        name: (identifier) @name
        body: (block) @body
      )
    "#};

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
    }

    export interface Q {
      body: Node;
      name: Node;
    }
    ");
}

#[test]
fn nested_named_node_captures() {
    let input = indoc! {r#"
      Q = (call
        function: (member target: (identifier) @target)
      )
    "#};

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
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
    }

    export interface Q {
      names: [Node, ...Node[]];
    }
    ");
}

#[test]
fn row_list_basic() {
    let input = indoc! {r#"
      Q = {(key) @k (value) @v}* @rows
    "#};

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
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
      Q = {(key) @k (value) @v}+ @rows
    "#};

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
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
    }

    export interface Q {
      dec?: Node;
    }
    ");
}

#[test]
fn optional_group_bubbles_fields() {
    let input = indoc! {r#"
        Q = {(modifier) @mod (decorator) @dec}?
    "#};

    let res = Query::expect_valid_types(input);
    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
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
        Q = {(a) @a (b) @b}
    "#};

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
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
        Q = {(a) @a (b) @b} @row
    "#};

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
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
    let input = "Q = [(a) @x (b) @x]";

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
    }

    export interface Q {
      x: Node;
    }
    ");
}

#[test]
fn untagged_alt_different_captures() {
    let input = "Q = [(a) @a (b) @b]";

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
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
        {(a) @x (b) @y}
        {(a) @x}
      ]
    "#};

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
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
    }

    export interface QNum {
      $tag: "Num";
      $data: { n: Node };
    }

    export interface QStr {
      $tag: "Str";
      $data: { s: Node };
    }

    export type Q = QNum | QStr;
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
    }

    export interface QNum {
      $tag: "Num";
      $data: { n: Node };
    }

    export interface QStr {
      $tag: "Str";
      $data: { s: string };
    }

    export type Q = QNum | QStr;
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
    }

    export interface QResultNum {
      $tag: "Num";
      $data: { n: Node };
    }

    export interface QResultStr {
      $tag: "Str";
      $data: { s: Node };
    }

    export type QResult = QResultNum | QResultStr;

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
        {(key) @k (value) @v} @pair
      }
    "#};

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
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
      Bad = {(a) @a (b) @b}*
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: quantifier `*` contains captures (`@a`, `@b`) but no row capture
      |
    1 | Bad = {(a) @a (b) @b}*
      |       ^^^^^^^^^^^^^^^^
      |
    help: wrap as `{...}* @rows`
    ");
}

#[test]
fn error_plus_with_internal_capture_no_row() {
    let input = indoc! {r#"
      Bad = {(c) @c}+
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: quantifier `+` contains captures (`@c`) but no row capture
      |
    1 | Bad = {(c) @c}+
      |       ^^^^^^^^^
      |
    help: wrap as `{...}* @rows`
    ");
}

#[test]
fn error_named_node_with_capture_quantified() {
    let input = indoc! {r#"
      Bad = (func (identifier) @name)*
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: quantifier `*` contains captures (`@name`) but no row capture
      |
    1 | Bad = (func (identifier) @name)*
      |       ^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    help: wrap as `{...}* @rows`
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
    export interface ExprBinary {
      $tag: "Binary";
      $data: { left: Expr; right: Expr };
    }

    export interface ExprLit {
      $tag: "Lit";
      $data: { value: string };
    }

    export type Expr = ExprBinary | ExprLit;
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
      Item = (item (Item)* @children)
    "#};

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @r"
    export interface Item {
      children: Item[];
    }
    ");
}
