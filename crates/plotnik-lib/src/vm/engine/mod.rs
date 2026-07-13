//! Runtime engine for executing compiled Plotnik queries.
//!
//! The VM executes bytecode against tree-sitter syntax trees,
//! producing a match journal that can be materialized into output values.

mod error;
mod inspect;
mod materializer;
mod recording;
mod trace;
mod value;
mod verify;
mod vm;

// Navigation, checkpoints, frames, the match journal, and limits live in
// `plotnik-rt`, shared with the generated-code backend; re-exported so the
// crate-facing paths stay `vm::...`.
pub use plotnik_rt::{JournalEvent, Limit, MatchJournal, ResolvedRuntimeLimits, RuntimeLimitSpec};

pub use error::RuntimeError;
pub use inspect::{Binding, Inspection, InspectionEntry, extract_inspection};
pub use materializer::{ValueMaterializer, materialize_verified};
pub use recording::{Recording, RecordingTracer, StepEvent, StepRecord, TraceNode};
pub use trace::{NoopTracer, PrintTracer, PrintTracerBuilder, Tracer, Verbosity};
pub use value::{NodeHandle, Value};
pub use verify::debug_verify_type;
pub use vm::{RunStats, VM, VMBuilder};
