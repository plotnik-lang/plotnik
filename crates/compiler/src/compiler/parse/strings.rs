//! Escape-sequence decoding for string literals.
//!
//! The lexer accepts any `\<char>` pair inside a string (see `StringLiteral`
//! in `cst.rs`); this module gives those pairs meaning. Decoding is lenient —
//! an unrecognized escape passes through verbatim and is reported as an issue,
//! so callers that run before validation still get a usable string, while the
//! validation pass turns the issues into error diagnostics.

use std::borrow::Cow;
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscapeIssueKind {
    /// `\q` and friends: the character after the backslash isn't an escape.
    Unknown,
    /// `\u` not followed by `{1-6 hex digits}` naming a Unicode scalar value.
    InvalidUnicode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EscapeIssue {
    /// Byte range of the offending escape, relative to the string content start.
    pub range: Range<usize>,
    pub kind: EscapeIssueKind,
}

/// Decode the escape sequences in a `StringContent` token's text.
///
/// Supported: `\n` `\r` `\t` `\\` `\"` `\'` and `\u{…}` with 1-6 hex digits.
/// Anything else after a backslash is kept verbatim and reported.
pub fn unescape(raw: &str) -> (Cow<'_, str>, Vec<EscapeIssue>) {
    if !raw.contains('\\') {
        return (Cow::Borrowed(raw), Vec::new());
    }

    let mut out = String::with_capacity(raw.len());
    let mut issues = Vec::new();
    let mut i = 0;

    while i < raw.len() {
        let c = raw[i..]
            .chars()
            .next()
            .expect("index sits on a char boundary");
        if c != '\\' {
            out.push(c);
            i += c.len_utf8();
            continue;
        }

        let esc_start = i;
        i += 1;
        let Some(esc) = raw[i..].chars().next() else {
            // Lone trailing backslash: unreachable for terminated strings, but
            // the function stays total.
            out.push('\\');
            issues.push(EscapeIssue {
                range: esc_start..i,
                kind: EscapeIssueKind::Unknown,
            });
            break;
        };
        i += esc.len_utf8();

        match esc {
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            '\\' => out.push('\\'),
            '"' => out.push('"'),
            '\'' => out.push('\''),
            'u' => match parse_unicode(&raw[i..]) {
                Ok((ch, consumed)) => {
                    out.push(ch);
                    i += consumed;
                }
                Err(consumed) => {
                    i += consumed;
                    out.push_str(&raw[esc_start..i]);
                    issues.push(EscapeIssue {
                        range: esc_start..i,
                        kind: EscapeIssueKind::InvalidUnicode,
                    });
                }
            },
            other => {
                out.push('\\');
                out.push(other);
                issues.push(EscapeIssue {
                    range: esc_start..i,
                    kind: EscapeIssueKind::Unknown,
                });
            }
        }
    }

    (Cow::Owned(out), issues)
}

/// Parse the `{1-6 hex}` payload after `\u`. `Ok` carries the decoded char and
/// the bytes consumed (braces included); `Err` carries the bytes to blame.
fn parse_unicode(rest: &str) -> Result<(char, usize), usize> {
    if !rest.starts_with('{') {
        return Err(0);
    }
    let Some(close) = rest.find('}') else {
        return Err(rest.len());
    };
    let digits = &rest[1..close];
    let consumed = close + 1;
    if digits.is_empty() || digits.len() > 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(consumed);
    }
    let value = u32::from_str_radix(digits, 16).expect("validated 1-6 hex digits");
    match char::from_u32(value) {
        Some(ch) => Ok((ch, consumed)),
        None => Err(consumed),
    }
}
