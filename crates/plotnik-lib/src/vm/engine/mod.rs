//! Runtime engine for executing compiled Plotnik queries.
//!
//! The VM executes bytecode against tree-sitter syntax trees,
//! producing a match journal that can be materialized into result values.

mod error;
mod execution_trace;
mod materializer;
mod result_provenance;
mod trace;
mod value;
mod verify;
mod vm;

// Navigation, checkpoints, frames, the match journal, and limits live in
// `plotnik-rt`, shared with the generated-code backend; re-exported so the
// crate-facing paths stay `vm::...`.
pub use plotnik_runtime::{
    JournalEvent, Limit, MatchJournal, OutputEvents, ResolvedRuntimeLimits, RuntimeLimitSpec,
};

pub use error::RuntimeError;
pub use execution_trace::{ExecutionTrace, TraceEvent, TraceNode, TraceRecord, TraceRecorder};
pub use materializer::{ValueMaterializer, materialize_verified};
pub use result_provenance::{ProvenanceBinding, ResultProvenanceEntry, extract_result_provenance};
pub use trace::{NoopTracer, PrintTracer, PrintTracerBuilder, Tracer, Verbosity};
pub use value::{NodeValue, Value};
pub use verify::debug_verify_type;
pub use vm::{RunStats, VM, VMBuilder};
