//! Stable function names in the generated Rust module's public contract.

use crate::compiler::emit::targets::rust::ident::snake_ident;

/// Public trace function for a definition (`FooBar` → `foo_bar_trace`).
pub fn entry_fn_name(def_name: &str) -> String {
    format!("{}_trace", snake_ident(def_name))
}

/// Limit-applying trace entry used by the generated typed API.
pub(crate) fn safe_entry_fn_name(def_name: &str) -> String {
    format!("{}_safe", snake_ident(def_name))
}

/// Limit-applying yes/no entry used by generated `is_match` APIs.
pub(crate) fn accepts_entry_fn_name(def_name: &str) -> String {
    format!("{}_accepts", snake_ident(def_name))
}
