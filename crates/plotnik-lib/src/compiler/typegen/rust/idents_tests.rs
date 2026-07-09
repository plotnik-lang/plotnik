use super::idents::scope_idents;

fn idents(names: &[&str]) -> Vec<String> {
    scope_idents(names.iter().copied())
}

#[test]
fn plain_names_pass_through() {
    assert_eq!(idents(&["name", "num"]), ["name", "num"]);
}

#[test]
fn keywords_render_raw() {
    assert_eq!(
        idents(&["type", "fn", "match"]),
        ["r#type", "r#fn", "r#match"]
    );
}

#[test]
fn unraw_keywords_get_trailing_underscore() {
    assert_eq!(
        idents(&["self", "super", "crate", "Self"]),
        ["self_", "super_", "crate_", "Self_"]
    );
}

#[test]
fn underscore_rename_disambiguates_against_taken_name() {
    assert_eq!(idents(&["self_", "self"]), ["self_", "self__"]);
}

#[test]
fn edition_2024_reserved_word_renders_raw() {
    assert_eq!(idents(&["gen"]), ["r#gen"]);
}
