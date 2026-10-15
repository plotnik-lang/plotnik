#[test]
fn macro_diagnostics() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/macro_diagnostics/pass/*.rs");
    cases.compile_fail("tests/macro_diagnostics/fail/*.rs");
}
