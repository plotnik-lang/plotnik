//! Error types for bytecode emission.

use crate::core::Symbol;

use crate::bytecode::EncodeError;

/// Error during bytecode emission.
#[derive(Clone, Debug, thiserror::Error)]
pub(in crate::compiler) enum EmitError {
    /// Query has validation errors (must be valid before emitting).
    #[error("query has validation errors")]
    InvalidQuery,
    /// Too many strings (exceeds u16 max).
    #[error("too many strings: {0} (max 65534)")]
    TooManyStrings(usize),
    /// Too many types (exceeds u16 max).
    #[error("too many types: {0} (max 65533)")]
    TooManyTypes(usize),
    /// Too many type members (exceeds u16 max).
    #[error("too many type members: {0} (max 65535)")]
    TooManyTypeMembers(usize),
    /// Struct has more fields than the format's u8 member count allows.
    #[error("too many struct fields: {0} (max 255)")]
    TooManyFields(usize),
    /// Enum has more variants than the format's u8 member count allows.
    #[error("too many enum variants: {0} (max 255)")]
    TooManyVariants(usize),
    /// Too many distinct node kinds (exceeds u16 max).
    #[error("too many node kinds: {0} (max 65535)")]
    TooManyNodeKinds(usize),
    /// Too many distinct node fields (exceeds u16 max).
    #[error("too many node fields: {0} (max 65535)")]
    TooManyNodeFields(usize),
    /// Too many entrypoints (exceeds u16 max).
    #[error("too many entrypoints: {0} (max 65535)")]
    TooManyEntrypoints(usize),
    /// Too many transitions (exceeds u16 max).
    #[error("too many transitions: {0} (max 65535)")]
    TooManyTransitions(usize),
    /// Too many regexes (exceeds u16 max).
    #[error("too many regexes: {0} (max 65535)")]
    TooManyRegexes(usize),
    /// String not found in interner.
    #[error("string not found for symbol: {0:?}")]
    StringNotFound(Symbol),
    /// Regex compilation failed.
    #[error("regex compile error for '{0}': {1}")]
    RegexCompile(String, String),
    /// An instruction could not be encoded (count or payload out of range).
    #[error("instruction encoding error: {0}")]
    Encode(#[from] EncodeError),
}
