use crate::compiler::emit::targets::rust::ident::snake_ident;

pub(crate) fn apply_fn_name(def_name: &str) -> String {
    format!("{}_apply", snake_ident(def_name))
}
