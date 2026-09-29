//! Formatting shared by grammar diagnostics.

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
