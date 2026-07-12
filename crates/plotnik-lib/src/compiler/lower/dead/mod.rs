//! Dead-code elimination: remove unreachable steps from the compiled IR.

mod dead_code;

pub use dead_code::remove_unreachable;
