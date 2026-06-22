pub mod regex_table {
    pub use plotnik_compiler_emit_regex::regex_table::*;
}

pub mod string_table {
    pub use plotnik_compiler_emit_strings::string_table::*;
}

pub mod type_table {
    pub use plotnik_compiler_emit_types::type_table::*;
}

pub use plotnik_compiler_emit_strings::EmitError;

#[path = "../../../plotnik-compiler/src/emit/instructions.rs"]
pub mod instructions;

pub use instructions::{emit_instructions, intern_predicate_strings, intern_regex_predicates};
