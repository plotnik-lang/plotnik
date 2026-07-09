//! Static template substitution for backend-owned source skeletons.

use super::sink::Sink;

/// Splice a column-zero template into `out`, substituting `@KEY@` values and
/// indenting every non-empty line. A surviving placeholder is always an
/// emitter bug, so it fails here instead of shipping malformed generated code.
pub(crate) fn splice(out: &mut String, indent: &str, template: &str, subs: &[(&str, &str)]) {
    let mut text = template.trim_matches('\n').to_string();
    for (key, value) in subs {
        text = text.replace(&format!("@{key}@"), value);
    }
    assert_no_placeholders(&text);

    let mut sink = Sink::<()>::new();
    for line in text.lines() {
        if line.is_empty() {
            sink.push("\n");
            continue;
        }
        sink.push(indent);
        sink.push(line);
        sink.push("\n");
    }
    out.push_str(sink.plain());
}

fn assert_no_placeholders(text: &str) {
    let bytes = text.as_bytes();
    let mut start = 0;
    while let Some(open) = bytes[start..].iter().position(|&byte| byte == b'@') {
        let open = start + open;
        let Some(close) = bytes[open + 1..].iter().position(|&byte| byte == b'@') else {
            return;
        };
        let close = open + 1 + close;
        let key = &text[open + 1..close];
        assert!(
            key.is_empty()
                || !key
                    .bytes()
                    .all(|byte| byte == b'_' || byte.is_ascii_uppercase()),
            "unsubstituted template placeholder `@{key}@`"
        );
        start = close + 1;
    }
}
