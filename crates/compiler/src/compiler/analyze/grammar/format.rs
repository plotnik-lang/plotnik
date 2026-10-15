use crate::core::NodeKindId;
use crate::core::grammar::Grammar;

pub(super) fn format_kind(grammar: &Grammar, kind: NodeKindId) -> String {
    let name = grammar.node_kind(kind).expect("resolved kind has a name");
    if grammar.is_anonymous_node(kind) {
        return format!("{name:?}");
    }

    name.to_string()
}

pub(super) fn format_list(items: &[&str], max_items: usize) -> String {
    let shown = items
        .iter()
        .take(max_items)
        .map(|item| format!("`{item}`"))
        .collect::<Vec<_>>()
        .join(", ");
    if items.len() <= max_items {
        return shown;
    }

    format!("{shown}, … ({} more)", items.len() - max_items)
}
