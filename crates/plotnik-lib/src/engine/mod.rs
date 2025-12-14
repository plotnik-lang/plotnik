//! Query execution engine.

pub mod effect_stream;
pub mod error;
pub mod interpreter;
pub mod materializer;
pub mod value;

#[cfg(test)]
mod interpreter_tests;
