use crate::compiler::analyze::Located;
use crate::compiler::analyze::visitor::{Visitor, walk};
use crate::compiler::diagnostics::report::{DiagnosticKind, Span};
use crate::compiler::diagnostics::source::SourceId;
use crate::compiler::parse::ast::token_src;
use crate::compiler::parse::ast::{self, MissingArg, NamedNodePattern};
use crate::compiler::parse::cst::{SyntaxKind, SyntaxToken};
use crate::compiler::parse::strings::unescape;
use crate::core::{NodeKind, NodeKindId};

use super::bind::GrammarBinder;
use super::utils::find_similar;

impl<'a, 'q> GrammarBinder<'a, 'q> {
    pub(super) fn resolve_symbols(&mut self, source: SourceId, root: &ast::Root) {
        let mut resolver = GrammarSymbolResolver { binder: self };
        resolver.visit(&Located::new(source, root.clone()));
    }

    fn resolve_named_node(&mut self, located: &Located<NamedNodePattern>) {
        let node = located.node();
        if node.is_any() {
            return;
        }
        let Some(type_token) = node.kind_token() else {
            return;
        };
        if type_token.kind() == SyntaxKind::KwError {
            return;
        }
        if type_token.kind() == SyntaxKind::KwMissing {
            self.resolve_missing_node(located);
            return;
        }
        self.bind_named_kind(located.source(), &type_token);
    }

    /// Validate the optional kind argument of `(MISSING …)`. The argument resolves
    /// like a normal named/anonymous kind (unknown → `UnknownNodeKind`); a named
    /// argument must additionally be a leaf token, since tree-sitter's error
    /// recovery only ever inserts tokens as missing nodes — `(MISSING binary_expression)`
    /// names a kind with children, can never match, and is rejected here.
    fn resolve_missing_node(&mut self, located: &Located<NamedNodePattern>) {
        let source = located.source();
        match located.node().missing_arg() {
            None => {}
            Some(MissingArg::Named(id_tok)) => {
                let Some(id) = self.bind_named_kind(source, &id_tok) else {
                    return;
                };
                if self.grammar.has_declared_child_structure(id) {
                    self.diag
                        .report(
                            DiagnosticKind::MissingKindNotToken,
                            Span::new(source, id_tok.text_range()),
                        )
                        .detail(id_tok.text())
                        .emit();
                }
            }
            Some(MissingArg::Anonymous(content)) => {
                // Anonymous kinds are literal tokens by definition, so the leaf check
                // the named arm performs is always satisfied here.
                self.bind_anonymous_kind(source, &content);
            }
        }
    }

    /// Resolve and bind a named node kind named by `token`, recording the id for
    /// lowering and reporting `UnknownNodeKind` (with a suggestion) when the grammar
    /// has no such kind. Returns the resolved id, or `None` if unknown. Idempotent:
    /// a repeated name hits the resolution cache and neither re-binds nor re-reports.
    fn bind_named_kind(&mut self, source: SourceId, token: &SyntaxToken) -> Option<NodeKindId> {
        let type_name = token.text();
        let key = NodeKind::Named(token_src(token, self.content(source)));
        if let Some(&cached) = self.node_kind_ids.get(&key) {
            return cached;
        }
        let resolved = self.grammar.resolve_named_node(type_name);
        self.node_kind_ids.insert(key, resolved);
        if let Some(id) = resolved {
            let sym = self.interner.intern(type_name);
            self.output.insert_node_kind_id(NodeKind::Named(sym), id);
            return Some(id);
        }
        let all_types = self.grammar.all_named_node_kinds();
        let suggestion = find_similar(type_name, &all_types);
        let mut builder = self
            .diag
            .report(
                DiagnosticKind::UnknownNodeKind,
                Span::new(source, token.text_range()),
            )
            .detail(type_name);
        if let Some(similar) = suggestion {
            builder = builder.fix(format!("did you mean `{}`?", similar), similar);
        }
        builder.emit();
        None
    }

    /// Resolve and bind an anonymous (literal-token) kind from `value_token`,
    /// reporting `UnknownNodeKind` when the grammar has no such token. Returns the
    /// resolved id, or `None` if unknown. Caches identically to [`Self::bind_named_kind`].
    fn bind_anonymous_kind(
        &mut self,
        source: SourceId,
        value_token: &SyntaxToken,
    ) -> Option<NodeKindId> {
        let value = unescape(value_token.text()).0;
        let key = NodeKind::Anonymous(token_src(value_token, self.content(source)));
        if let Some(&cached) = self.node_kind_ids.get(&key) {
            return cached;
        }
        let resolved = self.grammar.resolve_anonymous_node(&value);
        self.node_kind_ids.insert(key, resolved);
        if let Some(id) = resolved {
            let sym = self.interner.intern(&value);
            self.output
                .insert_node_kind_id(NodeKind::Anonymous(sym), id);
            return Some(id);
        }
        self.diag
            .report(
                DiagnosticKind::UnknownNodeKind,
                Span::new(source, value_token.text_range()),
            )
            .detail(value.into_owned())
            .emit();
        None
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
        let suggestion = find_similar(field_name, &all_fields);

        let mut builder = self
            .diag
            .report(
                DiagnosticKind::UnknownGrammarField,
                Span::new(source, name_token.text_range()),
            )
            .detail(field_name);

        if let Some(similar) = suggestion {
            builder = builder.fix(format!("did you mean `{}`?", similar), similar);
        }
        builder.emit();
    }

    /// Resolve a child/value named-node pattern to its grammar id, mirroring node-context resolution
    /// but returning just the id. `None` for `(_)`, `ERROR`, `MISSING`, or an unresolved kind
    /// (the latter already reported by the resolution pass) — all of which carry no
    /// check signal and are conservatively accepted.
    pub(super) fn resolve_named_node_id(
        &self,
        located: &Located<NamedNodePattern>,
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
    binder: &'l mut GrammarBinder<'a, 'q>,
}

impl Visitor for GrammarSymbolResolver<'_, '_, '_> {
    fn visit(&mut self, root: &Located<ast::Root>) {
        walk(self, root);
    }

    fn visit_named_node_pattern(&mut self, node: &Located<ast::NamedNodePattern>) {
        self.binder.resolve_named_node(node);

        for neg in node
            .node()
            .syntax()
            .children()
            .filter_map(ast::NegatedField::cast)
        {
            self.binder
                .resolve_field_by_token(node.source(), neg.name());
        }

        crate::compiler::analyze::visitor::walk_named_node_pattern(self, node);
    }

    fn visit_anonymous_node_pattern(&mut self, node: &Located<ast::AnonymousNodePattern>) {
        let Some(value_token) = node.node().value() else {
            return;
        };
        self.binder.bind_anonymous_kind(node.source(), &value_token);
    }

    fn visit_field_pattern(&mut self, field: &Located<ast::FieldPattern>) {
        self.binder
            .resolve_field_by_token(field.source(), field.node().name());
        crate::compiler::analyze::visitor::walk_field_pattern(self, field);
    }
}
