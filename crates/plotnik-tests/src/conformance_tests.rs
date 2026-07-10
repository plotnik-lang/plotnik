use super::parse_fixture;

#[test]
fn fixture_source_keeps_unknown_dash_padded_lines() {
    let raw = "Q = (program)\n--- input ---\nconst before = 1;\n-- note --\nconst after = 2;\n--- output ---\n{}";

    let fixture = parse_fixture("06-vm/dash-line".into(), raw).expect("fixture parses");

    assert_eq!(fixture.query, "Q = (program)");
    assert_eq!(
        fixture.input,
        "const before = 1;\n-- note --\nconst after = 2;"
    );
    assert_eq!(fixture.ext, None);
}
