//! Stable function names in the generated Rust module's public contract.

use crate::compiler::emit::targets::rust::ident::snake_ident;

/// Public match-journal function for a definition (`FooBar` → `foo_bar_journal`).
pub fn journal_fn_name(def_name: &str) -> String {
    format!("{}_journal", snake_ident(def_name))
}

/// Limit-applying journal entry used by the generated typed API.
pub(crate) fn limited_journal_fn_name(def_name: &str) -> String {
    format!("{}_journal_limited", snake_ident(def_name))
}

/// Limit-applying yes/no entry used by generated `matches` APIs.
pub(crate) fn matches_fn_name(def_name: &str) -> String {
    format!("{}_matches", snake_ident(def_name))
}
