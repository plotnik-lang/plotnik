use crate::Query;
use indoc::indoc;

#[test]
fn printer_with_spans() {
    let q = Query::try_from("(call)").unwrap();
    insta::assert_snapshot!(q.printer().with_spans(true).dump(), @r"
    Root [0..6]
      Def [0..6]
        NamedNode [0..6] call
    ");
}

#[test]
fn printer_with_cardinalities() {
    let q = Query::try_from("(call)").unwrap();
    insta::assert_snapshot!(q.printer().with_cardinalities(true).dump(), @r"
    Root¹
      Def¹
        NamedNode¹ call
    ");
}

#[test]
fn printer_cst_with_trivia() {
    let q = Query::try_from("(a) (b)").unwrap();
    insta::assert_snapshot!(q.printer().raw(true).with_trivia(true).dump(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "a"
          ParenClose ")"
      Whitespace " "
      Def
        Tree
          ParenOpen "("
          Id "b"
          ParenClose ")"
    "#);
}

#[test]
fn printer_alt_branches() {
    let input = indoc! {r#"
        [A: (a) B: (b)]
    "#};
    let q = Query::try_from(input).unwrap();
    insta::assert_snapshot!(q.printer().dump(), @r"
    Root
      Def
        Alt
          Branch A:
            NamedNode a
          Branch B:
            NamedNode b
    ");
}

#[test]
fn printer_capture_with_type() {
    let q = Query::try_from("(call)@x :: T").unwrap();
    insta::assert_snapshot!(q.printer().dump(), @r"
    Root
      Def
        CapturedExpr @x :: T
          NamedNode call
    ");
}

#[test]
fn printer_quantifiers() {
    let q = Query::try_from("(a)* (b)+ (c)?").unwrap();
    insta::assert_snapshot!(q.printer().dump(), @r"
    Root
      Def
        QuantifiedExpr *
          NamedNode a
      Def
        QuantifiedExpr +
          NamedNode b
      Def
        QuantifiedExpr ?
          NamedNode c
    ");
}

#[test]
fn printer_field() {
    let q = Query::try_from("(call name: (id))").unwrap();
    insta::assert_snapshot!(q.printer().dump(), @r"
    Root
      Def
        NamedNode call
          FieldExpr name:
            NamedNode id
    ");
}

#[test]
fn printer_negated_field() {
    let q = Query::try_from("(call !name)").unwrap();
    insta::assert_snapshot!(q.printer().dump(), @r"
    Root
      Def
        NamedNode call
          NegatedField !name
    ");
}

#[test]
fn printer_wildcard_and_anchor() {
    let q = Query::try_from("(call _ . (arg))").unwrap();
    insta::assert_snapshot!(q.printer().dump(), @r"
    Root
      Def
        NamedNode call
          AnonymousNode (any)
          .
          NamedNode arg
    ");
}

#[test]
fn printer_string_literal() {
    let q = Query::try_from(r#"(call "foo")"#).unwrap();
    insta::assert_snapshot!(q.printer().dump(), @r#"
    Root
      Def
        NamedNode call
          AnonymousNode "foo"
    "#);
}

#[test]
fn printer_ref() {
    let input = indoc! {r#"
        Expr = (call)
        (func (Expr))
    "#};
    let q = Query::try_from(input).unwrap();
    insta::assert_snapshot!(q.printer().dump(), @r"
    Root
      Def Expr
        NamedNode call
      Def
        NamedNode func
          Ref Expr
    ");
}

#[test]
fn printer_symbols_with_cardinalities() {
    let input = indoc! {r#"
        A = (a)
        B = {(b) (c)}
        (entry (A) (B))
    "#};
    let q = Query::try_from(input).unwrap();
    insta::assert_snapshot!(q.printer().only_symbols(true).with_cardinalities(true).dump(), @r"
    A¹
    B⁺
    _
      A¹
      B⁺
    ");
}

#[test]
fn printer_symbols_with_refs() {
    let input = indoc! {r#"
        A = (a)
        B = (b (A))
        (entry (B))
    "#};
    let q = Query::try_from(input).unwrap();
    insta::assert_snapshot!(q.printer().only_symbols(true).dump(), @r"
    A
    B
      A
    _
      B
        A
    ");
}

#[test]
fn printer_symbols_cycle() {
    let input = indoc! {r#"
        A = [(a) (B)]
        B = [(b) (A)]
        (entry (A))
    "#};
    let q = Query::try_from(input).unwrap();
    insta::assert_snapshot!(q.printer().only_symbols(true).dump(), @r"
    A
      B
        A (cycle)
    B
      A
        B (cycle)
    _
      A
        B
          A (cycle)
    ");
}

#[test]
fn printer_symbols_undefined_ref() {
    let input = "(call (Undefined))";
    let q = Query::try_from(input).unwrap();
    insta::assert_snapshot!(q.printer().only_symbols(true).dump(), @r"
    _
      Undefined?
    ");
}

#[test]
fn printer_symbols_broken_ref() {
    let input = "A = (foo (Undefined))";
    let q = Query::try_from(input).unwrap();
    insta::assert_snapshot!(q.printer().only_symbols(true).dump(), @r"
    A
      Undefined?
    ");
}

#[test]
fn printer_spans_comprehensive() {
    let input = indoc! {r#"
        Foo = (call name: (id) !bar)
        [(a) (b)]
    "#};
    let q = Query::try_from(input).unwrap();
    insta::assert_snapshot!(q.printer().with_spans(true).dump(), @r"
    Root [0..39]
      Def [0..28] Foo
        NamedNode [6..28] call
          FieldExpr [12..22] name:
            NamedNode [18..22] id
          NegatedField [23..27] !bar
      Def [29..38]
        Alt [29..38]
          Branch [30..33]
            NamedNode [30..33] a
          Branch [34..37]
            NamedNode [34..37] b
    ");
}

#[test]
fn printer_spans_seq() {
    let q = Query::try_from("{(a) (b)}").unwrap();
    insta::assert_snapshot!(q.printer().with_spans(true).dump(), @r"
    Root [0..9]
      Def [0..9]
        Seq [0..9]
          NamedNode [1..4] a
          NamedNode [5..8] b
    ");
}

#[test]
fn printer_spans_quantifiers() {
    let q = Query::try_from("(a)* (b)+").unwrap();
    insta::assert_snapshot!(q.printer().with_spans(true).dump(), @r"
    Root [0..9]
      Def [0..4]
        QuantifiedExpr [0..4] *
          NamedNode [0..3] a
      Def [5..9]
        QuantifiedExpr [5..9] +
          NamedNode [5..8] b
    ");
}

#[test]
fn printer_spans_alt_branches() {
    let q = Query::try_from("[A: (a) B: (b)]").unwrap();
    insta::assert_snapshot!(q.printer().with_spans(true).dump(), @r"
    Root [0..15]
      Def [0..15]
        Alt [0..15]
          Branch [1..7] A:
            NamedNode [4..7] a
          Branch [8..14] B:
            NamedNode [11..14] b
    ");
}
