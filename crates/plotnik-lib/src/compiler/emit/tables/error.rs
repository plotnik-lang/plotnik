//! Error types for bytecode emission.

use crate::core::Symbol;

use crate::bytecode::{EncodeError, MAX_SPANS};
use crate::compiler::analyze::output::OutputSchemaError;

/// Error during bytecode emission.
#[derive(Clone, Debug, thiserror::Error)]
pub(in crate::compiler) enum EmitError {
    /// Query has validation errors (must be valid before emitting).
    #[error("query has validation errors")]
    InvalidQuery,
    /// Too many strings (exceeds u16 max).
    #[error("too many strings: {0} (max {max})", max = EmitError::MAX_STRINGS)]
    TooManyStrings(usize),
    /// Too many types (exceeds u16 max).
    #[error("too many types: {0} (max {max})", max = EmitError::MAX_TYPES)]
    TooManyTypes(usize),
    /// Too many type members (exceeds u16 max).
    #[error("too many type members: {0} (max {max})", max = EmitError::MAX_TYPE_MEMBERS)]
    TooManyTypeMembers(usize),
    /// Too many type names (exceeds u16 max).
    #[error("too many type names: {0} (max {max})", max = EmitError::MAX_TYPE_NAMES)]
    TooManyTypeNames(usize),
    /// Struct has more fields than the format's u8 member count allows.
    #[error("too many struct fields: {0} (max {max})", max = EmitError::MAX_FIELDS)]
    TooManyFields(usize),
    /// Enum has more variants than the format's u8 member count allows.
    #[error("too many enum variants: {0} (max {max})", max = EmitError::MAX_VARIANTS)]
    TooManyVariants(usize),
    /// Too many distinct node kinds (exceeds u16 max).
    #[error("too many node kinds: {0} (max {max})", max = EmitError::MAX_NODE_KINDS)]
    TooManyNodeKinds(usize),
    /// Too many distinct node fields (exceeds u16 max).
    #[error("too many node fields: {0} (max {max})", max = EmitError::MAX_NODE_FIELDS)]
    TooManyNodeFields(usize),
    /// Too many entrypoints (exceeds u16 max).
    #[error("too many entrypoints: {0} (max {max})", max = EmitError::MAX_ENTRYPOINTS)]
    TooManyEntrypoints(usize),
    /// Too many transitions (exceeds u16 max).
    #[error("too many transitions: {0} (max {max})", max = EmitError::MAX_TRANSITIONS)]
    TooManyTransitions(usize),
    /// Too many regexes (exceeds u16 max).
    #[error("too many regexes: {0} (max {max})", max = EmitError::MAX_REGEXES)]
    TooManyRegexes(usize),
    /// Too many inspection spans (exceeds the span-id payload budget).
    #[error("too many inspection spans: {0} (max {max})", max = EmitError::MAX_SPANS)]
    TooManySpans(usize),
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

impl From<OutputSchemaError> for EmitError {
    fn from(error: OutputSchemaError) -> Self {
        match error {
            OutputSchemaError::Members(count) => Self::TooManyTypeMembers(count),
            OutputSchemaError::Fields(count) => Self::TooManyFields(count),
            OutputSchemaError::Variants(count) => Self::TooManyVariants(count),
        }
    }
}

impl EmitError {
    pub(in crate::compiler) const MAX_STRINGS: usize = 65_534;
    pub(in crate::compiler) const MAX_TYPES: usize = u16::MAX as usize;
    pub(in crate::compiler) const MAX_TYPE_MEMBERS: usize = u16::MAX as usize;
    pub(in crate::compiler) const MAX_TYPE_NAMES: usize = u16::MAX as usize;
    pub(in crate::compiler) const MAX_FIELDS: usize = u8::MAX as usize;
    pub(in crate::compiler) const MAX_VARIANTS: usize = u8::MAX as usize;
    pub(in crate::compiler) const MAX_NODE_KINDS: usize = u16::MAX as usize;
    pub(in crate::compiler) const MAX_NODE_FIELDS: usize = u16::MAX as usize;
    pub(in crate::compiler) const MAX_ENTRYPOINTS: usize = u16::MAX as usize;
    pub(in crate::compiler) const MAX_TRANSITIONS: usize = u16::MAX as usize;
    pub(in crate::compiler) const MAX_REGEXES: usize = u16::MAX as usize;
    pub(in crate::compiler) const MAX_SPANS: usize = MAX_SPANS;
}
