//! Result inference and static root extent computed over the query AST.

pub mod capture_kind;
mod capture_type;
mod entry_points;
mod naming;
mod raw_output;
mod root_extent;
pub mod type_analysis;
pub mod type_check;
pub mod type_shape;

pub use capture_kind::CaptureKind;
pub use capture_type::{
    BuiltInCaptureType, CaptureFact, CaptureTypePlan, CaptureTypePlanKind, FieldCompletion,
    FieldCompletions, OptionMode, RawCaptureFact, TerminalData,
};
pub use entry_points::check_entry_points;
pub use root_extent::RootExtent;
pub use type_analysis::TypeAnalysis;
pub use type_shape::TypeShape;

#[cfg(test)]
mod root_extent_tests;
