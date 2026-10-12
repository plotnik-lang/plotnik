//! Dead-code elimination: remove unreachable states from the matcher NFA.

mod dead_code;

pub use dead_code::remove_unreachable;
