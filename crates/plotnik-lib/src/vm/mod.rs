//! Runtime VM for executing compiled Plotnik queries.
//!
//! This crate provides the virtual machine that executes bytecode against
//! tree-sitter syntax trees, producing structured output.

#![allow(clippy::comparison_chain)]

pub mod engine;

pub use engine::{
    EffectLog, ExecLimits, Materializer, NodeHandle, PrintTracer, RuntimeEffect, RuntimeError,
    Tracer, VM, Value, ValueMaterializer, Verbosity, debug_verify_type, materialize_verified,
};
