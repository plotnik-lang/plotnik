use indexmap::IndexMap;

use crate::analyze::type_check::DefId;
use crate::bytecode::{InstructionIR, Label};

#[derive(Clone, Debug)]
pub enum CompileError {
    DefinitionNotFound(String),
    MissingBody(String),
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DefinitionNotFound(name) => write!(f, "definition not found: {name}"),
            Self::MissingBody(name) => write!(f, "missing body for definition: {name}"),
        }
    }
}

impl std::error::Error for CompileError {}

#[derive(Clone, Debug)]
pub struct CompileResult {
    pub instructions: Vec<InstructionIR>,
    /// Entry labels for each definition (in definition order).
    pub def_entries: IndexMap<DefId, Label>,
    /// Entry label for the universal preamble.
    /// The preamble wraps any entrypoint: Struct -> Trampoline -> EndStruct -> Return
    pub preamble_entry: Label,
}
