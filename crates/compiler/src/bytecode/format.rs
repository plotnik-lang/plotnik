use std::borrow::Cow;

use super::effects::EffectKind;
use plotnik_runtime::Nav;

pub mod cols {
    pub const INDENT: usize = 2;
    pub const GAP: usize = 1;
    pub const SYMBOL: usize = 5;
    pub const TOTAL_WIDTH: usize = 44;
    pub const EFFECTS: usize = 56;
}

#[derive(Clone, Debug)]
pub struct Symbol {
    pub left: &'static str,
    pub center: &'static str,
    pub right: Cow<'static, str>,
}

impl Default for Symbol {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl Symbol {
    pub const fn new(left: &'static str, center: &'static str, right: &'static str) -> Self {
        Self {
            left,
            center,
            right: Cow::Borrowed(right),
        }
    }

    pub fn with_right(left: &'static str, center: &'static str, right: String) -> Self {
        Self {
            left,
            center,
            right: Cow::Owned(right),
        }
    }

    pub const EMPTY: Symbol = Symbol::new("  ", " ", "  ");

    pub const EPSILON: Symbol = Symbol::new(" ", "-ε-", " ");

    pub const PADDING: Symbol = Symbol::new(" ", "...", " ");

    pub fn format(&self) -> String {
        format!("{}{}{}", self.left, self.center, self.right)
    }
}

pub fn nav_symbol(nav: Nav) -> Symbol {
    match nav {
        Nav::Epsilon => Symbol::EPSILON,
        Nav::Stay => Symbol::EMPTY,
        Nav::StayExact => Symbol::new("  ", "!", "  "),
        Nav::Down => Symbol::new(" └", "‣", "─ "),
        Nav::DownSkip => Symbol::new(" └", "•", "─ "),
        Nav::DownSkipExtras => Symbol::new(" └", "◦", "─ "),
        Nav::DownExact => Symbol::new(" └", "─", "! "),
        Nav::ChildlessSkipTrivia => Symbol::new(" └", "•", "┘ "),
        Nav::ChildlessSkipExtras => Symbol::new(" └", "◦", "┘ "),
        Nav::ChildlessExact => Symbol::new(" └", "!", "┘ "),
        Nav::Next => Symbol::new(" ─", "‣", "─ "),
        Nav::NextSkip => Symbol::new(" ─", "•", "─ "),
        Nav::NextSkipExtras => Symbol::new(" ─", "◦", "─ "),
        Nav::NextExact => Symbol::new(" ─", "─", "! "),
        Nav::Up(n) => up_symbol(" ─", "‣", n),
        Nav::UpSkipTrivia(n) => up_symbol(" ─", "•", n),
        Nav::UpSkipExtras(n) => up_symbol(" ─", "◦", n),
        Nav::UpExact(n) => up_symbol(" !", "─", n),
    }
}

pub mod trace {
    use super::Symbol;

    pub const MATCH_SUCCESS: Symbol = Symbol::new("  ", "●", "  ");
    pub const MATCH_FAILURE: Symbol = Symbol::new("  ", "○", "  ");

    pub const EFFECT: Symbol = Symbol::new("  ", "⬥", "  ");
    pub const EFFECT_SUPPRESSED: Symbol = Symbol::new("  ", "⬦", "  ");

    pub const CALL: Symbol = Symbol::new("  ", "▶", "  ");
    pub const RETURN: Symbol = Symbol::new("  ", "◀", "  ");

    pub const BACKTRACK: Symbol = Symbol::new(" ", "❮❮❮", " ");
}

const SUPERSCRIPT_DIGITS: &[char] = &['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];

pub fn superscript(n: u8) -> String {
    if n < 10 {
        SUPERSCRIPT_DIGITS[n as usize].to_string()
    } else {
        n.to_string()
            .chars()
            .map(|c| {
                SUPERSCRIPT_DIGITS[c
                    .to_digit(10)
                    .expect("char is an ASCII digit from integer formatting")
                    as usize]
            })
            .collect()
    }
}

fn up_symbol(left: &'static str, center: &'static str, n: u8) -> Symbol {
    if n == 1 {
        return Symbol::new(left, center, "┘ ");
    }

    Symbol::with_right(left, center, format!("┘{}", superscript(n)))
}

pub fn width_for_count(count: usize) -> usize {
    if count <= 1 {
        1
    } else {
        (count - 1).ilog10() as usize + 1
    }
}

pub fn truncate_text(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len - 1).collect();
        format!("{}…", truncated)
    }
}

