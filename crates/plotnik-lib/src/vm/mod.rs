//! Runtime VM for executing compiled Plotnik queries.
//!
//! This module provides the virtual machine that executes bytecode against
//! tree-sitter syntax trees, producing structured output.

#![allow(clippy::comparison_chain)]

mod engine;

pub use engine::{
    JournalEvent, Limit, MatchJournal, NodeHandle, NoopTracer, PrintTracer, PrintTracerBuilder,
    ProvenanceBinding, Recording, RecordingTracer, ResolvedRuntimeLimits, ResultProvenance,
    ResultProvenanceEntry, RunStats, RuntimeError, RuntimeLimitSpec, StepEvent, StepRecord,
    TraceNode, Tracer, VM, VMBuilder, Value, ValueMaterializer, Verbosity, debug_verify_type,
    extract_result_provenance, materialize_verified,
};
