pub mod error {
    pub use plotnik_compiler_lower_thompson::CompileResult;
}

#[path = "../../../plotnik-compiler/src/compile/epsilon_elim.rs"]
pub mod epsilon_elim;

pub use epsilon_elim::eliminate_epsilons;
pub use error::CompileResult;
