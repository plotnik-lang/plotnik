//! AST/CST pretty-printer for debugging and test snapshots.

use std::collections::HashMap;
use std::fmt::Write;

use indexmap::IndexSet;
use rowan::NodeOrToken;

use crate::compiler::analyze::types::type_check::Arity;
use crate::compiler::parse::{self as ast, SyntaxNode};
use crate::compiler::source::{SourceKind, SourceMap};

use super::Query;

fn indent(level: usize) -> String {
    "  ".repeat(level)
}

pub struct QueryPrinter<'q> {
    query: &'q Query,
    cst: bool,
    trivia: bool,
    arities: bool,
    spans: bool,
    definitions: bool,
}

impl<'q> QueryPrinter<'q> {
    pub fn new(query: &'q Query) -> Self {
        Self {
            query,
            cst: false,
            trivia: false,
            arities: false,
            spans: false,
            definitions: false,
        }
    }

    pub fn cst(mut self, value: bool) -> Self {
        self.cst = value;
        self
    }

    pub fn with_trivia(mut self, value: bool) -> Self {
        self.trivia = value;
        self
    }

    pub fn with_arities(mut self, value: bool) -> Self {
        self.arities = value;
        self
    }

    pub fn with_spans(mut self, value: bool) -> Self {
        self.spans = value;
        self
    }

    pub fn definitions_only(mut self, value: bool) -> Self {
        self.definitions = value;
        self
    }

    pub fn dump(&self) -> String {
        let mut out = String::new();
        self.format(&mut out).expect("String write never fails");
        out
    }

