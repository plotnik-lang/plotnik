//! Single source of truth for "what value shape does a capture hold".
//!
//! Inference and emission both have to decide what a `@capture` produces.
//! Historically they re-derived this from overlapping but divergent syntactic
//! predicates, which is exactly what let the declared type and the emitted
//! effects disagree (issue #420). This classifier answers the question once,
//! reading the inner expression's already-inferred type, so both sides stay in
//! lockstep.

use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::analyze::types::type_analysis::{InProgressTypeAnalysis, TypeAnalysis};
use crate::compiler::analyze::types::type_shape::{OutputFlow, QuantifierKind, TypeShape};
use crate::compiler::parse::ast::{Pattern, is_empty_group};
use crate::core::Interner;

/// How a captured value is produced — the bridge between the inferred type and
/// the emitted effects.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaptureMechanism {
    /// The matched tree-sitter node itself (`Node` effect). If the inner has
    /// bubbling child captures, they set into the enclosing scope as siblings.
    Node,
    /// A fresh struct built from the inner sequence/alternation's bubbling
    /// captures (`Struct … EndStruct`).
    StructScope,
    /// A reference whose definition returns a structured type. The call site wraps
    /// the `Call`/`Return` (with an `Struct`/`EndStruct` scope when the definition
    /// returns a struct) and consumes the result — the capture emits no `Node`.
    Ref,
    /// The inner expression itself leaves the captured value pending — an enum
    /// alternation (`Enum … EndEnum`) or a named node forwarding a single
    /// structured output child. Emit the inner, then a trailing `Set`; the capture
    /// contributes no `Node` and no wrapper.
    SetAfter,
    /// An array collected by `*` or `+` (`Arr … Push … EndArr`).
    Array,
}

/// Capture value-mechanism classification, exposed as accessors on the analyzed
/// [`TypeAnalysis`] artifact so inference and emission read one implementation.
impl TypeAnalysis {
    /// Classify the value mechanism of a captured inner expression.
    ///
    /// Reads the inner's admitted type info. Missing pattern or definition data is a
    /// compiler bug after [`TypeAnalysisBuilder::finish`](super::type_analysis::TypeAnalysisBuilder::finish),
    /// so this accessor is loud rather than recovering to `Node`.
    pub fn capture_mechanism(
        &self,
        inner: &Pattern,
        deps: &DependencyAnalysis,
        interner: &Interner,
    ) -> CaptureMechanism {
        self.capture_mechanism_with_mode(inner, deps, interner, CaptureLookupMode::Admitted)
    }

    fn capture_mechanism_with_mode(
        &self,
        inner: &Pattern,
        deps: &DependencyAnalysis,
        interner: &Interner,
        mode: CaptureLookupMode,
    ) -> CaptureMechanism {
        // `field: x @cap` parses as `(field: x) @cap`; the field is only a navigation
        // constraint, so the value mechanism is that of `x`.
        let pattern = unwrap_field(inner);

        if let Pattern::QuantifiedPattern(quant) = &pattern {
            let kind = match mode {
                CaptureLookupMode::Admitted => quant
                    .quantifier_kind()
                    .expect("admitted quantified pattern must have a quantifier operator"),
                CaptureLookupMode::InProgress => quant
                    .quantifier_kind()
                    .unwrap_or(QuantifierKind::ZeroOrMore),
            };
            return match kind {
                // `*` / `+` collect into an array regardless of element shape.
                QuantifierKind::ZeroOrMore | QuantifierKind::OneOrMore => CaptureMechanism::Array,
                // `?` only adds optionality; the value mechanism is the inner's.
                QuantifierKind::Optional => {
                    let Some(inner) = quant.inner() else {
                        return match mode {
                            CaptureLookupMode::Admitted => {
                                panic!("admitted optional quantifier must have an inner pattern")
                            }
                            CaptureLookupMode::InProgress => CaptureMechanism::Node,
                        };
                    };
                    self.capture_mechanism_with_mode(&inner, deps, interner, mode)
                }
            };
        }

        // A reference whose definition returns a structured type: the call site does
        // its own Call/Return (and Struct/EndStruct) scoping. A reference to a node/void
        // definition falls through to `Node` — its matched node is captured directly.
        if self.ref_returns_structured_with_mode(&pattern, deps, interner, mode) {
            return CaptureMechanism::Ref;
        }

        // An empty `{}` is an empty struct scope.
        if is_empty_group(&pattern) {
            return CaptureMechanism::StructScope;
        }

        // Everything else is decided by the inner's inferred data flow, so the type
        // and the emitted effects can't disagree.
        let flow = match self.pattern_result(&pattern).map(|info| &info.flow) {
            Some(flow) => flow,
            None => {
                return match mode {
                    CaptureLookupMode::Admitted => {
                        panic!("admitted captured pattern must have a pattern result")
                    }
                    CaptureLookupMode::InProgress => CaptureMechanism::Node,
                };
            }
        };

        match flow {
            // Bubbling captures: a sequence/alternation wraps them in a fresh struct
            // scope; a named node instead captures its matched node and lets the
            // children bubble alongside as sibling fields.
            OutputFlow::Fields(_) => {
                // Only a union alternation flows `Fields` here; an enum flows `Value`
                // and is handled below, so it must not appear in this arm.
                if matches!(pattern, Pattern::SeqPattern(_) | Pattern::Union(_)) {
                    CaptureMechanism::StructScope
                } else {
                    CaptureMechanism::Node
                }
            }
            // A structured scalar is left pending by the inner itself — an enum
            // alternation (`Enum`/`EndEnum`) or a named node forwarding a structured
            // output child.
            OutputFlow::Value(type_id) if self.is_structured_output(*type_id) => {
                CaptureMechanism::SetAfter
            }
            // Void, or a plain scalar node: the matched node is captured directly.
            _ => CaptureMechanism::Node,
        }
    }