pub struct LineBuilder {
    addr_width: usize,
}

impl LineBuilder {
    pub fn new(addr_width: usize) -> Self {
        Self { addr_width }
    }

    pub fn instruction(
        &self,
        addr: impl std::fmt::Display,
        symbol: Symbol,
        content: &str,
        successors: &str,
        effects: &[String],
    ) -> String {
        let prefix = self.instruction_prefix(addr, symbol);
        let mut line = self.pad_successors(format!("{prefix}{content}"), successors);
        if effects.is_empty() {
            line.truncate(line.trim_end().len());
            return line;
        }

        let padding = cols::EFFECTS.saturating_sub(display_width(&line)).max(2);
        line.push_str(&format!("{:padding$}[{}]", "", effects.join(" ")));
        line
    }

    pub fn instruction_prefix(&self, addr: impl std::fmt::Display, symbol: Symbol) -> String {
        format!(
            "{:indent$}{:0aw$} {} ",
            "",
            addr,
            symbol.format(),
            indent = cols::INDENT,
            aw = self.addr_width,
        )
    }

    pub fn switch_case(&self, kind: &str, target: &str) -> String {
        let indent = cols::INDENT + self.addr_width + 2 * cols::GAP + cols::SYMBOL + 2;
        self.pad_successors(format!("{:indent$}{kind}", ""), target)
    }

    pub fn pad_successors(&self, base: String, successors: &str) -> String {
        let padding = cols::TOTAL_WIDTH
            .saturating_sub(display_width(&base))
            .max(2);
        format!("{base}{:padding$}{successors}", "")
    }
}

fn display_width(s: &str) -> usize {
    let mut width = 0;
    let mut in_escape = false;

    for c in s.chars() {
        if in_escape {
            if c == 'm' {
                in_escape = false;
            }
        } else if c == '\x1b' {
            in_escape = true;
        } else {
            width += 1;
        }
    }

    width
}

pub(crate) fn pattern_constraints(mut pattern: String, constraints: Vec<String>) -> String {
    if constraints.is_empty() {
        return pattern;
    }

    let parenthesized = pattern.starts_with('(') && pattern.ends_with(')');
    if parenthesized {
        pattern.pop();
    }
    pattern.push(' ');
    pattern.push_str(&constraints.join(" "));
    if parenthesized {
        pattern.push(')');
    }
    pattern
}

pub(crate) fn return_content(port: plotnik_runtime::PortId) -> String {
    if port.to_byte() == 0 {
        return "return".to_string();
    }

    format!("return p{}", port.to_byte())
}

pub fn format_effect(kind: EffectKind, payload: impl std::fmt::Display) -> String {
    match kind {
        EffectKind::Node => "Node".to_string(),
        EffectKind::ListOpen => "ListOpen".to_string(),
        EffectKind::ArrayPush => "ArrayPush".to_string(),
        EffectKind::ListClose => "ListClose".to_string(),
        EffectKind::RecordOpen => "RecordOpen".to_string(),
        EffectKind::RecordClose => "RecordClose".to_string(),
        EffectKind::RecordSet => format!("RecordSet({payload})"),
        EffectKind::VariantOpen => format!("VariantOpen({payload})"),
        EffectKind::VariantClose => "VariantClose".to_string(),
        EffectKind::Absent => "Absent".to_string(),
        EffectKind::SuppressBegin => "SuppressBegin".to_string(),
        EffectKind::SuppressEnd => "SuppressEnd".to_string(),
        EffectKind::SpanStartAt => format!("SpanStartAt({payload})"),
        EffectKind::SpanStart => format!("SpanStart({payload})"),
        EffectKind::SpanEnd => format!("SpanEnd({payload})"),
        EffectKind::ScalarOpen => "ScalarOpen".to_string(),
        EffectKind::ScalarMark => "ScalarMark".to_string(),
        EffectKind::TextClose => "TextClose".to_string(),
        EffectKind::BoolClose => format!("BoolClose({payload})"),
        EffectKind::NodeText => "NodeText".to_string(),
        EffectKind::NodeBool => "NodeBool".to_string(),
        EffectKind::BoolValue => format!("BoolValue({payload})"),
    }
}
