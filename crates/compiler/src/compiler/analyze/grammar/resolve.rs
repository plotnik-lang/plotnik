use crate::compiler::analyze::Located;
use crate::compiler::analyze::visitor::Visitor;
use crate::compiler::diagnostics::report::{DiagnosticKind, Span};
use crate::compiler::diagnostics::source::SourceId;
use crate::compiler::parse::ast::token_src;
use crate::compiler::parse::ast::{self, MissingArg, NamedNodePattern};
use crate::compiler::parse::cst::{SyntaxKind, SyntaxToken};
use crate::compiler::parse::strings::unescape;
use crate::core::{NodeKind, NodeKindId};

use super::bind::GrammarBinder;
use crate::core::utils::find_similar;

impl<'a, 'q> GrammarBinder<'a, 'q> {
    pub(super) fn resolve_symbols(&mut self, pattern: &Located<ast::Pattern>) {
        let mut resolver = GrammarSymbolResolver { binder: self };
        resolver.visit_pattern(pattern);
    }

    fn resolve_named_node(&mut self, located: &Located<NamedNodePattern>) {
        let node = located.node();
        if node.is_any() {
            return;
        }

        let type_token = node.kind_token().expect("validated node has a kind");
        if type_token.kind() == SyntaxKind::KwError {
            return;
        }

        if type_token.kind() == SyntaxKind::KwMissing {
            self.resolve_missing_node(located);
            return;
        }

        let name = node.selector_name();
        if self.named_selectors.contains_key(&name) {
            return;
        }

        let resolved = self.grammar.resolve_named_selector(&name);
        self.named_selectors.insert(name.clone(), resolved.clone());
        if let Some(ids) = resolved {
            for &id in &ids {
                let concrete = self.grammar.node_kind(id).expect("bound kind has a name");
                let sym = self.interner.intern(concrete);
                self.output.insert_node_kind_id(NodeKind::Named(sym), id);
            }
            self.output.insert_named_selector(name, ids);
            return;
        }

        if let Some((group, member)) = name.split_once("__")
            && self.grammar.resolve_named_node(&name).is_some()
        {
            let selector = format!("{group}/{member}");
            self.diag
                .report(
                    DiagnosticKind::InvalidNodeSelector,
                    located.span_of(node.selector_range()),
                )
                .detail(&name)
                .fix(format!("write `{selector}`"), selector)
                .emit();
            return;
        }

        self.report_unknown_named_kind(&name, located.span_of(node.selector_range()));
    }

    fn resolve_missing_node(&mut self, located: &Located<NamedNodePattern>) {
        let source = located.source();
        let (token, id) = match located.node().missing_arg() {
            None => return,
            Some(MissingArg::Named(token)) => {
                let id = self.bind_named_kind(source, &token);
                (token, id)
            }
            Some(MissingArg::Anonymous(token)) => {
                let id = self.bind_anonymous_kind(source, &token);
                (token, id)
            }
        };
        let Some(id) = id else {
            return;
        };
        if self.grammar.can_be_token(id) {
            return;
        }

        self.diag
            .report(
                DiagnosticKind::MissingKindNotToken,
                Span::new(source, token.text_range()),
            )
            .detail(token.text())
            .emit();
    }

    fn bind_named_kind(&mut self, source: SourceId, token: &SyntaxToken) -> Option<NodeKindId> {
        let type_name = token.text();
        let key = NodeKind::Named(token_src(token, self.content(source)));
        if let Some(&cached) = self.node_kind_ids.get(&key) {
            return cached;
        }

        let resolved = self
            .grammar
            .resolve_named_selector(type_name)
            .and(self.grammar.resolve_named_node(type_name));
        self.node_kind_ids.insert(key, resolved);
        if let Some(id) = resolved {
            let sym = self.interner.intern(type_name);
            self.output.insert_node_kind_id(NodeKind::Named(sym), id);
            return Some(id);
        }

        self.report_unknown_named_kind(type_name, Span::new(source, token.text_range()));
        None
    }

    fn report_unknown_named_kind(&mut self, name: &str, span: Span) {
        let all_types = self
            .grammar
            .all_named_node_kinds()
            .into_iter()
            .filter(|name| !name.contains("__"))
            .collect::<Vec<_>>();
        let suggestion = find_similar(name, &all_types);

        let mut builder = self
            .diag
            .report(DiagnosticKind::UnknownNodeKind, span)
            .detail(name);
        if let Some(similar) = suggestion {
            builder = builder.fix(format!("replace with `{similar}`"), similar);
        }

        builder.emit();
    }

