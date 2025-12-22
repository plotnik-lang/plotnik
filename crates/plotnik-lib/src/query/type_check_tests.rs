use crate::Query;
use indoc::indoc;

// =============================================================================
// BASIC CAPTURES
// =============================================================================

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
    export interface Node {
      kind: string;
      text: string;
    }

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

    export interface Q {
      name: Identifier;
    }
    ");
}

// =============================================================================
// NAMED NODE FLOW PROPAGATION (Bug #2)
// =============================================================================

#[test]
fn named_node_with_field_capture() {
    // Child capture should bubble up through named node
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
    let input = "Q = (function name: (identifier) @name body: (block) @body)";
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
    let input = "Q = (call function: (member target: (identifier) @target))";
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

// =============================================================================
// SCALAR LISTS (Bug #1)
// =============================================================================

#[test]
fn scalar_list_zero_or_more() {
    // No internal captures → scalar list: Node[]
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
    // No internal captures → non-empty array: [Node, ...Node[]]
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

// =============================================================================
// ROW LISTS
// =============================================================================

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

    export interface Q {
      rows: Struct[];
    }

    export interface Struct {
      k: Node;
      v: Node;
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

    export interface Q {
      rows: [Struct, ...Struct[]];
    }

    export interface Struct {
      k: Node;
      v: Node;
    }
    ");
}

// =============================================================================
// OPTIONAL PATTERNS
// =============================================================================

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
      dec: Node;
    }
    ");
}

#[test]
fn optional_group_bubbles_fields() {
    // ? does NOT require row capture; fields bubble as optional
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

// =============================================================================
// SEQUENCES
// =============================================================================

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

    export interface Q {
      row: Struct;
    }

    export interface Struct {
      a: Node;
      b: Node;
    }
    ");
}

// =============================================================================
// UNTAGGED ALTERNATIONS (merge style)
// =============================================================================

#[test]
fn untagged_alt_same_capture_all_branches() {
    // Same capture in all branches → required field
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
    // Different captures → both optional
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
    // Partial overlap → common required, others optional
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

// =============================================================================
// TAGGED ALTERNATIONS (Bug #3)
// =============================================================================

#[test]
fn tagged_alt_basic() {
    let input = indoc! {r#"
        Q = [Str: (string) @s  Num: (number) @n]
    "#};
    let res = Query::expect_valid_types(input);
    insta::assert_snapshot!(res, @r#"
    export interface Node {
      kind: string;
      text: string;
    }

    export interface QNum {
      $tag: "Num";
      $data: Struct2;
    }

    export interface Struct2 {
      n: Node;
    }

    export interface QStr {
      $tag: "Str";
      $data: Struct;
    }

    export interface Struct {
      s: Node;
    }

    export type Q = QNum | QStr;
    "#);
}

#[test]
fn tagged_alt_with_type_annotation() {
    let input = indoc! {r#"
        Q = [Str: (string) @s ::string  Num: (number) @n]
    "#};
    let res = Query::expect_valid_types(input);
    insta::assert_snapshot!(res, @r#"
    export interface Node {
      kind: string;
      text: string;
    }

    export interface QNum {
      $tag: "Num";
      $data: Struct2;
    }

    export interface Struct2 {
      n: Node;
    }

    export interface QStr {
      $tag: "Str";
      $data: Struct;
    }

    export interface Struct {
      s: string;
    }

    export type Q = QNum | QStr;
    "#);
}

#[test]
fn tagged_alt_captured() {
    // Captured tagged alternation
    let input = indoc! {r#"
        Q = [Str: (string) @s  Num: (number) @n] @result
    "#};
    let res = Query::expect_valid_types(input);
    insta::assert_snapshot!(res, @r"
    export interface Node {
      kind: string;
      text: string;
    }

    export interface Q {
      result: Enum;
    }
    ");
}

// =============================================================================
// NESTED STRUCTURES
// =============================================================================

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

    export interface Q {
      name: Node;
      pair: Struct;
    }

    export interface Struct {
      k: Node;
      v: Node;
    }
    ");
}

// =============================================================================
// STRICT DIMENSIONALITY VIOLATIONS (errors)
// =============================================================================

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
    // (func (id) @name)* has internal capture
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
