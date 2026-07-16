//! AST/CST pretty-printer for debugging and test snapshots.

use std::collections::HashMap;
use std::fmt::Write;

use indexmap::IndexSet;
use rowan::NodeOrToken;

use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::analyze::types::type_check::RootExtent;
use crate::compiler::parse::ast::Capture;
use crate::compiler::parse::{self as ast, SyntaxNode};
use crate::compiler::source::{SourceKind, SourceMap};

use super::Query;

fn indent(level: usize) -> String {
    "  ".repeat(level)
}

pub(crate) struct QueryPrinter<'q> {
    query: &'q Query,
    cst: bool,
    trivia: bool,
    root_extents: bool,
    spans: bool,
    definitions: bool,
}

impl<'q> QueryPrinter<'q> {
    pub(crate) fn new(query: &'q Query) -> Self {
        Self {
            query,
            cst: false,
            trivia: false,
            root_extents: false,
            spans: false,
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

    #[cfg(test)]
    pub(crate) fn with_root_extents(mut self, value: bool) -> Self {
        self.root_extents = value;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_spans(mut self, value: bool) -> Self {
        self.spans = value;
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
            return self.format_symbols(w);
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

    fn format_symbols(&self, w: &mut impl Write) -> std::fmt::Result {
        let Some(analysis) = self.query.analysis() else {
            return Ok(());
        };
        let symbols = &analysis.symbol_table;
        if symbols.is_empty() {
            return Ok(());
        }

        let defined: IndexSet<&str> = symbols.names().collect();

        // Collect body nodes from all files
        let mut body_nodes: HashMap<String, SyntaxNode> = HashMap::new();
        for root in self.query.ast_map().values() {
            for def in root.defs() {
                if let (Some(name_tok), Some(body)) = (def.name(), def.body()) {
                    body_nodes.insert(name_tok.text().to_string(), body.syntax().clone());
                }
            }
        }

        let mut writer = SymbolWriter::new(self, symbols, &defined, &body_nodes, w);
        for name in symbols.names() {
            writer.format_symbol_tree(name, 0)?;
        }
        Ok(())
    }
}

struct SymbolWriter<'p, 'q, 'a, W> {
    printer: &'p QueryPrinter<'q>,
    symbols: &'a SymbolTable,
    defined: &'a IndexSet<&'a str>,
    body_nodes: &'a HashMap<String, SyntaxNode>,
    visited: IndexSet<String>,
    w: &'p mut W,
}

impl<'p, 'q, 'a, W: Write> SymbolWriter<'p, 'q, 'a, W> {
    fn new(
        printer: &'p QueryPrinter<'q>,
        symbols: &'a SymbolTable,
        defined: &'a IndexSet<&'a str>,
        body_nodes: &'a HashMap<String, SyntaxNode>,
        w: &'p mut W,
    ) -> Self {
        Self {
            printer,
            symbols,
            defined,
            body_nodes,
            visited: IndexSet::new(),
            w,
        }
    }

    fn format_symbol_tree(&mut self, name: &str, depth: usize) -> std::fmt::Result {
        let prefix = indent(depth);

        if self.visited.contains(name) {
            writeln!(self.w, "{}{} (cycle)", prefix, name)?;
            return Ok(());
        }

        let is_broken = !self.defined.contains(name);
        if is_broken {
            writeln!(self.w, "{}{}?", prefix, name)?;
            return Ok(());
        }

        let extent = self
            .body_nodes
            .get(name)
            .map(|node| self.printer.root_extent_glyph(node))
            .unwrap_or("");
        writeln!(self.w, "{}{}{}", prefix, name, extent)?;
        self.visited.insert(name.to_string());

        if let Some(body) = self.symbols.body(name) {
            let refs_set = crate::compiler::analyze::refs::collect::ref_names(body);
            let mut refs: Vec<_> = refs_set.iter().map(|s| s.as_str()).collect();
            refs.sort();
            for r in refs {
                self.format_symbol_tree(r, depth + 1)?;
            }
        }

        self.visited.shift_remove(name);
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
        let extent = self.printer.root_extent_glyph(node);
        let span = self.printer.span_str(node.text_range());

        writeln!(self.w, "{}{:?}{}{}", prefix, node.kind(), extent, span)?;

        for child in node.children_with_tokens() {
            match child {
                NodeOrToken::Node(n) => self.format_cst(&n, depth + 1)?,
                NodeOrToken::Token(t) => {
                    if !self.printer.trivia && t.kind().is_trivia() {
                        continue;
                    }
                    let child_prefix = indent(depth + 1);
                    let child_span = self.printer.span_str(t.text_range());
                    writeln!(
                        self.w,
                        "{}{:?}{} {:?}",
                        child_prefix,
                        t.kind(),
                        child_span,
                        t.text()
                    )?;
                }
            }
        }
        Ok(())
    }

    fn format_root(&mut self, root: &ast::Root) -> std::fmt::Result {
        let span = self.printer.span_str(root.text_range());
        writeln!(self.w, "Root{}", span)?;

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
        let extent = self.printer.root_extent_glyph(def.syntax());
        let span = self.printer.span_str(def.text_range());
        let name = def.name().map(|t| t.text().to_string());

        match name {
            Some(n) => writeln!(self.w, "{}Def{}{} {}", prefix, extent, span, n)?,
            None => writeln!(self.w, "{}Def{}{}", prefix, extent, span)?,
        }

        let Some(body) = def.body() else {
            return Ok(());
        };
        self.format_pattern(&body, depth + 1)
    }

    fn format_pattern(&mut self, pattern: &ast::Pattern, depth: usize) -> std::fmt::Result {
        let prefix = indent(depth);
        let extent = self.printer.root_extent_glyph(pattern.syntax());
        let span = self.printer.span_str(pattern.text_range());

        match pattern {
            ast::Pattern::NamedNodePattern(n) => {
                if n.is_any() {
                    writeln!(self.w, "{}NamedNode{}{} (any)", prefix, extent, span)?;
                } else {
                    let node_kind = n.kind_token().map(|tok| tok.text().to_string());
                    match node_kind {
                        Some(ty) => {
                            writeln!(self.w, "{}NamedNode{}{} {}", prefix, extent, span, ty)?
                        }
                        None => writeln!(self.w, "{}NamedNode{}{}", prefix, extent, span)?,
                    }
                }
                self.format_tree_children(n.syntax(), depth + 1)?;
            }
            ast::Pattern::DefRef(r) => {
                let name = r.name().map(|t| t.text().to_string()).unwrap_or_default();
                writeln!(self.w, "{}Ref{}{} {}", prefix, extent, span, name)?;
            }
            ast::Pattern::AnonymousNodePattern(node) => {
                let value = node
                    .value()
                    .map(|token| token.text().to_string())
                    .unwrap_or_default();
                writeln!(
                    self.w,
                    "{}AnonymousNode{}{} \"{}\"",
                    prefix, extent, span, value
                )?;
            }
            ast::Pattern::NodeWildcard(_) => {
                writeln!(self.w, "{}NodeWildcard{}{}", prefix, extent, span)?;
            }
            ast::Pattern::Alternation(a) => {
                writeln!(self.w, "{}Alternation{}{}", prefix, extent, span)?;
                for alternative in a.alternatives() {
                    self.format_alternative(&alternative, depth + 1)?;
                }
            }
            ast::Pattern::SeqPattern(s) => {
                writeln!(self.w, "{}Seq{}{}", prefix, extent, span)?;
                self.format_tree_children(s.syntax(), depth + 1)?;
            }
            ast::Pattern::CapturedPattern(captured_pattern) => {
                writeln!(self.w, "{}CapturedPattern{}{}", prefix, extent, span)?;
                self.format_pattern_if_present(captured_pattern.inner(), depth + 1)?;
                self.format_capture(&captured_pattern.capture(), depth + 1)?;
            }
            ast::Pattern::QuantifiedPattern(q) => {
                let op = q
                    .operator()
                    .map(|t| t.text().to_string())
                    .unwrap_or_default();
                writeln!(
                    self.w,
                    "{}QuantifiedPattern{}{} {}",
                    prefix, extent, span, op
                )?;
                self.format_pattern_if_present(q.inner(), depth + 1)?;
            }
            ast::Pattern::FieldPattern(f) => {
                let name = f.name().map(|t| t.text().to_string()).unwrap_or_default();
                writeln!(self.w, "{}FieldPattern{}{} {}:", prefix, extent, span, name)?;
                self.format_pattern_if_present(f.value(), depth + 1)?;
            }
        }
        Ok(())
    }

    fn format_capture(&mut self, capture: &Capture, depth: usize) -> std::fmt::Result {
        let prefix = indent(depth);
        let span = self.printer.span_str(capture.text_range());
        let name = capture
            .name()
            .map(|token| token.text().to_string())
            .unwrap_or_default();
        writeln!(self.w, "{}Capture{} {}", prefix, span, name)?;

        let Some(capture_type) = capture.capture_type() else {
            return Ok(());
        };
        let prefix = indent(depth + 1);
        let span = self.printer.span_str(capture_type.text_range());
        let name = capture_type
            .name()
            .map(|token| token.text().to_string())
            .unwrap_or_default();
        writeln!(self.w, "{}CaptureType{} {}", prefix, span, name)
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
        let span = self.printer.span_str(nf.text_range());
        let name = nf.name().map(|t| t.text().to_string()).unwrap_or_default();
        writeln!(self.w, "{}NegatedField{} -{}", prefix, span, name)
    }

    fn format_alternative(
        &mut self,
        alternative: &ast::Alternative,
        depth: usize,
    ) -> std::fmt::Result {
        let prefix = indent(depth);
        let extent = self.printer.root_extent_glyph(alternative.syntax());
        let span = self.printer.span_str(alternative.text_range());
        let label = alternative.label().map(|t| t.text().to_string());

        match label {
            Some(l) => writeln!(self.w, "{}Alternative{}{} {}:", prefix, extent, span, l)?,
            None => writeln!(self.w, "{}Alternative{}{}", prefix, extent, span)?,
        }

        let Some(body) = alternative.body() else {
            return Ok(());
        };
        self.format_pattern(&body, depth + 1)
    }
}

impl QueryPrinter<'_> {
    fn root_extent_glyph(&self, node: &SyntaxNode) -> &'static str {
        if !self.root_extents {
            return "";
        }
        match self.query.root_extent(node) {
            Some(RootExtent::SingleNode) => "¹",
            Some(RootExtent::Other) => "≠¹",
            None => "",
        }
    }

    fn span_str(&self, range: rowan::TextRange) -> String {
        if !self.spans {
            return String::new();
        }
        format!(
            " [{}..{}]",
            u32::from(range.start()),
            u32::from(range.end())
        )
    }
}

impl Query {
    pub(crate) fn printer(&self) -> QueryPrinter<'_> {
        QueryPrinter::new(self)
    }
}
