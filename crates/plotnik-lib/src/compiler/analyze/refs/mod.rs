//! Reference analysis: dependency graph and recursion validation.

mod definition_graph;
mod dependencies;
mod recursion;

pub(crate) use definition_graph::{DefinitionGraph, DefinitionReachability};
pub(in crate::compiler) use dependencies::build_definition_graph;
pub(in crate::compiler) use recursion::validate_recursion;
