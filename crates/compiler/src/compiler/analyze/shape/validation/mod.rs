use indexmap::IndexMap;

use crate::compiler::analyze::Located;
use crate::compiler::diagnostics::report::{DiagnosticKind, Diagnostics};
use crate::compiler::diagnostics::source::SourceId;
use crate::compiler::diagnostics::source::SourceMap;
use crate::compiler::parse::SyntaxNode;
use crate::compiler::parse::ast::{Anchor, Expression, NegatedField, Pattern, Root};

pub mod alternative_labels;
pub mod anchors;
pub mod empty_constructs;
pub mod predicates;
pub mod strings;

pub struct ValidationInput<'q, 'd> {
    pub source_id: SourceId,
    pub ast: &'q SyntaxNode,
    pub diag: &'d mut Diagnostics,
}

pub struct PredicateInput<'q, 'd> {
    pub source_id: SourceId,
    pub ast: &'q SyntaxNode,
    pub source_content: &'q str,
    pub diag: &'d mut Diagnostics,
}

pub struct ShapeValidationInput<'q, 'd> {
    pub source_map: &'q SourceMap,
    pub ast_map: &'q IndexMap<SourceId, Root>,
    pub expression: Option<&'q Located<Expression>>,
    pub diag: &'d mut Diagnostics,
}

pub(crate) struct ValidatedAst<'q> {
    source_map: &'q SourceMap,
    ast_map: &'q IndexMap<SourceId, Root>,
    expression: Option<Located<Pattern>>,
}

impl<'q> ValidatedAst<'q> {
    fn new(
        source_map: &'q SourceMap,
        ast_map: &'q IndexMap<SourceId, Root>,
        expression: Option<&Located<Expression>>,
    ) -> Self {
        for &source in ast_map.keys() {
            source_map.source(source);
        }

        Self {
            source_map,
            ast_map,
            expression: expression.map(|root| {
                root.wrap(root.node().body().expect("validated expression has a body"))
            }),
        }
    }

    pub(crate) fn source_map(&self) -> &'q SourceMap {
        self.source_map
    }

    pub(crate) fn ast_map(&self) -> &'q IndexMap<SourceId, Root> {
        self.ast_map
    }

    pub(crate) fn expression(&self) -> Option<&Located<Pattern>> {
        self.expression.as_ref()
    }
}

pub fn validate_ast<'q>(input: ShapeValidationInput<'q, '_>) -> Option<ValidatedAst<'q>> {
    if !input.diag.has_errors()
        && let Some(expression) = input.expression
        && expression.node().body().is_none()
    {
        let syntax = expression.node().syntax();
        if let Some(anchor) = syntax.children().find_map(Anchor::cast) {
            input
                .diag
                .report(
                    DiagnosticKind::AnchorWithoutContext,
                    expression.span_of(anchor.text_range()),
                )
                .emit();
        } else {
            let field = syntax
                .children()
                .find_map(NegatedField::cast)
                .expect("a parsed expression without a pattern is an anchor or negated field");
            input
                .diag
                .report(
                    DiagnosticKind::NoEntrypoints,
                    expression.span_of(field.text_range()),
                )
                .detail("a negated field needs an enclosing node")
                .emit();
        }
    }

    let modules = input.ast_map.iter().map(|(&id, root)| (id, root.syntax()));
    let expression = input
        .expression
        .map(|root| (root.source(), root.node().syntax()));
    for (id, ast) in modules.chain(expression) {
        let source = input.source_map.source(id);
        validate_alternative_labels(ValidationInput {
            source_id: source.id,
            ast,
            diag: &mut *input.diag,
        });
        validate_empty_constructs(ValidationInput {
            source_id: source.id,
            ast,
            diag: &mut *input.diag,
        });
        validate_predicates(PredicateInput {
            source_id: source.id,
            ast,
            source_content: source.content,
            diag: &mut *input.diag,
        });
        strings::validate_strings(ValidationInput {
            source_id: source.id,
            ast,
            diag: &mut *input.diag,
        });
    }

    (!input.diag.has_errors())
        .then(|| ValidatedAst::new(input.source_map, input.ast_map, input.expression))
}

pub use alternative_labels::validate_alternative_labels;
pub(crate) use anchors::{AnchorValidationInput, validate_anchors};
pub use empty_constructs::validate_empty_constructs;
pub use predicates::validate_predicates;
