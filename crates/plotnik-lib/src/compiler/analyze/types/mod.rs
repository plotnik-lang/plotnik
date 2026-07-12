//! Type inference: structural arity and data-flow types computed over the AST.

pub mod capture_kind;
mod capture_type;
mod entrypoints;
mod naming;
mod raw_output;
pub mod type_analysis;
pub mod type_check;
pub mod type_shape;

pub use capture_kind::CaptureKind;
pub use capture_type::{
    BuiltInCaptureType, CaptureFact, CaptureTypePlan, CaptureTypePlanKind, FieldFallback,
    OptionalCaptureTypeMode, RawCaptureFact, TerminalData, UnionFlowPlan,
};
pub use entrypoints::check_entrypoints;
pub use type_analysis::TypeAnalysis;
pub use type_shape::TypeShape;
