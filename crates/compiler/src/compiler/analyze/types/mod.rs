//! Result-type inference over the query AST.

mod capture;
mod entrypoints;
mod naming;
pub mod type_analysis;
pub mod type_check;
mod type_description;
pub mod type_shape;

pub use capture::{
    BuiltInCaptureType, CaptureFact, CaptureKind, CaptureTypePlan, CaptureTypePlanKind,
    FieldCompletion, FieldCompletions, OptionMode, RawCaptureFact, TerminalData,
};
pub use entrypoints::check_entrypoints;
pub use type_analysis::TypeAnalysis;
pub use type_shape::TypeShape;