    pub fn format(&self, w: &mut impl Write) -> std::fmt::Result {
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

            if self.cst {
                self.format_cst(root.syntax(), 0, w)?;
            } else {
                self.format_root(root, w)?;
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

        for name in symbols.names() {
            let mut visited = IndexSet::new();
            self.format_symbol_tree(name, 0, &defined, &body_nodes, &mut visited, w)?;
        }
        Ok(())
    }

    fn format_symbol_tree(
        &self,
        name: &str,
        depth: usize,
        defined: &indexmap::IndexSet<&str>,
        body_nodes: &std::collections::HashMap<String, SyntaxNode>,
        visited: &mut indexmap::IndexSet<String>,
        w: &mut impl Write,
    ) -> std::fmt::Result {
        let prefix = indent(depth);

        if visited.contains(name) {
            writeln!(w, "{}{} (cycle)", prefix, name)?;
            return Ok(());
        }

        let is_broken = !defined.contains(name);
        if is_broken {
            writeln!(w, "{}{}?", prefix, name)?;
            return Ok(());
        }

        let arity = body_nodes
            .get(name)
            .map(|n| self.arity_glyph(n))
            .unwrap_or("");
        writeln!(w, "{}{}{}", prefix, name, arity)?;
        visited.insert(name.to_string());

        let analysis = self
            .query
            .analysis()
            .expect("symbol formatting only recurses after analysis exists");
        if let Some(body) = analysis.symbol_table.body(name) {
            let refs_set = crate::compiler::analyze::refs::refs::ref_names(body);
            let mut refs: Vec<_> = refs_set.iter().map(|s| s.as_str()).collect();
            refs.sort();
            for r in refs {
                self.format_symbol_tree(r, depth + 1, defined, body_nodes, visited, w)?;
            }
        }

        visited.shift_remove(name);
        Ok(())
    }

    fn format_cst(&self, node: &SyntaxNode, depth: usize, w: &mut impl Write) -> std::fmt::Result {
        let prefix = indent(depth);
        let arity = self.arity_glyph(node);
        let span = self.span_str(node.text_range());

        writeln!(w, "{}{:?}{}{}", prefix, node.kind(), arity, span)?;

        for child in node.children_with_tokens() {
            match child {
                NodeOrToken::Node(n) => self.format_cst(&n, depth + 1, w)?,
                NodeOrToken::Token(t) => {
                    if !self.trivia && t.kind().is_trivia() {
                        continue;
                    }
                    let child_prefix = indent(depth + 1);
                    let child_span = self.span_str(t.text_range());
                    writeln!(
                        w,
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

    fn format_root(&self, root: &ast::Root, w: &mut impl Write) -> std::fmt::Result {
        let arity = self.arity_glyph(root.syntax());
        let span = self.span_str(root.text_range());
        writeln!(w, "Root{}{}", arity, span)?;

        for def in root.defs() {
            self.format_def(&def, 1, w)?;
        }
        // Parser wraps all top-level patterns in Def nodes, so this should be empty
        assert!(
            root.patterns().next().is_none(),
            "printer: unexpected bare Pattern in Root (parser should wrap in Def)"
        );
        Ok(())
    }

    fn format_def(&self, def: &ast::Def, depth: usize, w: &mut impl Write) -> std::fmt::Result {
        let prefix = indent(depth);
        let arity = self.arity_glyph(def.syntax());
        let span = self.span_str(def.text_range());
        let name = def.name().map(|t| t.text().to_string());

        match name {
            Some(n) => writeln!(w, "{}Def{}{} {}", prefix, arity, span, n)?,
            None => writeln!(w, "{}Def{}{}", prefix, arity, span)?,
        }

        let Some(body) = def.body() else {
            return Ok(());
        };
        self.format_pattern(&body, depth + 1, w)
    }

    fn format_pattern(
        &self,
        pattern: &ast::Pattern,
        depth: usize,
        w: &mut impl Write,
    ) -> std::fmt::Result {
        let prefix = indent(depth);
        let arity = self.arity_glyph(pattern.syntax());
        let span = self.span_str(pattern.text_range());

        match pattern {
            ast::Pattern::NodePattern(n) => {
                if n.is_any() {
                    writeln!(w, "{}NamedNode{}{} (any)", prefix, arity, span)?;
                } else {
                    let node_kind = n.kind_token().map(|tok| tok.text().to_string());
                    match node_kind {
                        Some(ty) => writeln!(w, "{}NamedNode{}{} {}", prefix, arity, span, ty)?,
                        None => writeln!(w, "{}NamedNode{}{}", prefix, arity, span)?,
                    }
                }
                self.format_tree_children(n.syntax(), depth + 1, w)?;
            }
            ast::Pattern::Ref(r) => {
                let name = r.name().map(|t| t.text().to_string()).unwrap_or_default();
                writeln!(w, "{}Ref{}{} {}", prefix, arity, span, name)?;
            }
            ast::Pattern::TokenPattern(a) => {
                if a.is_any() {
                    writeln!(w, "{}AnonymousNode{}{} (any)", prefix, arity, span)?;
                } else {
                    let value = a.value().map(|t| t.text().to_string()).unwrap_or_default();
                    writeln!(w, "{}AnonymousNode{}{} \"{}\"", prefix, arity, span, value)?;
                }
            }
            ast::Pattern::Union(u) => {
                writeln!(w, "{}Union{}{}", prefix, arity, span)?;
                for branch in u.branches() {
                    self.format_branch(&branch, depth + 1, w)?;
                }
                for pattern in u.patterns() {
                    self.format_pattern(&pattern, depth + 1, w)?;
                }
            }
            ast::Pattern::Enum(e) => {
                writeln!(w, "{}Enum{}{}", prefix, arity, span)?;
                for branch in e.branches() {
                    self.format_branch(&branch, depth + 1, w)?;
                }
                for pattern in e.patterns() {
                    self.format_pattern(&pattern, depth + 1, w)?;
                }
            }
            ast::Pattern::SeqPattern(s) => {
                writeln!(w, "{}Seq{}{}", prefix, arity, span)?;
                self.format_tree_children(s.syntax(), depth + 1, w)?;
            }
            ast::Pattern::CapturedPattern(c) => {
                let name = c
                    .name()
                    .map(|t| t.text()[1..].to_string())
                    .unwrap_or_default();
                let type_ann = c
                    .type_annotation()
                    .and_then(|t| t.name())
                    .map(|t| t.text().to_string());
                match type_ann {
                    Some(ty) => writeln!(
                        w,
                        "{}CapturedPattern{}{} @{} :: {}",
                        prefix, arity, span, name, ty
                    )?,
                    None => writeln!(w, "{}CapturedPattern{}{} @{}", prefix, arity, span, name)?,
                }
                let Some(inner) = c.inner() else {
                    return Ok(());
                };
                self.format_pattern(&inner, depth + 1, w)?;
            }
            ast::Pattern::QuantifiedPattern(q) => {
                let op = q
                    .operator()
                    .map(|t| t.text().to_string())
                    .unwrap_or_default();
                writeln!(w, "{}QuantifiedPattern{}{} {}", prefix, arity, span, op)?;
                let Some(inner) = q.inner() else {
                    return Ok(());
                };
                self.format_pattern(&inner, depth + 1, w)?;
            }
            ast::Pattern::FieldPattern(f) => {
                let name = f.name().map(|t| t.text().to_string()).unwrap_or_default();
                writeln!(w, "{}FieldPattern{}{} {}:", prefix, arity, span, name)?;
                let Some(value) = f.value() else {
                    return Ok(());
                };
                self.format_pattern(&value, depth + 1, w)?;
            }
        }
        Ok(())
    }

    fn format_tree_children(
        &self,
        node: &SyntaxNode,
        depth: usize,
        w: &mut impl Write,
    ) -> std::fmt::Result {
        use crate::compiler::parse::SyntaxKind;
        for child in node.children() {
            if child.kind() == SyntaxKind::Anchor {
                self.mark_anchor(
                    &ast::Anchor::cast(child).expect("child is an Anchor by the matched kind"),
                    depth,
                    w,
                )?;
            } else if child.kind() == SyntaxKind::NegatedField {
                self.format_negated_field(
                    &ast::NegatedField::cast(child)
                        .expect("child is a NegatedField by the matched kind"),
                    depth,
                    w,
                )?;
            } else if let Some(pattern) = ast::Pattern::cast(child) {
                self.format_pattern(&pattern, depth, w)?;
            }
        }
        Ok(())
    }

    fn mark_anchor(
        &self,
        anchor: &ast::Anchor,
        depth: usize,
        w: &mut impl Write,
    ) -> std::fmt::Result {
        let spelling = if anchor.is_strict() { ".!" } else { "." };
        writeln!(w, "{}{}", indent(depth), spelling)
    }

    fn format_negated_field(
        &self,
        nf: &ast::NegatedField,
        depth: usize,
        w: &mut impl Write,
    ) -> std::fmt::Result {
        let prefix = indent(depth);
        let span = self.span_str(nf.text_range());
        let name = nf.name().map(|t| t.text().to_string()).unwrap_or_default();
        writeln!(w, "{}NegatedField{} -{}", prefix, span, name)
    }

    fn format_branch(
        &self,
        branch: &ast::Branch,
        depth: usize,
        w: &mut impl Write,
    ) -> std::fmt::Result {
        let prefix = indent(depth);
        let arity = self.arity_glyph(branch.syntax());
        let span = self.span_str(branch.text_range());
        let label = branch.label().map(|t| t.text().to_string());

        match label {
            Some(l) => writeln!(w, "{}Branch{}{} {}:", prefix, arity, span, l)?,
            None => writeln!(w, "{}Branch{}{}", prefix, arity, span)?,
        }

        let Some(body) = branch.body() else {
            return Ok(());
        };
        self.format_pattern(&body, depth + 1, w)
    }

    fn arity_glyph(&self, node: &SyntaxNode) -> &'static str {
        if !self.arities {
            return "";
        }
        match self.query.arity(node) {
            Some(Arity::One) => "¹",
            Some(Arity::Many) => "⁺",
            None => "ˣ",
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
    pub fn printer(&self) -> QueryPrinter<'_> {
        QueryPrinter::new(self)
    }
}
