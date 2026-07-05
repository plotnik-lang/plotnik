//! Runtime VM for executing compiled Plotnik queries.
//!
//! This module provides the virtual machine that executes bytecode against
//! tree-sitter syntax trees, producing structured output.

#![allow(clippy::comparison_chain)]

mod engine;

pub use engine::{
    Binding, EffectLog, Inspection, InspectionEntry, Limit, NodeHandle, NoopTracer, PrintTracer,
    PrintTracerBuilder, ResolvedRuntimeLimits, RuntimeEffect, RuntimeError, RuntimeLimitSpec,
    Tracer, VM, VMBuilder, Value, ValueMaterializer, Verbosity, debug_verify_type,
    extract_inspection, materialize_verified,
};
