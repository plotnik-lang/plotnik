plotnik::query! {
    "Text = (identifier) @text :: text Probe = (identifier) List = (program (expression_statement (identifier) @node)* @items)",
    grammar = "tree-sitter-javascript",
}

fn node_outlives_tree(source: &str) -> plotnik::rt::Node<'static> {
    let mut parser = plotnik::tree_sitter::Parser::new();
    parser.set_language(&tree_sitter_javascript::LANGUAGE.into()).unwrap();
    let tree = parser.parse(source, None).unwrap();
    Probe::find(tree.root_node(), source).unwrap().unwrap()
}

fn text_outlives_source(node: plotnik::rt::Node<'_>) -> plotnik::rt::Match<Text<'static>> {
    let source = String::from("name");
    Text::exec(node, &source).unwrap().unwrap()
}

fn rows_outlive_owner<'t>(node: plotnik::rt::Node<'t>, source: &str) -> List<'static, 't> {
    let result = List::exec(node, source).unwrap().unwrap();
    *result.view()
}

fn rows_outlive_collection<'t>(node: plotnik::rt::Node<'t>, source: &str) -> List<'static, 't> {
    let results = List::find_all(node, source).unwrap();
    *(&results).into_iter().next().unwrap()
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
