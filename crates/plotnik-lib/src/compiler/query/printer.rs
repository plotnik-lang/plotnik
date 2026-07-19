//! AST/CST pretty-printer for debugging and test snapshots.

use std::collections::BTreeMap;
use std::fmt::Write;

use indexmap::IndexSet;
use rowan::NodeOrToken;

use crate::compiler::analyze::refs::DefinitionGraph;
use crate::compiler::ids::DefId;
use crate::compiler::parse::ast::{Capture, DefRef};
use crate::compiler::parse::{self as ast, SyntaxNode};
use crate::compiler::source::{SourceKind, SourceMap};
use crate::core::Interner;

use super::Query;

fn indent(level: usize) -> String {
    "  ".repeat(level)
}

pub(crate) struct QueryPrinter<'q> {
    query: &'q Query,
    cst: bool,
    trivia: bool,
    definitions: bool,
}

impl<'q> QueryPrinter<'q> {
    pub(crate) fn new(query: &'q Query) -> Self {
        Self {
            query,
            cst: false,
            trivia: false,
            definitions: false,
        }
    }

    pub(crate) fn cst(mut self, value: bool) -> Self {
        self.cst = value;
        self
    }

    pub(crate) fn with_trivia(mut self, value: bool) -> Self {
        self.trivia = value;
        self
    }

    pub(crate) fn definitions_only(mut self, value: bool) -> Self {
        self.definitions = value;
        self
    }

    pub(crate) fn dump(&self) -> String {
        let mut out = String::new();
        self.format(&mut out).expect("String write never fails");
        out
    }

    pub(crate) fn format(&self, w: &mut impl Write) -> std::fmt::Result {
        if self.definitions {
            return self.format_definitions(w);
        }

        let source_map = self.query.source_map();
        let ast_map = self.query.ast_map();
        let show_headers = self.should_show_headers(source_map);
        let mut first = true;

        for source in source_map.iter() {
            let Some(root) = ast_map.get(&source.id) else {
                continue;
            };

            if show_headers {
                if !first {
                    writeln!(w)?;
                }
                writeln!(w, "# {}", source.kind.display_name())?;
            }

            let mut writer = AstWriter::new(self, w);
            if self.cst {
                writer.format_cst(root.syntax(), 0)?;
            } else {
                writer.format_root(root)?;
            }

            first = false;
        }

        Ok(())
    }

    fn should_show_headers(&self, source_map: &SourceMap) -> bool {
        source_map.len() > 1
            || source_map
                .iter()
                .next()
                .is_some_and(|s| !matches!(s.kind, SourceKind::Inline))
    }

    fn format_definitions(&self, w: &mut impl Write) -> std::fmt::Result {
        let Some(analysis) = self.query.analysis() else {
            return Ok(());
        };
        if analysis.definitions.ids_in_declaration_order().is_empty() {
            return Ok(());
        }

        let mut writer = DefinitionWriter::new(&analysis.definitions, &analysis.interner, w);
        for &def_id in analysis.definitions.ids_in_declaration_order() {
            writer.format_definition_tree(def_id, 0)?;
        }
        Ok(())
    }
}

struct DefinitionWriter<'w, 'a, W> {
    definitions: &'a DefinitionGraph,
    interner: &'a Interner,
    active_path: IndexSet<DefId>,
    w: &'w mut W,
}

impl<'w, 'a, W: Write> DefinitionWriter<'w, 'a, W> {
    fn new(definitions: &'a DefinitionGraph, interner: &'a Interner, w: &'w mut W) -> Self {
        Self {
            definitions,
            interner,
            active_path: IndexSet::new(),
            w,
        }
    }

