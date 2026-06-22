#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod diagnostics {
    pub use plotnik_compiler_diagnostics::diagnostics::*;
}

pub mod parser {
    pub use plotnik_compiler_parse::parser::*;
}

pub mod source {
    pub use plotnik_compiler_diagnostics::source::*;
}

pub use plotnik_compiler_diagnostics::{Diagnostics, SourceId};

pub mod analyze;
pub use analyze::*;
