//! Instruction packing: lower the symbolic IR into its final packed form.

mod lower;

pub use lower::pack_instructions;
