//! Runtime engine for executing compiled Plotnik queries.
//!
//! The VM executes bytecode against tree-sitter syntax trees,
//! producing an effect log that can be materialized into output values.

mod checkpoint;
mod cursor;
mod effect;
mod error;
mod frame;
mod inspect;
mod limits;
mod materializer;
mod recording;
mod trace;
mod value;
mod verify;
mod vm;

#[cfg(test)]
mod checkpoint_tests;

pub use effect::{EffectLog, RuntimeEffect};
pub use error::RuntimeError;
pub use inspect::{Binding, Inspection, InspectionEntry, extract_inspection};
pub use limits::{Limit, ResolvedRuntimeLimits, RuntimeLimitSpec};
pub use materializer::{ValueMaterializer, materialize_verified};
pub use recording::{NodeRef, Recording, RecordingTracer, StepEvent, StepRecord};
pub use trace::{NoopTracer, PrintTracer, PrintTracerBuilder, Tracer, Verbosity};
pub use value::{NodeHandle, Value};
pub use verify::debug_verify_type;
pub use vm::{RunStats, VM, VMBuilder};
