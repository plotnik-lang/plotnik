mod error;
mod execution_trace;
mod materializer;
mod result_provenance;
mod trace;
mod value;
mod verify;
mod vm;

pub use plotnik_runtime::{JournalEvent, Limit, MatchJournal, OutputEvents};

pub use error::RuntimeError;
pub use execution_trace::{ExecutionTrace, TraceEvent, TraceNode, TraceRecord, TraceRecorder};
pub use materializer::{ValueMaterializer, materialize_verified};
pub use result_provenance::{ProvenanceBinding, ResultProvenanceEntry, extract_result_provenance};
pub use trace::{NoopTracer, PrintTracer, PrintTracerBuilder, Tracer, Verbosity};
pub use value::{NodeValue, Value};
pub use verify::debug_verify_type;
pub use vm::{RunStats, VM};
