use crate::compiler::Query;

#[test]
fn category_subtype_shares_id_grammar_with_slash() {
    // The subtype after `#` is a plain `Id`, so `.`/`-` are admitted exactly as the `/` form
    // admits them — the two spellings parse identically (and both collapse to the supertype).
    let hash = Query::expect_valid_ast("Q = (expression#member-expression)");
    let slash = Query::expect_valid_ast("Q = (expression/member-expression)");
    assert_eq!(hash, slash);
}
