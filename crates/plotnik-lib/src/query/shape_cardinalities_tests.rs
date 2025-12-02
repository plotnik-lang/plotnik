use crate::Query;
use indoc::indoc;

#[test]
fn tree_is_one() {
    let query = Query::new("(identifier)");
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_cardinalities(), @r"
    Root¹
      Def¹
        Tree¹ identifier
    ");
}

#[test]
fn singleton_seq_is_one() {
    let query = Query::new("{(identifier)}");
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_cardinalities(), @r"
    Root¹
      Def¹
        Seq¹
          Tree¹ identifier
    ");
}

#[test]
fn nested_singleton_seq_is_one() {
    let query = Query::new("{{{(identifier)}}}");
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_cardinalities(), @r"
    Root¹
      Def¹
        Seq¹
          Seq¹
            Seq¹
              Tree¹ identifier
    ");
}

#[test]
fn multi_seq_is_many() {
    let query = Query::new("{(a) (b)}");
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_cardinalities(), @r"
    Root¹
      Def⁺
        Seq⁺
          Tree¹ a
          Tree¹ b
    ");
}

#[test]
fn alt_is_one() {
    let query = Query::new("[(a) (b)]");
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_cardinalities(), @r"
    Root¹
      Def¹
        Alt¹
          Branch¹
            Tree¹ a
          Branch¹
            Tree¹ b
    ");
}

#[test]
fn alt_with_seq_branches() {
    let input = indoc! {r#"
    [{(a) (b)} (c)]
    "#};
    let query = Query::new(input);
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_cardinalities(), @r"
    Root¹
      Def¹
        Alt¹
          Branch⁺
            Seq⁺
              Tree¹ a
              Tree¹ b
          Branch¹
            Tree¹ c
    ");
}

#[test]
fn ref_to_tree_is_one() {
    let input = indoc! {r#"
    X = (identifier)
    (call (X))
    "#};
    let query = Query::new(input);
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_cardinalities(), @r"
    Root⁺
      Def¹ X
        Tree¹ identifier
      Def¹
        Tree¹ call
          Ref¹ X
    ");
}

#[test]
fn ref_to_seq_is_many() {
    let input = indoc! {r#"
    X = {(a) (b)}
    (call (X))
    "#};
    let query = Query::new(input);
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_cardinalities(), @r"
    Root⁺
      Def⁺ X
        Seq⁺
          Tree¹ a
          Tree¹ b
      Def¹
        Tree¹ call
          Ref⁺ X
    ");
}

#[test]
fn field_with_tree() {
    let query = Query::new("(call name: (identifier))");
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_cardinalities(), @r"
    Root¹
      Def¹
        Tree¹ call
          Field¹ name:
            Tree¹ identifier
    ");
}

#[test]
fn field_with_alt() {
    let query = Query::new("(call name: [(identifier) (string)])");
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_cardinalities(), @r"
    Root¹
      Def¹
        Tree¹ call
          Field¹ name:
            Alt¹
              Branch¹
                Tree¹ identifier
              Branch¹
                Tree¹ string
    ");
}

#[test]
fn field_with_seq_error() {
    let query = Query::new("(call name: {(a) (b)})");
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_with_cardinalities(), @r"
    Root¹
      Def¹
        Tree¹ call
          Field¹ name:
            Seq⁺
              Tree¹ a
              Tree¹ b
    ");
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: field `name` value must match a single node, not a sequence
      |
    1 | (call name: {(a) (b)})
      |             ^^^^^^^^^ field `name` value must match a single node, not a sequence
    ");
}

#[test]
fn field_with_ref_to_seq_error() {
    let input = indoc! {r#"
    X = {(a) (b)}
    (call name: (X))
    "#};
    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_with_cardinalities(), @r"
    Root⁺
      Def⁺ X
        Seq⁺
          Tree¹ a
          Tree¹ b
      Def¹
        Tree¹ call
          Field¹ name:
            Ref⁺ X
    ");
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: field `name` value must match a single node, not a sequence
      |
    2 | (call name: (X))
      |             ^^^ field `name` value must match a single node, not a sequence
    ");
}

#[test]
fn quantifier_preserves_inner_shape() {
    let query = Query::new("(identifier)*");
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_cardinalities(), @r"
    Root¹
      Def¹
        Quantifier¹ *
          Tree¹ identifier
    ");
}

#[test]
fn capture_preserves_inner_shape() {
    let query = Query::new("(identifier) @name");
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_cardinalities(), @r"
    Root¹
      Def¹
        Capture¹ @name
          Tree¹ identifier
    ");
}

#[test]
fn capture_on_seq() {
    let query = Query::new("{(a) (b)} @items");
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_cardinalities(), @r"
    Root¹
      Def⁺
        Capture⁺ @items
          Seq⁺
            Tree¹ a
            Tree¹ b
    ");
}

#[test]
fn complex_nested_shapes() {
    let input = indoc! {r#"
    Stmt = [(expr_stmt) (return_stmt)]
    (function_definition
        name: (identifier) @name
        body: (block (Stmt)* @stmts))
    "#};
    let query = Query::new(input);
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_cardinalities(), @r"
    Root⁺
      Def¹ Stmt
        Alt¹
          Branch¹
            Tree¹ expr_stmt
          Branch¹
            Tree¹ return_stmt
      Def¹
        Tree¹ function_definition
          Capture¹ @name
            Field¹ name:
              Tree¹ identifier
          Field¹ body:
            Tree¹ block
              Capture¹ @stmts
                Quantifier¹ *
                  Ref¹ Stmt
    ");
}

#[test]
fn tagged_alt_shapes() {
    let input = indoc! {r#"
    [Ident: (identifier) Num: (number)]
    "#};
    let query = Query::new(input);
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_cardinalities(), @r"
    Root¹
      Def¹
        Alt¹
          Branch¹ Ident:
            Tree¹ identifier
          Branch¹ Num:
            Tree¹ number
    ");
}

#[test]
fn anchor_is_one() {
    let query = Query::new("(block . (statement))");
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_cardinalities(), @r"
    Root¹
      Def¹
        Tree¹ block
          Anchor¹
          Tree¹ statement
    ");
}

#[test]
fn negated_field_is_one() {
    let query = Query::new("(function !async)");
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_cardinalities(), @r"
    Root¹
      Def¹
        Tree¹ function
          NegatedField¹ !async
    ");
}

#[test]
fn tree_with_wildcard_type() {
    let query = Query::new("(_)");
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_cardinalities(), @r"
    Root¹
      Def¹
        Tree¹ _
    ");
}

#[test]
fn bare_wildcard_is_one() {
    let query = Query::new("_");
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_cardinalities(), @r"
    Root¹
      Def¹
        Wildcard¹
    ");
}

#[test]
fn empty_seq_is_one() {
    let query = Query::new("{}");
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_cardinalities(), @r"
    Root¹
      Def¹
        Seq¹
    ");
}

#[test]
fn literal_is_one() {
    let query = Query::new(r#""if""#);
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_cardinalities(), @r#"
    Root¹
      Def¹
        Str¹ "if"
    "#);
}
