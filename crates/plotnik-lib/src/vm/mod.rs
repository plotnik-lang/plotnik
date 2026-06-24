//! Runtime VM for executing compiled Plotnik queries.
//!
//! This module provides the virtual machine that executes bytecode against
//! tree-sitter syntax trees, producing structured output.

#![allow(clippy::comparison_chain)]

mod engine;

pub use engine::{
    EffectLog, ExecLimits, NodeHandle, NoopTracer, PrintTracer, PrintTracerBuilder, RuntimeEffect,
    RuntimeError, Tracer, VM, VMBuilder, Value, ValueMaterializer, Verbosity, debug_verify_type,
    materialize_verified,
};
