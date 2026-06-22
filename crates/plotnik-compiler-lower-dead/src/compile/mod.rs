pub mod error {
    pub use plotnik_compiler_lower_thompson::CompileResult;
}

#[path = "../../../plotnik-compiler/src/compile/dce.rs"]
pub mod dce;

pub use dce::remove_unreachable;
pub use error::CompileResult;
