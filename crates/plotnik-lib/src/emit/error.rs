//! Error types for bytecode emission.

use plotnik_core::Symbol;

/// Error during bytecode emission.
#[derive(Clone, Debug)]
pub enum EmitError {
    /// Query has validation errors (must be valid before emitting).
    InvalidQuery,
    /// Too many strings (exceeds u16 max).
    TooManyStrings(usize),
    /// Too many types (exceeds u16 max).
    TooManyTypes(usize),
    /// Too many type members (exceeds u16 max).
    TooManyTypeMembers(usize),
    /// Too many entrypoints (exceeds u16 max).
    TooManyEntrypoints(usize),
    /// Too many transitions (exceeds u16 max).
    TooManyTransitions(usize),
    /// String not found in interner.
    StringNotFound(Symbol),
    /// Compilation error.
    Compile(crate::compile::CompileError),
}

impl std::fmt::Display for EmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidQuery => write!(f, "query has validation errors"),
            Self::TooManyStrings(n) => write!(f, "too many strings: {n} (max 65534)"),
            Self::TooManyTypes(n) => write!(f, "too many types: {n} (max 65533)"),
            Self::TooManyTypeMembers(n) => write!(f, "too many type members: {n} (max 65535)"),
            Self::TooManyEntrypoints(n) => write!(f, "too many entrypoints: {n} (max 65535)"),
            Self::TooManyTransitions(n) => write!(f, "too many transitions: {n} (max 65535)"),
            Self::StringNotFound(sym) => write!(f, "string not found for symbol: {sym:?}"),
            Self::Compile(e) => write!(f, "compilation error: {e}"),
        }
    }
}

impl std::error::Error for EmitError {}
