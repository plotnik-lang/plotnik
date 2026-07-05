use super::tokenize;

#[test]
fn tokenize_reports_editor_classes_and_byte_ranges() {
    let query = "Q = (identifier == \"foo\") @id // line\nR = (string =~ /a\\/b/) @_\n'bad\n%";

    let rendered: Vec<_> = tokenize(query)
        .into_iter()
        .map(|span| {
            (
                span.kind,
                &query[span.start as usize..span.end as usize],
                span.start,
                span.end,
            )
        })
        .collect();

    assert_eq!(
        rendered,
        vec![
            ("ident", "Q", 0, 1),
            ("whitespace", " ", 1, 2),
            ("punct", "=", 2, 3),
            ("whitespace", " ", 3, 4),
            ("punct", "(", 4, 5),
            ("ident", "identifier", 5, 15),
            ("whitespace", " ", 15, 16),
            ("punct", "==", 16, 18),
            ("whitespace", " ", 18, 19),
            ("string", "\"", 19, 20),
            ("string", "foo", 20, 23),
            ("string", "\"", 23, 24),
            ("punct", ")", 24, 25),
            ("whitespace", " ", 25, 26),
            ("capture", "@id", 26, 29),
            ("whitespace", " ", 29, 30),
            ("comment", "// line", 30, 37),
            ("whitespace", "\n", 37, 38),
            ("ident", "R", 38, 39),
            ("whitespace", " ", 39, 40),
            ("punct", "=", 40, 41),
            ("whitespace", " ", 41, 42),
            ("punct", "(", 42, 43),
            ("ident", "string", 43, 49),
            ("whitespace", " ", 49, 50),
            ("punct", "=~", 50, 52),
            ("whitespace", " ", 52, 53),
            ("regex", "/a\\/b/", 53, 59),
            ("punct", ")", 59, 60),
            ("whitespace", " ", 60, 61),
            ("capture", "@_", 61, 63),
            ("whitespace", "\n", 63, 64),
            ("string", "'bad", 64, 68),
            ("whitespace", "\n", 68, 69),
            ("error", "%", 69, 70),
        ]
    );
}
