use super::super::rules::Rule;
use super::TokenText;

pub(super) fn synthesize(rule: &Rule) -> TokenText {
    match strip(rule) {
        Rule::String(value) => TokenText::Str(value.clone()),
        other => TokenText::Regex(synth(other).text),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Prec {
    Alt,
    Concat,
    Atom,
}

struct Piece {
    text: String,
    prec: Prec,
}

impl Piece {
    fn atom(&self) -> String {
        if self.prec == Prec::Atom {
            self.text.clone()
        } else {
            format!("({})", self.text)
        }
    }
}

fn strip(rule: &Rule) -> &Rule {
    match rule {
        Rule::Metadata { rule, .. } | Rule::Reserved { rule, .. } => strip(rule),
        Rule::Seq(members) | Rule::Choice(members) if members.len() == 1 => strip(&members[0]),
        other => other,
    }
}

fn synth(rule: &Rule) -> Piece {
    match strip(rule) {
        Rule::Blank => Piece {
            text: String::new(),
            prec: Prec::Concat,
        },
        Rule::String(value) => Piece {
            text: escape_literal(value),
            prec: literal_prec(value),
        },
        Rule::Pattern(value, flags) => synth_pattern(value, flags),
        Rule::Repeat(inner) => Piece {
            text: format!("{}+", synth(inner).atom()),
            prec: Prec::Atom,
        },
        Rule::Seq(members) => synth_seq(members),
        Rule::Choice(members) => synth_choice(members),
        _ => Piece {
            text: String::new(),
            prec: Prec::Concat,
        },
    }
}

fn synth_pattern(value: &str, flags: &str) -> Piece {
    let body = escape_slashes(value);
    if flags.contains('i') {
        Piece {
            text: format!("(?i:{body})"),
            prec: Prec::Atom,
        }
    } else {
        Piece {
            text: body,
            prec: pattern_prec(value),
        }
    }
}

fn synth_seq(members: &[Rule]) -> Piece {
    let mut parts = Vec::new();
    for member in members {
        if matches!(strip(member), Rule::Blank) {
            continue;
        }
        let piece = synth(member);
        if piece.prec == Prec::Alt {
            parts.push(format!("({})", piece.text));
        } else {
            parts.push(piece.text);
        }
    }
    match parts.len() {
        0 => Piece {
            text: String::new(),
            prec: Prec::Concat,
        },
        1 => Piece {
            text: parts.pop().expect("one part"),
            prec: Prec::Concat,
        },
        _ => Piece {
            text: parts.concat(),
            prec: Prec::Concat,
        },
    }
}

fn synth_choice(members: &[Rule]) -> Piece {
    let has_blank = members.iter().any(|m| matches!(strip(m), Rule::Blank));
    let non_blank: Vec<&Rule> = members
        .iter()
        .map(strip)
        .filter(|m| !matches!(m, Rule::Blank))
        .collect();

    if has_blank && let [Rule::Repeat(inner)] = non_blank.as_slice() {
        return Piece {
            text: format!("{}*", synth(inner).atom()),
            prec: Prec::Atom,
        };
    }

    let alternation = join_alternatives(&non_blank);

    if has_blank {
        Piece {
            text: format!("{}?", alternation.atom()),
            prec: Prec::Atom,
        }
    } else {
        alternation
    }
}

fn join_alternatives(members: &[&Rule]) -> Piece {
    match members {
        [] => Piece {
            text: String::new(),
            prec: Prec::Concat,
        },
        [only] => synth(only),
        _ => {
            let text = members
                .iter()
                .map(|m| synth(m).text)
                .collect::<Vec<_>>()
                .join("|");
            Piece {
                text,
                prec: Prec::Alt,
            }
        }
    }
}

fn literal_prec(value: &str) -> Prec {
    if value.chars().count() == 1 {
        Prec::Atom
    } else {
        Prec::Concat
    }
}

fn escape_literal(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' | '.' | '^' | '$' | '|' | '?' | '*' | '+' | '(' | ')' | '[' | ']' | '{' | '}'
            | '/' => {
                out.push('\\');
                out.push(ch);
            }
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

fn escape_slashes(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len());
    let mut pending_backslashes = 0usize;
    for ch in pattern.chars() {
        if ch == '/' && pending_backslashes.is_multiple_of(2) {
            out.push('\\');
        }
        out.push(ch);
        if ch == '\\' {
            pending_backslashes += 1;
        } else {
            pending_backslashes = 0;
        }
    }
    out
}

fn pattern_prec(pattern: &str) -> Prec {
    let mut depth = 0i32;
    let mut in_class = false;
    let mut escaped = false;
    for ch in pattern.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '[' => in_class = true,
            ']' => in_class = false,
            '(' if !in_class => depth += 1,
            ')' if !in_class => depth -= 1,
            '|' if !in_class && depth == 0 => return Prec::Alt,
            _ => {}
        }
    }
    if is_single_atom(pattern) {
        Prec::Atom
    } else {
        Prec::Concat
    }
}

fn is_single_atom(pattern: &str) -> bool {
    let chars: Vec<char> = pattern.chars().collect();
    match chars.as_slice() {
        [] => false,
        [_] => true,
        ['\\', _] => true,
        ['[', .., ']'] => balanced_until_end(&chars, '[', ']'),
        ['(', .., ')'] => balanced_until_end(&chars, '(', ')'),
        _ => false,
    }
}

fn balanced_until_end(chars: &[char], open: char, close: char) -> bool {
    let mut depth = 0i32;
    let mut escaped = false;
    for (i, &ch) in chars.iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            c if c == open => depth += 1,
            c if c == close => {
                depth -= 1;
                if depth == 0 {
                    return i == chars.len() - 1;
                }
            }
            _ => {}
        }
    }
    false
}
