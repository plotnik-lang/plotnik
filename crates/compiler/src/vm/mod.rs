#![allow(clippy::comparison_chain)]

mod engine;

pub use engine::{
    ExecutionTrace, JournalEvent, Limit, MatchJournal, NodeValue, NoopTracer, OutputEvents,
    PrintTracer, PrintTracerBuilder, ProvenanceBinding, ResolvedRuntimeLimits,
    ResultProvenanceEntry, RunStats, RuntimeError, RuntimeLimitSpec, TraceEvent, TraceNode,
    TraceRecord, TraceRecorder, Tracer, VM, VMBuilder, Value, ValueMaterializer, Verbosity,
    debug_verify_type, extract_result_provenance, materialize_verified,
};
