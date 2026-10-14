plotnik::query! {
    "Text = (identifier) @text :: text Probe = (identifier)",
    grammar = "tree-sitter-javascript",
}

fn node_outlives_tree(source: &str) -> plotnik::rt::Node<'static> {
    let mut parser = plotnik::tree_sitter::Parser::new();
    parser.set_language(&tree_sitter_javascript::LANGUAGE.into()).unwrap();
    let tree = parser.parse(source, None).unwrap();
    Probe::find(tree.root_node(), source).unwrap().unwrap()
}

fn text_outlives_source(node: plotnik::rt::Node<'_>) -> Text<'static> {
    let source = String::from("name");
    Text::exec(node, &source).unwrap().unwrap()
}

fn iterator_outlives_source(node: plotnik::rt::Node<'_>) {
    let mut each;
    {
        let source = String::from("name");
        each = Probe::each(node, &source);
    }
    let _ = each.next();
}

fn main() {}
