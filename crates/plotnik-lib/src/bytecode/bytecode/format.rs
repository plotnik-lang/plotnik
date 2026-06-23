//! Shared formatting utilities for bytecode dump and execution trace.
//!
//! Both dump and trace use the same column layout:
//! ```text
//! | 2 | step | 1 |   5   | 1 | content              | 1 | succ |
//! |   | pad  |   | (sym) |   |                      |   |      |
//! ```

use std::borrow::Cow;

use super::Effect;
use super::effects::EffectKind;
use super::nav::Nav;

/// Display label for the bootstrap preamble at step 0. The preamble has no name
/// in the format; `dump` and `trace` share this so both render it identically.
pub const PREAMBLE_NAME: &str = "_StructWrap";

/// Column widths for instruction line formatting.
pub mod cols {
    /// Leading indentation (2 spaces).
    pub const INDENT: usize = 2;
    /// Gap between columns (1 space).
    pub const GAP: usize = 1;
    /// Symbol column width for fixed-width trace symbols.
    pub const SYMBOL: usize = 5;
    /// Total width before successors are right-aligned.
    pub const TOTAL_WIDTH: usize = 44;
}

/// Symbols for the instruction and trace columns.
///
/// Format: `| left | center | right |`. Dump symbols keep the connector
/// shape in these three parts; multi-digit up counts may extend `right`.
#[derive(Clone, Debug)]
pub struct Symbol {
    /// Left connector or padding.
    pub left: &'static str,
    /// Center connector, marker, or status.
    pub center: &'static str,
    /// Right connector, suffix, or padding.
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

    /// Empty symbol (5 spaces).
    pub const EMPTY: Symbol = Symbol::new("  ", " ", "  ");

    /// Epsilon symbol for unconditional transitions.
    pub const EPSILON: Symbol = Symbol::new(" ", "-ε-", " ");

    /// Padding indicator (centered "..." in 5-char column).
    pub const PADDING: Symbol = Symbol::new(" ", "...", " ");

    pub fn format(&self) -> String {
        format!("{}{}{}", self.left, self.center, self.right)
    }
}

/// Format navigation command as a dump symbol.
///
/// | Nav             | Symbol examples | Notes                                   |
/// | --------------- | --------------- | --------------------------------------- |
/// | Epsilon         | -ε-             | Pure control flow, no cursor check      |
/// | Stay            | (blank)         | No movement                             |
/// | StayExact       | !               | Stay at position, exact match only      |
/// | Down            | └‣─             | First child, skip any                   |
/// | DownSkip        | └•─             | First child, skip trivia                |
/// | DownSkipExtras  | └◦─             | First child, skip extras only           |
/// | DownExact       | └─!             | First child, exact                      |
/// | Next            | ─‣─             | Next sibling, skip any                  |
/// | NextSkip        | ─•─             | Next sibling, skip trivia               |
/// | NextSkipExtras  | ─◦─             | Next sibling, skip extras only          |
/// | NextExact       | ──!             | Next sibling, exact                     |
/// | Up(1)           | ─‣┘             | Ascend 1 level, skip any                |
/// | Up(2), Up(10)   | ─‣┘², ─‣┘¹⁰     | Ascend n levels, skip any               |
/// | UpSkipTrivia(n) | ─•┘², ─•┘¹⁰     | Ascend n, last non-trivia on each level |
/// | UpSkipExtras(n) | ─◦┘², ─◦┘¹⁰     | Ascend n, last non-extra on each level  |
/// | UpExact(n)      | !─┘², !─┘¹⁰     | Ascend n, last child on each level      |
pub fn nav_symbol(nav: Nav) -> Symbol {
    match nav {
        Nav::Epsilon => Symbol::EPSILON,
        Nav::Stay => Symbol::EMPTY,
        Nav::StayExact => Symbol::new("  ", "!", "  "),
        Nav::Down => Symbol::new(" └", "‣", "─ "),
        Nav::DownSkip => Symbol::new(" └", "•", "─ "),
        Nav::DownSkipExtras => Symbol::new(" └", "◦", "─ "),
        Nav::DownExact => Symbol::new(" └", "─", "! "),
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

/// Trace sub-line symbols.
pub mod trace {
    use super::Symbol;

    /// Match: success.
    pub const MATCH_SUCCESS: Symbol = Symbol::new("  ", "●", "  ");
    /// Match: failure.
    pub const MATCH_FAILURE: Symbol = Symbol::new("  ", "○", "  ");

    /// Effect: data capture or structure.
    pub const EFFECT: Symbol = Symbol::new("  ", "⬥", "  ");
    /// Effect: suppressed (inside @_ capture).
    pub const EFFECT_SUPPRESSED: Symbol = Symbol::new("  ", "⬦", "  ");

    /// Call: entering definition.
    pub const CALL: Symbol = Symbol::new("  ", "▶", "  ");
    /// Return: back from definition.
    pub const RETURN: Symbol = Symbol::new("  ", "◀", "  ");

    /// Backtrack symbol (centered in 5 chars).
    pub const BACKTRACK: Symbol = Symbol::new(" ", "❮❮❮", " ");
}

const SUPERSCRIPT_DIGITS: &[char] = &['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];

/// Convert a number to superscript digits.
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
        ((count - 1) as f64).log10().floor() as usize + 1
    }
}

/// Truncate text to max length with ellipsis.
///
/// Used for displaying node text in traces.
pub fn truncate_text(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len - 1).collect();
        format!("{}…", truncated)
    }
}

/// Builder for formatted output lines.
///
/// Constructs lines following the column layout:
/// `<indent><step><gap><symbol><gap><content>...<successors>`
pub struct LineBuilder {
    step_width: usize,
}

impl LineBuilder {
    /// Create a new line builder with the given step width.
    pub fn new(step_width: usize) -> Self {
        Self { step_width }
    }

    /// Build an instruction line prefix: `  <step> <symbol> `
    pub fn instruction_prefix(&self, step: u16, symbol: Symbol) -> String {
        format!(
            "{:indent$}{:0sw$} {} ",
            "",
            step,
            symbol.format(),
            indent = cols::INDENT,
            sw = self.step_width,
        )
    }

    /// Pad content to total width and append successors.
    ///
    /// Ensures at least 2 spaces between content and successors.
    pub fn pad_successors(&self, base: String, successors: &str) -> String {
        let padding = cols::TOTAL_WIDTH
            .saturating_sub(display_width(&base))
            .max(2);
        format!("{base}{:padding$}{successors}", "")
    }
}

/// Calculate display width of a string, ignoring ANSI escape sequences.
///
/// ANSI sequences have the form `\x1b[...m` and render as zero-width.
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

pub fn format_effect(effect: &Effect) -> String {
    match effect.kind {
        EffectKind::Node => "Node".to_string(),
        EffectKind::ArrayOpen => "ArrayOpen".to_string(),
        EffectKind::Push => "Push".to_string(),
        EffectKind::ArrayClose => "ArrayClose".to_string(),
        EffectKind::StructOpen => "StructOpen".to_string(),
        EffectKind::StructClose => "StructClose".to_string(),
        EffectKind::Set => format!("Set(M{})", effect.payload),
        EffectKind::EnumOpen => format!("EnumOpen(M{})", effect.payload),
        EffectKind::EnumClose => "EnumClose".to_string(),
        EffectKind::Null => "Null".to_string(),
        EffectKind::SuppressBegin => "SuppressBegin".to_string(),
        EffectKind::SuppressEnd => "SuppressEnd".to_string(),
    }
}
