use super::shebang::{ShebangDecl, parse_shebang};

#[test]
fn no_shebang() {
    let result = parse_shebang("(identifier) @id").unwrap();

    assert_eq!(result, None);
}

#[test]
fn shebang_without_plotnik_is_ignored() {
    let result = parse_shebang("#!/bin/sh\n(identifier)").unwrap();

    assert_eq!(result, None);
}

#[test]
fn canonical_env_form() {
    let input = "#!/usr/bin/env -S plotnik run -l typescript\n(identifier)";

    let result = parse_shebang(input).unwrap().unwrap();

    assert_eq!(result.lang.as_deref(), Some("typescript"));
    assert_eq!(result.entry, None);
}

#[test]
fn direct_path_form() {
    let result = parse_shebang("#!/usr/local/bin/plotnik run -l rust")
        .unwrap()
        .unwrap();

    assert_eq!(result.lang.as_deref(), Some("rust"));
}

#[test]
fn no_subcommand() {
    let result = parse_shebang("#!/usr/bin/env -S plotnik -l js")
        .unwrap()
        .unwrap();

    assert_eq!(result.lang.as_deref(), Some("js"));
}

#[test]
fn entry_option() {
    let result = parse_shebang("#!/usr/bin/env -S plotnik run -l typescript --entry Func")
        .unwrap()
        .unwrap();

    assert_eq!(result.lang.as_deref(), Some("typescript"));
    assert_eq!(result.entry.as_deref(), Some("Func"));
}

#[test]
fn bare_plotnik_declares_nothing() {
    let result = parse_shebang("#!/usr/bin/env plotnik").unwrap().unwrap();

    assert_eq!(result, ShebangDecl::default());
}

#[test]
fn presentation_flags_accepted_and_ignored() {
    let result = parse_shebang("#!/usr/bin/env -S plotnik run -l ts --compact --color never")
        .unwrap()
        .unwrap();

    assert_eq!(result.lang.as_deref(), Some("ts"));
}

#[test]
fn malformed_option_is_loud() {
    let err = parse_shebang("#!/usr/bin/env -S plotnik run --frobnicate").unwrap_err();

    insta::assert_snapshot!(err, @"
    unexpected argument '--frobnicate' found

    expected form: #!/usr/bin/env -S plotnik run -l <lang>
    ");
}

#[test]
fn positional_in_shebang_is_rejected() {
    let err = parse_shebang("#!/usr/bin/env -S plotnik run query.ptk -l ts").unwrap_err();

    insta::assert_snapshot!(err, @"
    unexpected argument 'query.ptk' found

    expected form: #!/usr/bin/env -S plotnik run -l <lang>
    ");
}

#[test]
fn only_first_line_is_considered() {
    let input = "(a)\n#!/usr/bin/env -S plotnik run -l ts";

    let result = parse_shebang(input).unwrap();

    assert_eq!(result, None);
}
