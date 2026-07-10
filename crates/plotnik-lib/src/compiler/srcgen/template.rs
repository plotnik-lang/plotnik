//! Static template substitution for backend-owned source skeletons.

use super::sink::Sink;

/// Splice a column-zero template into `out`, substituting `@KEY@` values and
/// indenting every non-empty line. A template placeholder without a supplied
/// value is always an emitter bug, so it fails here instead of shipping
/// malformed generated code.
pub(crate) fn splice(out: &mut String, indent: &str, template: &str, subs: &[(&str, &str)]) {
    let text = substitute(template.trim_matches('\n'), subs);

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

fn substitute(template: &str, subs: &[(&str, &str)]) -> String {
    let mut text = String::with_capacity(template.len());
    let mut rest = template;

    loop {
        let Some(open) = rest.find('@') else {
            text.push_str(rest);
            break;
        };
        text.push_str(&rest[..open]);

        let after_open = &rest[open + 1..];
        let Some(close) = after_open.find('@') else {
            text.push_str(&rest[open..]);
            break;
        };
        let key = &after_open[..close];
        if is_placeholder(key) {
            let Some((_, value)) = subs.iter().find(|(candidate, _)| *candidate == key) else {
                panic!("unsubstituted template placeholder `@{key}@`");
            };
            text.push_str(value);
        } else {
            text.push_str(&rest[open..open + close + 2]);
        }
        rest = &after_open[close + 1..];
    }

    text
}

fn is_placeholder(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|byte| byte == b'_' || byte.is_ascii_uppercase())
}
