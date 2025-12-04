//! Compiler diagnostics infrastructure.
//!
//! This module provides types for collecting, filtering, and rendering
//! diagnostic messages from parsing and analysis stages.

mod collection;
mod message;
mod printer;

#[cfg(test)]
mod tests;

pub use collection::Diagnostics;
pub use message::{DiagnosticMessage, DiagnosticStage, Fix, RelatedInfo, Severity};
pub use printer::DiagnosticsPrinter;