    fn bind_anonymous_kind(
        &mut self,
        source: SourceId,
        value_token: &SyntaxToken,
    ) -> Option<NodeKindId> {
        let key = NodeKind::Anonymous(token_src(value_token, self.content(source)));
        if let Some(&cached) = self.node_kind_ids.get(&key) {
            return cached;
        }

        let value = unescape(value_token.text()).0;
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

    fn resolve_field_by_token(
        &mut self,
        source: SourceId,
        name_token: SyntaxToken,
    ) -> Option<SyntaxToken> {
        let field_name = name_token.text();
        if self.node_field_ids.contains_key(field_name) {
            return None;
        }

        let resolved = self.grammar.resolve_field(field_name);
        self.node_field_ids
            .insert(token_src(&name_token, self.content(source)), resolved);
        if let Some(id) = resolved {
            let sym = self.interner.intern(field_name);
            self.output.insert_node_field_id(sym, id);
            return None;
        }

        Some(name_token)
    }

    fn report_unknown_field(
        &mut self,
        source: SourceId,
        token: &SyntaxToken,
        suggestion: Option<&str>,
    ) {
        let name = token.text();
        let mut builder = self
            .diag
            .report(
                DiagnosticKind::UnknownGrammarField,
                Span::new(source, token.text_range()),
            )
            .detail(name);
        if let Some(similar) = suggestion {
            builder = builder.hint(format!("did you mean the field `{similar}`?"));
        }
        builder.emit();
    }

    pub(super) fn resolve_named_node_id(
        &self,
        located: &Located<NamedNodePattern>,
    ) -> Option<NodeKindId> {
        let ids = self.resolve_named_node_ids(located)?;
        (ids.len() == 1).then(|| ids[0])
    }

    pub(super) fn resolve_named_node_ids(
        &self,
        located: &Located<NamedNodePattern>,
    ) -> Option<&[NodeKindId]> {
        self.named_selectors
            .get(&located.node().selector_name())?
            .as_deref()
    }
}

struct GrammarSymbolResolver<'l, 'a, 'q> {
    binder: &'l mut GrammarBinder<'a, 'q>,
}

impl Visitor for GrammarSymbolResolver<'_, '_, '_> {
    fn visit_named_node_pattern(&mut self, node: &Located<ast::NamedNodePattern>) {
        self.binder.resolve_named_node(node);

        for neg in node
            .node()
            .syntax()
            .children()
            .filter_map(ast::NegatedField::cast)
        {
            if let Some(token) = self.binder.resolve_field_by_token(
                node.source(),
                neg.name().expect("validated field has a name"),
            ) {
                let grammar = self.binder.grammar;
                let suggestion = self.binder.resolve_named_node_id(node).and_then(|kind| {
                    find_similar(token.text(), &grammar.fields_for_node_kind(kind))
                });
                self.binder
                    .report_unknown_field(node.source(), &token, suggestion);
            }
        }

        crate::compiler::analyze::visitor::walk_named_node_pattern(self, node);
    }

    fn visit_anonymous_node_pattern(&mut self, node: &Located<ast::AnonymousNodePattern>) {
        let value_token = node
            .node()
            .value()
            .expect("validated anonymous node has a value");
        self.binder.bind_anonymous_kind(node.source(), &value_token);
    }

    fn visit_field_pattern(&mut self, field: &Located<ast::FieldPattern>) {
        let unknown = self.binder.resolve_field_by_token(
            field.source(),
            field.node().name().expect("validated field has a name"),
        );
        crate::compiler::analyze::visitor::walk_field_pattern(self, field);
        if let Some(token) = unknown {
            let grammar = self.binder.grammar;
            let parent = field
                .node()
                .syntax()
                .ancestors()
                .find_map(NamedNodePattern::cast);
            let kind =
                parent.and_then(|parent| self.binder.resolve_named_node_id(&field.wrap(parent)));
            let candidates = kind
                .map(|kind| grammar.fields_for_node_kind(kind))
                .unwrap_or_else(|| grammar.all_field_names());
            let suggestion = find_similar(token.text(), &candidates);
            self.binder
                .report_unknown_field(field.source(), &token, suggestion);
        }
    }
}
