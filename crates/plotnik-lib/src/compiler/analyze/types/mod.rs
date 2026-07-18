//! Result-type inference over the query AST.

mod capture;
mod entry_points;
mod naming;
pub mod type_analysis;
pub mod type_check;
mod type_description;
pub mod type_shape;

pub use capture::{
    BuiltInCaptureType, CaptureFact, CaptureKind, CaptureTypePlan, CaptureTypePlanKind,
    FieldCompletion, FieldCompletions, OptionMode, RawCaptureFact, TerminalData,
};
pub use entry_points::check_entry_points;
pub use type_analysis::TypeAnalysis;
pub use type_shape::TypeShape;
