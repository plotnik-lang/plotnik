//! Runtime VM for executing compiled Plotnik queries.
//!
//! This module provides the virtual machine that executes bytecode against
//! tree-sitter syntax trees, producing structured output.

#![allow(clippy::comparison_chain)]

mod engine;

pub use engine::{
    ExecutionTrace, JournalEvent, Limit, MatchJournal, NodeValue, NoopTracer, OutputEvents,
    PrintTracer, PrintTracerBuilder, ProvenanceBinding, ResolvedRuntimeLimits, ResultProvenance,
    ResultProvenanceEntry, RunStats, RuntimeError, RuntimeLimitSpec, TraceEvent, TraceNode,
    TraceRecord, TraceRecorder, Tracer, VM, VMBuilder, Value, ValueMaterializer, Verbosity,
    debug_verify_type, extract_result_provenance, materialize_verified,
};
