use crate::compiler::emit::targets::rust::ident::snake_ident;

/// Public match-journal function for a definition (`FooBar` → `foo_bar_journal`).
pub fn journal_fn_name(def_name: &str) -> String {
    format!("{}_journal", snake_ident(def_name))
}

pub(crate) fn apply_fn_name(def_name: &str) -> String {
    format!("{}_apply", snake_ident(def_name))
}