    /// Whether `pattern` is a reference to a definition that returns a structured type.
    pub fn ref_returns_structured(
        &self,
        pattern: &Pattern,
        deps: &DependencyAnalysis,
        interner: &Interner,
    ) -> bool {
        self.ref_returns_structured_with_mode(pattern, deps, interner, CaptureLookupMode::Admitted)
    }

    fn ref_returns_structured_with_mode(
        &self,
        pattern: &Pattern,
        deps: &DependencyAnalysis,
        interner: &Interner,
        mode: CaptureLookupMode,
    ) -> bool {
        let Pattern::Ref(r) = pattern else {
            return false;
        };
        let name = match r.name() {
            Some(name) => name,
            None => {
                return match mode {
                    CaptureLookupMode::Admitted => {
                        panic!("admitted reference pattern must have a name")
                    }
                    CaptureLookupMode::InProgress => false,
                };
            }
        };
        let def_id = match deps.def_id_for_name(interner, name.text()) {
            Some(def_id) => def_id,
            None => {
                return match mode {
                    CaptureLookupMode::Admitted => {
                        panic!("admitted reference must resolve to a definition")
                    }
                    CaptureLookupMode::InProgress => false,
                };
            }
        };

        // After inference the definition's registered output type is authoritative;
        // this is the path emission always takes.
        if let Some(output_type) = self.def_output(def_id) {
            return matches!(
                self.type_shape(output_type),
                Some(TypeShape::Struct(_) | TypeShape::Enum(_) | TypeShape::Array { .. })
            );
        }

        if matches!(mode, CaptureLookupMode::Admitted) {
            panic!("admitted reference target must have an output type");
        }

        // During inference a leaf definition may not be registered yet — the visitor
        // walks every definition in a file before any output type is set. Fall back to
        // the reference's own transparently-inferred flow: a structured result either
        // bubbles its fields (struct) or is a structured scalar (enum/array).
        match self.pattern_result(pattern).map(|info| &info.flow) {
            Some(OutputFlow::Fields(_)) => true,
            Some(OutputFlow::Value(t)) => self.is_structured_output(*t),
            _ => false,
        }
    }
}

impl InProgressTypeAnalysis<'_> {
    /// Classification used while inference is still constructing [`TypeAnalysis`].
    ///
    /// In-progress inference can legitimately ask before every definition output has
    /// been memoized, so this view keeps the conservative fallbacks off the frozen
    /// artifact's API.
    pub(crate) fn capture_mechanism(
        &self,
        inner: &Pattern,
        deps: &DependencyAnalysis,
        interner: &Interner,
    ) -> CaptureMechanism {
        self.analysis
            .capture_mechanism_with_mode(inner, deps, interner, CaptureLookupMode::InProgress)
    }
}

#[derive(Clone, Copy)]
enum CaptureLookupMode {
    Admitted,
    InProgress,
}

/// Look through a `field: x` wrapper to the value it constrains.
fn unwrap_field(pattern: &Pattern) -> Pattern {
    match pattern {
        Pattern::FieldPattern(f) => f.value().unwrap_or_else(|| pattern.clone()),
        other => other.clone(),
    }
}
