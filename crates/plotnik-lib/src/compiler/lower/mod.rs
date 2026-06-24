mod context;
pub mod dead;
mod driver;
pub mod epsilon;
pub mod ir;
pub mod nav;
pub mod pack;
pub mod thompson;
mod verify;

#[cfg(test)]
mod ir_tests;

pub(crate) use driver::{LowerInput, lower_to_ir};
