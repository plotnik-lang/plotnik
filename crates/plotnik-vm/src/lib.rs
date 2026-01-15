//! Runtime VM for executing compiled Plotnik queries.
//!
//! This crate provides the virtual machine that executes bytecode against
//! tree-sitter syntax trees, producing structured output.

#![allow(clippy::comparison_chain)]

pub mod engine;

// Re-export commonly used items at crate root
pub use engine::{
    EffectLog, FuelLimits, Materializer, NodeHandle, PrintTracer, RuntimeEffect, RuntimeError,
    Tracer, VM, Value, ValueMaterializer, Verbosity, debug_verify_type,
};
