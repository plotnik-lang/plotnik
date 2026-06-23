//! Utilities for working with definition references in expressions.

use indexmap::IndexSet;

use plotnik_compiler_core::ast::{self, Pattern};

pub fn ref_nodes(pattern: &Pattern) -> impl Iterator<Item = ast::Ref> + '_ {
    pattern.syntax().descendants().filter_map(ast::Ref::cast)
}

pub fn ref_names(pattern: &Pattern) -> IndexSet<String> {
    ref_nodes(pattern)
        .filter_map(|r| r.name())
        .map(|tok| tok.text().to_string())
        .collect()
}

pub fn contains_ref(pattern: &Pattern, name: &str) -> bool {
    ref_nodes(pattern)
        .filter_map(|r| r.name())
        .any(|tok| tok.text() == name)
}