    fn format_definition_tree(&mut self, def_id: DefId, depth: usize) -> std::fmt::Result {
        let prefix = indent(depth);
        let definition = self.definitions.definition(def_id);
        let name = self.interner.resolve(definition.name());

        if self.active_path.contains(&def_id) {
            writeln!(self.w, "{}{} (cycle)", prefix, name)?;
            return Ok(());
        }

        writeln!(self.w, "{}{}", prefix, name)?;
        self.active_path.insert(def_id);

        let mut references = BTreeMap::new();
        for reference in definition
            .body()
            .syntax()
            .descendants()
            .filter_map(DefRef::cast)
        {
            let Some(name) = reference.name() else {
                continue;
            };
            references
                .entry(name.text().to_string())
                .or_insert_with(|| self.definitions.reference_target(&reference));
        }
        for (referenced_name, referenced_id) in references {
            if let Some(referenced_id) = referenced_id {
                self.format_definition_tree(referenced_id, depth + 1)?;
            } else {
                writeln!(self.w, "{}{}?", indent(depth + 1), referenced_name)?;
            }
        }

        self.active_path.shift_remove(&def_id);
        Ok(())
    }
}

struct AstWriter<'p, 'q, W> {
    printer: &'p QueryPrinter<'q>,
    w: &'p mut W,
}

impl<'p, 'q, W: Write> AstWriter<'p, 'q, W> {
    fn new(printer: &'p QueryPrinter<'q>, w: &'p mut W) -> Self {
        Self { printer, w }
    }

    fn format_cst(&mut self, node: &SyntaxNode, depth: usize) -> std::fmt::Result {
        let prefix = indent(depth);

        writeln!(self.w, "{}{:?}", prefix, node.kind())?;

        for child in node.children_with_tokens() {
            match child {
                NodeOrToken::Node(n) => self.format_cst(&n, depth + 1)?,
                NodeOrToken::Token(t) => {
                    if !self.printer.trivia && t.kind().is_trivia() {
                        continue;
                    }
                    let child_prefix = indent(depth + 1);
                    writeln!(self.w, "{}{:?} {:?}", child_prefix, t.kind(), t.text())?;
                }
            }
        }
        Ok(())
    }

    fn format_root(&mut self, root: &ast::Root) -> std::fmt::Result {
        writeln!(self.w, "Root")?;

        for def in root.defs() {
            self.format_def(&def, 1)?;
        }
        // Parser wraps all top-level patterns in Def nodes, so this should be empty
        assert!(
            root.patterns().next().is_none(),
            "printer: unexpected bare Pattern in Root (parser should wrap in Def)"
        );
        Ok(())
    }

    fn format_def(&mut self, def: &ast::Def, depth: usize) -> std::fmt::Result {
        let prefix = indent(depth);
        let name = def.name().map(|t| t.text().to_string());

        match name {
            Some(n) => writeln!(self.w, "{}Def {}", prefix, n)?,
            None => writeln!(self.w, "{}Def", prefix)?,
        }

        let Some(body) = def.body() else {
            return Ok(());
        };
        self.format_pattern(&body, depth + 1)
    }

    fn format_pattern(&mut self, pattern: &ast::Pattern, depth: usize) -> std::fmt::Result {
        let prefix = indent(depth);

        match pattern {
            ast::Pattern::NamedNodePattern(n) => {
                if n.is_any() {
                    writeln!(self.w, "{}NamedNode (any)", prefix)?;
                } else {
                    let node_kind = n.kind_token().map(|tok| tok.text().to_string());
                    match node_kind {
                        Some(ty) => writeln!(self.w, "{}NamedNode {}", prefix, ty)?,
                        None => writeln!(self.w, "{}NamedNode", prefix)?,
                    }
                }
                self.format_tree_children(n.syntax(), depth + 1)?;
            }
            ast::Pattern::DefRef(r) => {
                let name = r.name().map(|t| t.text().to_string()).unwrap_or_default();
                writeln!(self.w, "{}Ref {}", prefix, name)?;
            }
            ast::Pattern::AnonymousNodePattern(node) => {
                let value = node
                    .value()
                    .map(|token| token.text().to_string())
                    .unwrap_or_default();
                writeln!(self.w, "{}AnonymousNode \"{}\"", prefix, value)?;
            }
            ast::Pattern::NodeWildcard(_) => {
                writeln!(self.w, "{}NodeWildcard", prefix)?;
            }
            ast::Pattern::Alternation(a) => {
                writeln!(self.w, "{}Alternation", prefix)?;
                for alternative in a.alternatives() {
                    self.format_alternative(&alternative, depth + 1)?;
                }
            }
            ast::Pattern::SeqPattern(s) => {
                writeln!(self.w, "{}Seq", prefix)?;
                self.format_tree_children(s.syntax(), depth + 1)?;
            }
            ast::Pattern::CapturedPattern(captured_pattern) => {
                writeln!(self.w, "{}CapturedPattern", prefix)?;
                self.format_pattern_if_present(captured_pattern.inner(), depth + 1)?;
                self.format_capture(&captured_pattern.capture(), depth + 1)?;
            }
            ast::Pattern::QuantifiedPattern(q) => {
                let op = q
                    .operator()
                    .map(|t| t.text().to_string())
                    .unwrap_or_default();
                writeln!(self.w, "{}QuantifiedPattern {}", prefix, op)?;
                self.format_pattern_if_present(q.inner(), depth + 1)?;
            }
            ast::Pattern::FieldPattern(f) => {
                let name = f.name().map(|t| t.text().to_string()).unwrap_or_default();
                writeln!(self.w, "{}FieldPattern {}:", prefix, name)?;
                self.format_pattern_if_present(f.value(), depth + 1)?;
            }
        }
        Ok(())
    }

