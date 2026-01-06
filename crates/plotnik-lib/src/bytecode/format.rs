//! Shared formatting utilities for bytecode dump and execution trace.
//!
//! Both dump and trace use the same column layout:
//! ```text
//! | 2 | step | 1 |   5   | 1 | content              | 1 | succ |
//! |   | pad  |   | (sym) |   |                      |   |      |
//! ```

use super::EffectOp;
use super::effects::EffectOpcode;
use super::nav::Nav;

/// Column widths for instruction line formatting.
pub mod cols {
    /// Leading indentation (2 spaces).
    pub const INDENT: usize = 2;
    /// Gap between columns (1 space).
    pub const GAP: usize = 1;
    /// Symbol column width (5 chars: 2 left + 1 center + 2 right).
    pub const SYMBOL: usize = 5;
    /// Total width before successors are right-aligned.
    pub const TOTAL_WIDTH: usize = 44;
}

/// Symbols for the 5-character symbol column.
///
/// Format: `| left(2) | center(1) | right(2) |`
///
/// Used in both dump (nav symbols) and trace (nav, match, effect symbols).
#[derive(Clone, Copy, Debug)]
pub struct Symbol {
    /// Left modifier (2 chars): mode indicator or spaces.
    /// Examples: "  ", " !", "!!"
    pub left: &'static str,
    /// Center symbol (1 char): direction or status.
    /// Examples: "ε", "▽", "▷", "△", "●", "○", "⬥", "▶", "◀"
    pub center: &'static str,
    /// Right suffix (2 chars): level or spaces.
    /// Examples: "  ", "¹ ", "¹²"
    pub right: &'static str,
}

impl Default for Symbol {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl Symbol {
    /// Create a new symbol with all parts.
    pub const fn new(left: &'static str, center: &'static str, right: &'static str) -> Self {
        Self {
            left,
            center,
            right,
        }
    }

    /// Empty symbol (5 spaces).
    pub const EMPTY: Symbol = Symbol::new("  ", " ", "  ");

    /// Epsilon symbol for unconditional transitions.
    pub const EPSILON: Symbol = Symbol::new("  ", "ε", "  ");

    /// Format as a 5-character string.
    pub fn format(&self) -> String {
        format!("{}{}{}", self.left, self.center, self.right)
    }
}

/// Format navigation command as a Symbol using the doc-specified triangles.
///
/// | Nav             | Symbol  | Notes                               |
/// | --------------- | ------- | ----------------------------------- |
/// | Stay            | (blank) | No movement, 5 spaces               |
/// | Stay (epsilon)  | ε       | Only when no type/field constraints |
/// | StayExact       | !       | Stay at position, exact match only  |
/// | Down            | ▽       | First child, skip any               |
/// | DownSkip        | !▽      | First child, skip trivia            |
/// | DownExact       | !!▽     | First child, exact                  |
/// | Next            | ▷       | Next sibling, skip any              |
/// | NextSkip        | !▷      | Next sibling, skip trivia           |
/// | NextExact       | !!▷     | Next sibling, exact                 |
/// | Up(n)           | △ⁿ      | Ascend n levels, skip any           |
/// | UpSkipTrivia(n) | !△ⁿ     | Ascend n, must be last non-trivia   |
/// | UpExact(n)      | !!△ⁿ    | Ascend n, must be last child        |
pub fn nav_symbol(nav: Nav) -> Symbol {
    match nav {
        Nav::Stay => Symbol::EMPTY,
        Nav::StayExact => Symbol::new("  ", "!", "  "),
        Nav::Down => Symbol::new("  ", "▽", "  "),
        Nav::DownSkip => Symbol::new(" !", "▽", "  "),
        Nav::DownExact => Symbol::new("!!", "▽", "  "),
        Nav::Next => Symbol::new("  ", "▷", "  "),
        Nav::NextSkip => Symbol::new(" !", "▷", "  "),
        Nav::NextExact => Symbol::new("!!", "▷", "  "),
        Nav::Up(n) => Symbol::new("  ", "△", superscript_suffix(n)),
        Nav::UpSkipTrivia(n) => Symbol::new(" !", "△", superscript_suffix(n)),
        Nav::UpExact(n) => Symbol::new("!!", "△", superscript_suffix(n)),
    }
}

/// Format navigation for epsilon transitions (when is_epsilon is true).
///
/// True epsilon transitions require all three conditions:
/// - `nav == Stay` (no cursor movement)
/// - `node_type == None` (no type constraint)
/// - `node_field == None` (no field constraint)
pub fn nav_symbol_epsilon(nav: Nav, is_epsilon: bool) -> Symbol {
    if is_epsilon {
        Symbol::EPSILON
    } else {
        nav_symbol(nav)
    }
}

/// Trace sub-line symbols.
pub mod trace {
    use super::Symbol;

    /// Navigation: descended to child.
    pub const NAV_DOWN: Symbol = Symbol::new("  ", "▽", "  ");
    /// Navigation: moved to sibling.
    pub const NAV_NEXT: Symbol = Symbol::new("  ", "▷", "  ");
    /// Navigation: ascended to parent.
    pub const NAV_UP: Symbol = Symbol::new("  ", "△", "  ");

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
            .map(|c| SUPERSCRIPT_DIGITS[c.to_digit(10).unwrap() as usize])
            .collect()
    }
}

/// Convert a number to a 2-char superscript suffix for the symbol right column.
/// Level 1 shows no superscript (blank), levels 2+ show superscript.
fn superscript_suffix(n: u8) -> &'static str {
    match n {
        1 => "  ",
        2 => "² ",
        3 => "³ ",
        4 => "⁴ ",
        5 => "⁵ ",
        6 => "⁶ ",
        7 => "⁷ ",
        8 => "⁸ ",
        9 => "⁹ ",
        // For 10+, we'd need dynamic allocation. Rare in practice.
        _ => "ⁿ ",
    }
}

/// Calculate minimum width needed to display numbers up to `count - 1`.
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

    /// Build a sub-line prefix (blank step area): `       <symbol> `
    pub fn subline_prefix(&self, symbol: Symbol) -> String {
        let step_area = cols::INDENT + self.step_width + cols::GAP;
        format!("{:step_area$}{} ", "", symbol.format())
    }

    /// Build a backtrack line: `  <step>  ❮❮❮`
    pub fn backtrack_line(&self, step: u16) -> String {
        format!(
            "{:indent$}{:0sw$} {}",
            "",
            step,
            trace::BACKTRACK.format(),
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

/// Format an effect operation for display.
pub fn format_effect(effect: &EffectOp) -> String {
    match effect.opcode {
        EffectOpcode::Node => "Node".to_string(),
        EffectOpcode::Arr => "Arr".to_string(),
        EffectOpcode::Push => "Push".to_string(),
        EffectOpcode::EndArr => "EndArr".to_string(),
        EffectOpcode::Obj => "Obj".to_string(),
        EffectOpcode::EndObj => "EndObj".to_string(),
        EffectOpcode::Set => format!("Set(M{})", effect.payload),
        EffectOpcode::Enum => format!("Enum(M{})", effect.payload),
        EffectOpcode::EndEnum => "EndEnum".to_string(),
        EffectOpcode::Text => "Text".to_string(),
        EffectOpcode::Clear => "Clear".to_string(),
        EffectOpcode::Null => "Null".to_string(),
        EffectOpcode::SuppressBegin => "SuppressBegin".to_string(),
        EffectOpcode::SuppressEnd => "SuppressEnd".to_string(),
    }
}
