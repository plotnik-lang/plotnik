#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod analyze {
    pub use plotnik_analyze::analyze::*;
}

pub mod bytecode {
    pub use plotnik_ir::*;
}

pub mod emit {
    #[path = "../../../plotnik-compiler/src/emit/emitter.rs"]
    mod emitter;
    #[path = "../../../plotnik-compiler/src/emit/error.rs"]
    mod error;
    #[path = "../../../plotnik-compiler/src/emit/layout.rs"]
    pub mod layout;
    #[path = "../../../plotnik-compiler/src/emit/regex_table.rs"]
    mod regex_table;
    #[path = "../../../plotnik-compiler/src/emit/string_table.rs"]
    mod string_table;
    #[path = "../../../plotnik-compiler/src/emit/type_table.rs"]
    mod type_table;

    pub use emitter::{EmitInput, emit, emit_unchecked};
    pub use error::EmitError;
    pub use regex_table::{RegexTableBuilder, deserialize_dfa};
    pub use string_table::StringTableBuilder;
    pub use type_table::TypeTableBuilder;
}

pub use emit::*;
