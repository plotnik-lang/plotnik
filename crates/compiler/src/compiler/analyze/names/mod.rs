//! Name resolution: collect definitions and validate references.

mod collected_definitions;
mod resolve;

pub(in crate::compiler::analyze) use collected_definitions::CollectedDefinitions;
pub(in crate::compiler) use resolve::resolve_names;
