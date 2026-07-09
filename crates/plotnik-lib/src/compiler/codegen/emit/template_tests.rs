use crate::compiler::codegen::emit::template::splice;

#[test]
fn substitutes_and_indents_non_empty_lines() {
    let mut out = String::new();

    splice(
        &mut out,
        "    ",
        "\nfn @NAME@() {\n\n    @BODY@\n}\n",
        &[("NAME", "run"), ("BODY", "work();")],
    );

    assert_eq!(out, "    fn run() {\n\n        work();\n    }\n");
}

#[test]
#[should_panic(expected = "unsubstituted template placeholder `@BODY@`")]
fn rejects_unsubstituted_placeholders() {
    splice(&mut String::new(), "", "@BODY@", &[]);
}
