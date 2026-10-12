//! Diagnostic snapshots for `query!` (trybuild): the fail cases pin the
//! `compile_error!` text — argument mistakes, grammar-resolution failures,
//! and the annotate-snippets rendering of query diagnostics — and the pass
//! cases pin that the legal shapes stay legal. Regenerate the `.stderr`
//! snapshots with `make shot` (`TRYBUILD=overwrite`).

#[test]
fn macro_diagnostics() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/macro_diagnostics/pass/*.rs");
    cases.compile_fail("tests/macro_diagnostics/fail/*.rs");
}
