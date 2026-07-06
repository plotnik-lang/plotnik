use arborium_tree_sitter::{Language as TsLanguage, Node, Parser as TsParser, Tree};
use indoc::indoc;

use crate::bytecode::Module;
use crate::compiler::test_utils::javascript_grammar;
use crate::{QueryBuilder, RuntimeEffect, extract_inspection};

fn compile(src: &str) -> Module {
    let compiled = QueryBuilder::from_inline(src)
        .with_inspection(true)
        .compile(javascript_grammar())
        .expect("query parsing should not exhaust fuel");
    assert!(
        compiled.is_valid(),
        "{}",
        compiled.diagnostics().render(compiled.source_map())
    );
    compiled.into_module().expect("valid query emits module")
}

fn parse_js(source: &str) -> Tree {
    let mut parser = TsParser::new();
    let lang: TsLanguage = arborium_javascript::language().into();
    parser.set_language(&lang).expect("set javascript language");
    parser.parse(source, None).expect("parse javascript source")
}

fn find_node<'t>(tree: &'t Tree, source: &str, kind: &str, text: &str) -> Node<'t> {
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.kind() == kind && node_text(source, node) == text {
            return node;
        }
        for i in (0..node.child_count()).rev() {
            let child_idx = u32::try_from(i).expect("child index fits u32");
            if let Some(child) = node.child(child_idx) {
                stack.push(child);
            }
        }
    }
    panic!("node {kind:?} with text {text:?} not found");
}

fn node_text<'s>(source: &'s str, node: Node<'_>) -> &'s str {
    source
        .get(node.start_byte()..node.end_byte())
        .expect("test node span is valid UTF-8")
}

fn member_index(module: &Module, name: &str) -> u16 {
    let strings = module.strings();
    module
        .types()
        .members()
        .enumerate()
        .find(|(_, member)| strings.get(member.name_id) == name)
        .map(|(idx, _)| u16::try_from(idx).expect("member index fits u16"))
        .expect("member exists in test module")
}

#[test]
fn child_hull_rolls_up_to_parent_span() {
    let module = compile("Q = (program)");
    let source = "x";
    let tree = parse_js(source);
    let id = find_node(&tree, source, "identifier", "x");
    let effects = vec![
        RuntimeEffect::SpanStart { id: 0, node: None },
        RuntimeEffect::SpanStart {
            id: 1,
            node: Some(id),
        },
        RuntimeEffect::SpanEnd(1),
        RuntimeEffect::SpanEnd(0),
    ];

    let inspection = extract_inspection(&effects, &module);

    assert_eq!(inspection.v, 1);
    assert_eq!(inspection.entries.len(), 2);
    assert_eq!(inspection.entries[0].hull, Some((0, 1)));
    assert_eq!(inspection.entries[0].effect_range, (0, 3));
    assert_eq!(inspection.entries[1].parent, Some(0));
    assert_eq!(inspection.entries[1].hull, Some((0, 1)));
    assert_eq!(inspection.entries[1].effect_range, (1, 2));
}

#[test]
fn enum_open_binds_tag_path() {
    let module = compile(indoc! {"
        Expr = [Id: (identifier) @id]
        Q = (program (expression_statement (Expr) @expr))
    "});
    let id_variant = member_index(&module, "Id");
    let effects = vec![
        RuntimeEffect::SpanStart { id: 0, node: None },
        RuntimeEffect::EnumOpen(id_variant),
        RuntimeEffect::EnumClose,
        RuntimeEffect::SpanEnd(0),
    ];

    let inspection = extract_inspection(&effects, &module);

    let bindings = &inspection.entries[0].bindings;
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].path, "/$tag");
    assert_eq!(bindings[0].effect_idx, 1);
}

#[test]
fn array_pushes_bind_current_indices() {
    let module = compile("Q = (program (expression_statement)* @stmts)");
    let source = "x\n42";
    let tree = parse_js(source);
    let id = find_node(&tree, source, "identifier", "x");
    let number = find_node(&tree, source, "number", "42");
    let effects = vec![
        RuntimeEffect::SpanStart { id: 0, node: None },
        RuntimeEffect::ArrayOpen,
        RuntimeEffect::Node(id),
        RuntimeEffect::Push,
        RuntimeEffect::Node(number),
        RuntimeEffect::Push,
        RuntimeEffect::ArrayClose,
        RuntimeEffect::SpanEnd(0),
    ];

    let inspection = extract_inspection(&effects, &module);

    let bindings = &inspection.entries[0].bindings;
    assert_eq!(bindings.len(), 2);
    assert_eq!(bindings[0].path, "/0");
    assert_eq!(bindings[0].effect_idx, 3);
    assert_eq!(bindings[1].path, "/1");
    assert_eq!(bindings[1].effect_idx, 5);
    assert_eq!(inspection.entries[0].hull, Some((0, 4)));
}

#[test]
fn empty_marker_pair_has_no_hull() {
    let module = compile("Q = (program)");
    let effects = vec![
        RuntimeEffect::SpanStart { id: 0, node: None },
        RuntimeEffect::SpanEnd(0),
    ];

    let inspection = extract_inspection(&effects, &module);

    assert_eq!(inspection.entries.len(), 1);
    assert_eq!(inspection.entries[0].hull, None);
    assert!(inspection.entries[0].bindings.is_empty());
    assert_eq!(inspection.entries[0].effect_range, (0, 1));
}

#[test]
fn hull_only_span_records_no_bindings() {
    let module = compile("Q = (program)");
    let source = "x";
    let tree = parse_js(source);
    let id = find_node(&tree, source, "identifier", "x");
    let effects = vec![
        RuntimeEffect::SpanStart {
            id: 0,
            node: Some(id),
        },
        RuntimeEffect::SpanEnd(0),
    ];

    let inspection = extract_inspection(&effects, &module);

    assert_eq!(inspection.entries[0].hull, Some((0, 1)));
    assert!(inspection.entries[0].bindings.is_empty());
}
