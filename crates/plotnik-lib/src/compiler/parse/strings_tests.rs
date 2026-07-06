use std::borrow::Cow;

use super::strings::{EscapeIssue, EscapeIssueKind, unescape};

#[test]
fn no_backslash_returns_borrowed_text() {
    let (decoded, issues) = unescape("plain");

    assert!(matches!(decoded, Cow::Borrowed("plain")));
    assert!(issues.is_empty());
}

#[test]
fn simple_escapes_decode() {
    let cases = [
        (r"\n", "\n"),
        (r"\r", "\r"),
        (r"\t", "\t"),
        (r"\\", "\\"),
        (r#"\""#, "\""),
        (r"\'", "'"),
    ];

    for (raw, expected) in cases {
        let (decoded, issues) = unescape(raw);

        assert_eq!(decoded, expected);
        assert!(issues.is_empty());
    }
}

#[test]
fn unicode_escapes_decode() {
    let (decoded, issues) = unescape(r"\u{41} \u{1f600}");

    assert_eq!(decoded, "A 😀");
    assert!(issues.is_empty());
}

#[test]
fn unknown_escape_is_preserved_and_reported() {
    let (decoded, issues) = unescape(r"a\qb");

    assert_eq!(decoded, r"a\qb");
    assert_eq!(
        issues,
        vec![EscapeIssue {
            range: 1..3,
            kind: EscapeIssueKind::Unknown,
        }]
    );
}

#[test]
fn invalid_unicode_escapes_are_preserved_and_reported() {
    let cases = [
        r"\u",
        r"\u{}",
        r"\u{zz}",
        r"\u{1234567}",
        r"\u{d800}",
        r"\u{110000}",
        r"\u{12",
    ];

    for raw in cases {
        let (decoded, issues) = unescape(raw);

        assert_eq!(decoded, raw);
        assert_eq!(
            issues,
            vec![EscapeIssue {
                range: 0..raw.len(),
                kind: EscapeIssueKind::InvalidUnicode,
            }]
        );
    }
}

#[test]
fn mixed_valid_and_invalid_escapes_decode_independently() {
    let (decoded, issues) = unescape(r"a\nb\qc");

    assert_eq!(decoded, "a\nb\\qc");
    assert_eq!(
        issues,
        vec![EscapeIssue {
            range: 4..6,
            kind: EscapeIssueKind::Unknown,
        }]
    );
}