    fn format_capture(&mut self, capture: &Capture, depth: usize) -> std::fmt::Result {
        let prefix = indent(depth);
        let name = capture
            .name()
            .map(|token| token.text().to_string())
            .unwrap_or_default();
        writeln!(self.w, "{}Capture {}", prefix, name)?;

        let Some(capture_type) = capture.capture_type() else {
            return Ok(());
        };
        let prefix = indent(depth + 1);
        let name = capture_type
            .name()
            .map(|token| token.text().to_string())
            .unwrap_or_default();
        writeln!(self.w, "{}CaptureType {}", prefix, name)
    }

    fn format_pattern_if_present(
        &mut self,
        pattern: Option<ast::Pattern>,
        depth: usize,
    ) -> std::fmt::Result {
        let Some(pattern) = pattern else {
            return Ok(());
        };

        self.format_pattern(&pattern, depth)
    }

    fn format_tree_children(&mut self, node: &SyntaxNode, depth: usize) -> std::fmt::Result {
        use crate::compiler::parse::SyntaxKind;
        for child in node.children() {
            if child.kind() == SyntaxKind::Anchor {
                self.mark_anchor(
                    &ast::Anchor::cast(child).expect("child is an Anchor by the matched kind"),
                    depth,
                )?;
            } else if child.kind() == SyntaxKind::NegatedField {
                self.format_negated_field(
                    &ast::NegatedField::cast(child)
                        .expect("child is a NegatedField by the matched kind"),
                    depth,
                )?;
            } else if let Some(pattern) = ast::Pattern::cast(child) {
                self.format_pattern(&pattern, depth)?;
            }
        }
        Ok(())
    }

    fn mark_anchor(&mut self, anchor: &ast::Anchor, depth: usize) -> std::fmt::Result {
        let spelling = if anchor.is_exact() { ".!" } else { "." };
        writeln!(self.w, "{}{}", indent(depth), spelling)
    }

    fn format_negated_field(&mut self, nf: &ast::NegatedField, depth: usize) -> std::fmt::Result {
        let prefix = indent(depth);
        let name = nf.name().map(|t| t.text().to_string()).unwrap_or_default();
        writeln!(self.w, "{}NegatedField -{}", prefix, name)
    }

    fn format_alternative(
        &mut self,
        alternative: &ast::Alternative,
        depth: usize,
    ) -> std::fmt::Result {
        let prefix = indent(depth);
        let label = alternative.label().map(|t| t.text().to_string());

        match label {
            Some(l) => writeln!(self.w, "{}Alternative {}:", prefix, l)?,
            None => writeln!(self.w, "{}Alternative", prefix)?,
        }

        let Some(body) = alternative.body() else {
            return Ok(());
        };
        self.format_pattern(&body, depth + 1)
    }
}

impl Query {
    pub(crate) fn printer(&self) -> QueryPrinter<'_> {
        QueryPrinter::new(self)
    }
}
