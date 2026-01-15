//! Runtime engine for executing compiled Plotnik queries.
//!
//! The VM executes bytecode against tree-sitter syntax trees,
//! producing an effect log that can be materialized into output values.

mod checkpoint;
mod cursor;
mod effect;
mod error;
mod frame;
mod materializer;
mod trace;
mod value;
mod verify;
mod vm;

#[cfg(test)]
mod engine_tests;
#[cfg(test)]
mod verify_tests;

pub use effect::{EffectLog, RuntimeEffect};
pub use error::RuntimeError;
pub use materializer::{Materializer, ValueMaterializer};
pub use trace::{PrintTracer, Tracer, Verbosity};
pub use value::{NodeHandle, Value};
pub use verify::debug_verify_type;
pub use vm::{FuelLimits, VM};
