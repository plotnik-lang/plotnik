use std::collections::HashMap;

use crate::compiler::analyze::refs::DefinitionGraph;
use crate::compiler::analyze::types::type_analysis::{TypeAnalysis, TypeAnalysisView};
use crate::compiler::analyze::types::type_shape::{
    PatternFlow, PatternShape, QuantifierKind, TypeShape,
};
use crate::compiler::parse::ast::{Pattern, is_empty_group};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaptureKind {
    Node,
    Record,
    Ref,
    PendingValue,
    List,
}

impl TypeAnalysis {
    fn classify<'a>(
        &'a self,
        inner: &Pattern,
        definitions: &DefinitionGraph,
        mode: CaptureLookupMode<'a>,
    ) -> CaptureKind {
        let pattern = unwrap_field(inner);

        if let Pattern::QuantifiedPattern(quant) = &pattern {
            let kind = mode.quantifier_kind(quant);
            return match kind {
                QuantifierKind::ZeroOrMore | QuantifierKind::OneOrMore => CaptureKind::List,
                QuantifierKind::Optional => {
                    let Some(inner) = quant.inner() else {
                        return mode.recover(
                            "admitted optional quantifier must have an inner pattern",
                            CaptureKind::Node,
                        );
                    };
                    let kind = self.classify(&inner, definitions, mode);
                    let inner_flow = mode.pattern_flow(self, &unwrap_field(&inner));
                    if kind == CaptureKind::Node
                        && matches!(inner_flow, Some(PatternFlow::Fields(_)))
                    {
                        return CaptureKind::Record;
                    }
                    kind
                }
            };
        }

        if self.ref_structured(&pattern, definitions, mode) {
            return CaptureKind::Ref;
        }

        if is_empty_group(&pattern) {
            return CaptureKind::Record;
        }

        let Some(flow) = mode.pattern_flow(self, &pattern) else {
            return CaptureKind::Node;
        };

        match flow {
            PatternFlow::Fields(_) => {
                if matches!(pattern, Pattern::SeqPattern(_) | Pattern::Alternation(_)) {
                    CaptureKind::Record
                } else {
                    CaptureKind::Node
                }
            }
            PatternFlow::Value(type_id) if self.is_structured_output(*type_id) => {
                CaptureKind::PendingValue
            }
            _ => CaptureKind::Node,
        }
    }

    pub fn ref_returns_structured(&self, pattern: &Pattern, definitions: &DefinitionGraph) -> bool {
        self.ref_structured(pattern, definitions, CaptureLookupMode::Admitted)
    }

    fn ref_structured<'a>(
        &'a self,
        pattern: &Pattern,
        definitions: &DefinitionGraph,
        mode: CaptureLookupMode<'a>,
    ) -> bool {
        let Pattern::DefRef(r) = pattern else {
            return false;
        };
        let Some(def_id) = definitions.reference_target(r) else {
            return mode.recover("admitted reference must resolve to a definition", false);
        };

        if mode.is_admitted() {
            let Some(result_type) = self.expect_def_output(def_id).value() else {
                return false;
            };
            return matches!(
                self.expect_type_shape(result_type),
                TypeShape::Record(_)
                    | TypeShape::Variant(_)
                    | TypeShape::List { .. }
                    | TypeShape::Option(_)
            );
        }

        if let Some(result_type) = self.def_output(def_id).and_then(|output| output.value()) {
            return matches!(
                self.type_shape(result_type),
                Some(
                    TypeShape::Record(_)
                        | TypeShape::Variant(_)
                        | TypeShape::List { .. }
                        | TypeShape::Option(_)
                )
            );
        }

        match mode.pattern_flow(self, pattern) {
            Some(PatternFlow::Value(t)) => self.is_structured_output(*t),
            _ => false,
        }
    }
}

impl TypeAnalysisView<'_> {
    pub(crate) fn capture_kind(
        &self,
        inner: &Pattern,
        definitions: &DefinitionGraph,
    ) -> CaptureKind {
        self.analysis.classify(
            inner,
            definitions,
            CaptureLookupMode::InProgress(self.pattern_shapes),
        )
    }
}

#[derive(Clone, Copy)]
enum CaptureLookupMode<'a> {
    Admitted,
    InProgress(&'a HashMap<Pattern, PatternShape>),
}

impl CaptureLookupMode<'_> {
    fn is_admitted(self) -> bool {
        matches!(self, Self::Admitted)
    }

    fn recover<T>(self, message: &str, fallback: T) -> T {
        match self {
            Self::Admitted => panic!("{message}"),
            Self::InProgress(_) => fallback,
        }
    }

    fn quantifier_kind(
        self,
        quant: &crate::compiler::parse::ast::QuantifiedPattern,
    ) -> QuantifierKind {
        match self {
            Self::Admitted => quant
                .quantifier_kind()
                .expect("admitted quantified pattern must have a quantifier operator"),
            Self::InProgress(_) => quant
                .quantifier_kind()
                .unwrap_or(QuantifierKind::ZeroOrMore),
        }
    }
}

impl<'a> CaptureLookupMode<'a> {
    fn pattern_flow(
        self,
        analysis: &'a TypeAnalysis,
        pattern: &Pattern,
    ) -> Option<&'a PatternFlow> {
        match self {
            Self::Admitted => Some(analysis.expect_pattern_flow(pattern)),
            Self::InProgress(pattern_shapes) => {
                pattern_shapes.get(pattern).map(|shape| &shape.flow)
            }
        }
    }
}

fn unwrap_field(pattern: &Pattern) -> Pattern {
    match pattern {
        Pattern::FieldPattern(f) => f.value().unwrap_or_else(|| pattern.clone()),
        other => other.clone(),
    }
}
