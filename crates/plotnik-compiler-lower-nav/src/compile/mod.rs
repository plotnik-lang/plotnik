pub mod error {
    pub use plotnik_compiler_lower_thompson::CompileResult;
}

pub use error::CompileResult;

#[path = "../../../plotnik-compiler/src/compile/collapse_up.rs"]
pub mod collapse_up;

pub use collapse_up::collapse_up;
