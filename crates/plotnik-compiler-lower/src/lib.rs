#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod analyze {
    pub use plotnik_compiler_analyze::analyze::*;
}

pub mod bytecode {
    pub use plotnik_compiler_ir::*;
}

pub mod parser {
    pub use plotnik_compiler_parse::parser::*;
}

pub mod compile {
    #[path = "../../../plotnik-compiler/src/compile/capture.rs"]
    mod capture;
    #[path = "../../../plotnik-compiler/src/compile/collapse_up.rs"]
    mod collapse_up;
    #[path = "../../../plotnik-compiler/src/compile/compiler.rs"]
    mod compiler;
    #[path = "../../../plotnik-compiler/src/compile/dce.rs"]
    mod dce;
    #[path = "../../../plotnik-compiler/src/compile/epsilon_elim.rs"]
    pub(crate) mod epsilon_elim;
    #[path = "../../../plotnik-compiler/src/compile/error.rs"]
    mod error;
    #[path = "../../../plotnik-compiler/src/compile/expressions.rs"]
    mod expressions;
    #[path = "../../../plotnik-compiler/src/compile/lower.rs"]
    mod lower;
    #[path = "../../../plotnik-compiler/src/compile/navigation.rs"]
    mod navigation;
    #[path = "../../../plotnik-compiler/src/compile/quantifier.rs"]
    mod quantifier;
    #[path = "../../../plotnik-compiler/src/compile/scope.rs"]
    mod scope;
    #[path = "../../../plotnik-compiler/src/compile/sequences.rs"]
    mod sequences;
    #[path = "../../../plotnik-compiler/src/compile/verify.rs"]
    mod verify;

    pub use capture::CaptureEffects;
    pub use compiler::{CompileCtx, Compiler};
    pub use error::CompileResult;
}

pub use compile::*;
