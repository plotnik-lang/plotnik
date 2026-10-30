mod definition_graph;
mod dependencies;
mod recursion;
mod source_cycles;
#[cfg(test)]
mod source_cycles_tests;

pub(crate) use definition_graph::{DefinitionGraph, DefinitionReachability};
pub(in crate::compiler) use dependencies::build_definition_graph;
pub(in crate::compiler) use recursion::validate_recursion;
pub(in crate::compiler) use source_cycles::validate_source_cycles;
