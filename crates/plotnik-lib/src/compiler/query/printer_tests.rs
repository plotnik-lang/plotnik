use crate::compiler::Query;
use indoc::indoc;

#[test]
fn printer_with_spans() {
    let input = "Q = (call)";
    let q = Query::expect(input);

    let res = q.printer().with_spans(true).dump();

    insta::assert_snapshot!(res, @r"
    Root [0..10]
      Def [0..10] Q
        NamedNode [4..10] call
    ");
}

#[test]
fn printer_with_arities() {
    let input = "Q = (call)";
    let q = Query::expect(input);

    let res = q.printer().with_arities(true).dump();

    insta::assert_snapshot!(res, @r"
    Root¹
      Def¹ Q
        NamedNode¹ call
    ");
}

#[test]
fn printer_symbols_with_arities() {
    let input = indoc! {r#"
        A = (a)
        B = {(b) (c)}
        Q = (entry (A) (B))
    "#};
    let q = Query::expect(input);

    let res = q.printer().definitions_only(true).with_arities(true).dump();

    insta::assert_snapshot!(res, @r"
    A¹
    B⁺
    Q¹
      A¹
      B⁺
    ");
}

#[test]
fn printer_symbols_with_refs() {
    let input = indoc! {r#"
        A = (a)
        B = (b (A))
        Q = (entry (B))
    "#};
    let q = Query::expect(input);

    let res = q.printer().definitions_only(true).dump();

    insta::assert_snapshot!(res, @r"
    A
    B
      A
    Q
      B
        A
    ");
}

#[test]
fn printer_symbols_cycle() {
    let input = indoc! {r#"
        A = [(a) (B)]
        B = [(b) (A)]
        Q = (entry (A))
    "#};
    let q = Query::expect(input);

    let res = q.printer().definitions_only(true).dump();

    insta::assert_snapshot!(res, @r"
    A
      B
        A (cycle)
    B
      A
        B (cycle)
    Q
      A
        B
          A (cycle)
    ");
}

#[test]
fn printer_symbols_undefined_ref() {
    let input = "Q = (call (Undefined))";
    let q = Query::expect(input);

    let res = q.printer().definitions_only(true).dump();

    insta::assert_snapshot!(res, @r"
    Q
      Undefined?
    ");
}

#[test]
fn printer_symbols_broken_ref() {
    let input = "A = (foo (Undefined))";
    let q = Query::expect(input);

    let res = q.printer().definitions_only(true).dump();

    insta::assert_snapshot!(res, @r"
    A
      Undefined?
    ");
}

#[test]
fn printer_spans_comprehensive() {
    let input = indoc! {r#"
        Foo = (call name: (id) -bar)
        Q = [(a) (b)]
    "#};
    let q = Query::expect(input);

    let res = q.printer().with_spans(true).dump();

    insta::assert_snapshot!(res, @"
    Root [0..43]
      Def [0..28] Foo
        NamedNode [6..28] call
          FieldPattern [12..22] name:
            NamedNode [18..22] id
          NegatedField [23..27] -bar
      Def [29..42] Q
        Union [33..42]
          Branch [34..37]
            NamedNode [34..37] a
          Branch [38..41]
            NamedNode [38..41] b
    ");
}

#[test]
fn printer_spans_seq() {
    let input = "Q = {(a) (b)}";
    let q = Query::expect(input);

    let res = q.printer().with_spans(true).dump();

    insta::assert_snapshot!(res, @r"
    Root [0..13]
      Def [0..13] Q
        Seq [4..13]
          NamedNode [5..8] a
          NamedNode [9..12] b
    ");
}

#[test]
fn printer_spans_quantifiers() {
    let input = "Q = { (a)* (b)+ }";
    let q = Query::expect(input);

    let res = q.printer().with_spans(true).dump();

    insta::assert_snapshot!(res, @r"
    Root [0..17]
      Def [0..17] Q
        Seq [4..17]
          QuantifiedPattern [6..10] *
            NamedNode [6..9] a
          QuantifiedPattern [11..15] +
            NamedNode [11..14] b
    ");
}

#[test]
fn printer_spans_alt_branches() {
    let input = "Q = [A: (a) B: (b)]";
    let q = Query::expect(input);

    let res = q.printer().with_spans(true).dump();

    insta::assert_snapshot!(res, @"
    Root [0..19]
      Def [0..19] Q
        Enum [4..19]
          Branch [5..11] A:
            NamedNode [8..11] a
          Branch [12..18] B:
            NamedNode [15..18] b
    ");
}
