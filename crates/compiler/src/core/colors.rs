/// ANSI color palette for CLI output.
///
/// Designed for jq-inspired colorization that works in both light and dark themes.
/// Uses only standard 16-color ANSI codes (no RGB).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Colors {
    pub blue: &'static str,
    pub green: &'static str,
    pub dim: &'static str,
    pub reset: &'static str,
}

impl Default for Colors {
    fn default() -> Self {
        Self::OFF
    }
}

impl Colors {
    /// Colors enabled (ANSI escape codes).
    pub const ON: Self = Self {
        blue: "\x1b[34m",
        green: "\x1b[32m",
        dim: "\x1b[2m",
        reset: "\x1b[0m",
    };

    /// Colors disabled (empty strings).
    pub const OFF: Self = Self {
        blue: "",
        green: "",
        dim: "",
        reset: "",
    };

    pub fn new(enabled: bool) -> Self {
        if enabled { Self::ON } else { Self::OFF }
    }
}
