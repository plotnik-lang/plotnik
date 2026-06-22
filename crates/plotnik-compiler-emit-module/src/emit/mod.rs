pub mod instructions {
    pub use plotnik_compiler_emit_instructions::instructions::*;
}

pub mod layout {
    pub use plotnik_compiler_emit_layout::layout::*;
}

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

#[path = "../../../plotnik-compiler/src/emit/emitter.rs"]
mod emitter;

pub use emitter::{EmitInput, emit, emit_unchecked};
