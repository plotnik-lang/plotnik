plotnik::query! {
    grammar = "tree-sitter-javascript",
    r#"
    Left = left: (_) @node
    Alias = (Left) @value
    Wrapped = {(Alias) @value}
    Choice = [{left: (_) @node} {right: (_) @node}]
    Mixed = [(Left) (_)]
    Q = (program (expression_statement
      (binary_expression (Wrapped) @left (Choice) @right)))
    "#
}

fn standalone(tree: &plotnik::rt::Tree, source: &str) {
    let _ = Left::exec(tree.root_node(), source);
    let _ = Wrapped::each(tree.root_node(), source);
    let _ = Choice::find(tree.root_node(), source);
    let _ = Mixed::find_all(tree.root_node(), source);
    let _ = left_journal(tree, source);
}

fn main() {}
