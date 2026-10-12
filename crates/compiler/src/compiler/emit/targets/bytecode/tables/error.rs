//! Error types for bytecode emission.

use crate::bytecode::{EncodeError, MAX_SPANS};
use crate::compiler::analyze::result::ResultSchemaError;

#[derive(Clone, Debug, thiserror::Error)]
pub(in crate::compiler) enum EmitError {
    #[error("too many strings: {0} (max {max})", max = EmitError::MAX_STRINGS)]
    TooManyStrings(usize),
    #[error("too many types: {0} (max {max})", max = EmitError::MAX_TYPES)]
    TooManyTypes(usize),
    #[error("too many type members: {0} (max {max})", max = EmitError::MAX_TYPE_MEMBERS)]
    TooManyTypeMembers(usize),
    #[error("too many type names: {0} (max {max})", max = EmitError::MAX_TYPE_NAMES)]
    TooManyTypeNames(usize),
    #[error("too many record fields: {0} (max {max})", max = EmitError::MAX_FIELDS)]
    TooManyFields(usize),
    #[error("too many variant cases: {0} (max {max})", max = EmitError::MAX_CASES)]
    TooManyCases(usize),
    #[error("too many node kinds: {0} (max {max})", max = EmitError::MAX_NODE_KINDS)]
    TooManyNodeKinds(usize),
    #[error("too many node fields: {0} (max {max})", max = EmitError::MAX_NODE_FIELDS)]
    TooManyNodeFields(usize),
    #[error("too many entry points: {0} (max {max})", max = EmitError::MAX_ENTRY_POINTS)]
    TooManyEntryPoints(usize),
    #[error("too many instruction words: {0} (max {max})", max = EmitError::MAX_INSTRUCTION_WORDS)]
    TooManyInstructionWords(usize),
    #[error("too many regexes: {0} (max {max})", max = EmitError::MAX_REGEXES)]
    TooManyRegexes(usize),
    #[error("too many inspection spans: {0} (max {max})", max = EmitError::MAX_SPANS)]
    TooManySpans(usize),
    #[error("inspection source id {0} exceeds the bytecode maximum of {max}", max = u16::MAX)]
    SourceIdTooLarge(u32),
    #[error("{section} is too large: {size} bytes (max {max})", max = u32::MAX)]
    SectionTooLarge { section: &'static str, size: usize },
    #[error("bytecode module is too large: {0} bytes (max {max})", max = u32::MAX)]
    ModuleTooLarge(usize),
    #[error("regex compile error for '{0}': {1}")]
    RegexCompile(String, String),
    #[error("instruction encoding error: {0}")]
    Encode(#[from] EncodeError),
}

impl From<ResultSchemaError> for EmitError {
    fn from(error: ResultSchemaError) -> Self {
        match error {
            ResultSchemaError::Members(count) => Self::TooManyTypeMembers(count),
        }
    }
}

impl EmitError {
    pub(in crate::compiler) const MAX_STRINGS: usize = 65_534;
    pub(in crate::compiler) const MAX_TYPES: usize = u16::MAX as usize;
    pub(in crate::compiler) const MAX_TYPE_MEMBERS: usize = u16::MAX as usize;
    pub(in crate::compiler) const MAX_TYPE_NAMES: usize = u16::MAX as usize;
    pub(in crate::compiler) const MAX_FIELDS: usize = u8::MAX as usize;
    pub(in crate::compiler) const MAX_CASES: usize = u8::MAX as usize;
    pub(in crate::compiler) const MAX_NODE_KINDS: usize = u16::MAX as usize;
    pub(in crate::compiler) const MAX_NODE_FIELDS: usize = u16::MAX as usize;
    pub(in crate::compiler) const MAX_ENTRY_POINTS: usize = u16::MAX as usize;
    pub(in crate::compiler) const MAX_INSTRUCTION_WORDS: usize = u16::MAX as usize;
    pub(in crate::compiler) const MAX_REGEXES: usize = u16::MAX as usize;
    pub(in crate::compiler) const MAX_SPANS: usize = MAX_SPANS;
}
