pub mod error {
    pub use plotnik_compiler_lower_thompson::CompileResult;
}

pub use error::CompileResult;

#[path = "../../../plotnik-compiler/src/compile/lower.rs"]
pub mod lower;

pub use lower::lower;
