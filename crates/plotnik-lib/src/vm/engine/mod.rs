//! Runtime engine for executing compiled Plotnik queries.
//!
//! The VM executes bytecode against tree-sitter syntax trees,
//! producing an effect log that can be materialized into output values.

mod error;
mod inspect;
mod materializer;
mod recording;
mod trace;
mod value;
mod verify;
mod vm;

// Navigation, checkpoints, frames, the effect log, and limits live in
// `plotnik-rt`, shared with the generated-code backend; re-exported so the
// crate-facing paths stay `vm::...`.
pub use plotnik_rt::{EffectLog, Limit, ResolvedRuntimeLimits, RuntimeEffect, RuntimeLimitSpec};

pub use error::RuntimeError;
pub use inspect::{Binding, Inspection, InspectionEntry, extract_inspection};
pub use materializer::{ValueMaterializer, materialize_verified};
pub use recording::{NodeRef, Recording, RecordingTracer, StepEvent, StepRecord};
pub use trace::{NoopTracer, PrintTracer, PrintTracerBuilder, Tracer, Verbosity};
pub use value::{NodeHandle, Value};
pub use verify::debug_verify_type;
pub use vm::{RunStats, VM, VMBuilder};
