use indexmap::IndexMap;

use crate::analyze::type_check::DefId;
use crate::bytecode::{InstructionIR, Label};

#[derive(Clone, Debug)]
pub struct CompileResult {
    pub instructions: Vec<InstructionIR>,
    /// Entry labels for each definition (in definition order).
    pub def_entries: IndexMap<DefId, Label>,
    /// Entry label for the universal preamble.
    /// The preamble wraps any entrypoint: Struct -> Trampoline -> EndStruct -> Return
    pub preamble_entry: Label,
}
