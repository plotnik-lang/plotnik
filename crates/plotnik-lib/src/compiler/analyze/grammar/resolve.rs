use crate::compiler::analyze::Located;
use crate::compiler::analyze::visitor::{Visitor, walk};
use crate::compiler::diagnostics::diagnostics::{DiagnosticKind, Span};
use crate::compiler::diagnostics::source::SourceId;
use crate::compiler::parse::ast::token_src;
use crate::compiler::parse::ast::{self, NodePattern};
use crate::compiler::parse::cst::{SyntaxKind, SyntaxToken};
use crate::core::{NodeKind, NodeKindId};

use super::link::GrammarLinker;
use super::utils::find_similar;

impl<'a, 'q> GrammarLinker<'a, 'q> {
    pub(super) fn resolve_symbols(&mut self, source: SourceId, root: &ast::Root) {
        let mut resolver = GrammarSymbolResolver { linker: self };
        resolver.visit(&Located::new(source, root.clone()));
    }

    fn resolve_named_node(&mut self, located: &Located<NodePattern>) {
        let node = located.node();
        if node.is_any() {
            return;
        }
        let Some(type_token) = node.kind_token() else {
            return;
        };
        if matches!(
            type_token.kind(),
            SyntaxKind::KwError | SyntaxKind::KwMissing
        ) {
            return;
        }
        let type_name = type_token.text();
        let key = NodeKind::Named(token_src(&type_token, self.content(located.source())));
        if self.node_kind_ids.contains_key(&key) {
            return;
        }
        let resolved = self.grammar.resolve_named_node(type_name);
        self.node_kind_ids.insert(key, resolved);
        if let Some(id) = resolved {
            let sym = self.interner.intern(type_name);
            self.output.insert_node_kind_id(NodeKind::Named(sym), id);
        }
        if resolved.is_none() {
            let all_types = self.grammar.all_named_node_kinds();
            let max_dist = (type_name.len() / 3).clamp(2, 4);
            let suggestion = find_similar(type_name, &all_types, max_dist);

            let mut builder = self
                .diag
                .report(
                    DiagnosticKind::UnknownNodeKind,
                    located.span_of(type_token.text_range()),
                )
                .detail(type_name);

            if let Some(similar) = suggestion {
                builder = builder.fix(format!("did you mean `{}`?", similar), similar);
            }
            builder.emit();
        }
    }

    fn resolve_field_by_token(&mut self, source: SourceId, name_token: Option<SyntaxToken>) {
        let Some(name_token) = name_token else {
            return;
        };
        let field_name = name_token.text();
        if self.node_field_ids.contains_key(field_name) {
            return;
        }
        let resolved = self.grammar.resolve_field(field_name);
        self.node_field_ids
            .insert(token_src(&name_token, self.content(source)), resolved);
        if let Some(id) = resolved {
            let sym = self.interner.intern(field_name);
            self.output.insert_node_field_id(sym, id);
            return;
        }
        let all_fields = self.grammar.all_field_names();
        let max_dist = (field_name.len() / 3).clamp(2, 4);
        let suggestion = find_similar(field_name, &all_fields, max_dist);

        let mut builder = self
            .diag
            .report(
                DiagnosticKind::UnknownField,
                Span::new(source, name_token.text_range()),
            )
            .detail(field_name);

        if let Some(similar) = suggestion {
            builder = builder.fix(format!("did you mean `{}`?", similar), similar);
        }
        builder.emit();
    }

    /// Resolve a child/value `NodePattern` to its grammar id, mirroring node-context resolution
    /// but returning just the id. `None` for `(_)`, `ERROR`, `MISSING`, or an unresolved kind
    /// (the latter already reported by the resolution pass) — all of which carry no
    /// check signal and are conservatively accepted.
    pub(super) fn resolve_named_node_id(
        &self,
        located: &Located<NodePattern>,
    ) -> Option<NodeKindId> {
        let node = located.node();
        if node.is_any() {
            return None;
        }
        let type_token = node.kind_token()?;
        if matches!(
            type_token.kind(),
            SyntaxKind::KwError | SyntaxKind::KwMissing
        ) {
            return None;
        }
        let key = NodeKind::Named(token_src(&type_token, self.content(located.source())));
        self.node_kind_ids.get(&key).copied().flatten()
    }
}

struct GrammarSymbolResolver<'l, 'a, 'q> {
    linker: &'l mut GrammarLinker<'a, 'q>,
}

impl Visitor for GrammarSymbolResolver<'_, '_, '_> {
    fn visit(&mut self, root: &Located<ast::Root>) {
        walk(self, root);
    }

    fn visit_node_pattern(&mut self, node: &Located<ast::NodePattern>) {
        self.linker.resolve_named_node(node);

        for neg in node
            .node()
            .syntax()
            .children()
            .filter_map(ast::NegatedField::cast)
        {
            self.linker
                .resolve_field_by_token(node.source(), neg.name());
        }

        crate::compiler::analyze::visitor::walk_node_pattern(self, node);
    }

    fn visit_token_pattern(&mut self, node: &Located<ast::TokenPattern>) {
        let home = node.source();
        let token = node.node();
        if token.is_any() {
            return;
        }
        let Some(value_token) = token.value() else {
            return;
        };
        let value = value_token.text();
        let key = NodeKind::Anonymous(token_src(&value_token, self.linker.content(home)));
        if self.linker.node_kind_ids.contains_key(&key) {
            return;
        }

        let resolved = self.linker.grammar.resolve_anonymous_node(value);
        self.linker.node_kind_ids.insert(key, resolved);

        if let Some(id) = resolved {
            let sym = self.linker.interner.intern(value);
            self.linker
                .output
                .insert_node_kind_id(NodeKind::Anonymous(sym), id);
            return;
        }

        self.linker
            .diag
            .report(
                DiagnosticKind::UnknownNodeKind,
                node.span_of(value_token.text_range()),
            )
            .detail(value)
            .emit();
    }

    fn visit_field_pattern(&mut self, field: &Located<ast::FieldPattern>) {
        self.linker
            .resolve_field_by_token(field.source(), field.node().name());
        crate::compiler::analyze::visitor::walk_field_pattern(self, field);
    }
}
