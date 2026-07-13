//! Runtime VM for executing compiled Plotnik queries.
//!
//! This module provides the virtual machine that executes bytecode against
//! tree-sitter syntax trees, producing structured output.

#![allow(clippy::comparison_chain)]

mod engine;

pub use engine::{
    Binding, Inspection, InspectionEntry, JournalEvent, Limit, MatchJournal, NodeHandle, NodeRef,
    NoopTracer, PrintTracer, PrintTracerBuilder, Recording, RecordingTracer, ResolvedRuntimeLimits,
    RunStats, RuntimeError, RuntimeLimitSpec, StepEvent, StepRecord, Tracer, VM, VMBuilder, Value,
    ValueMaterializer, Verbosity, debug_verify_type, extract_inspection, materialize_verified,
};
