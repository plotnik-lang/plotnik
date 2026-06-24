//! Single source of truth for "what value shape does a capture hold".
//!
//! Inference and emission both have to decide what a `@capture` produces.
//! Historically they re-derived this from overlapping but divergent syntactic
//! predicates, which is exactly what let the declared type and the emitted
//! effects disagree (issue #420). This classifier answers the question once,
//! reading the inner expression's already-inferred type, so both sides stay in
//! lockstep.

use crate::compiler::core::Interner;
use crate::compiler::parse::ast::{Pattern, is_empty_group};
use crate::compiler::core::dependency_analysis::DependencyAnalysis;
use crate::compiler::core::type_analysis::TypeAnalysis;
use crate::compiler::core::type_shape::{OutputFlow, QuantifierKind, TypeShape};

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
    /// Reads the inner's cached type info, so it is valid both during bottom-up
    /// inference (a capture's inner is inferred before the capture itself) and
    /// during emission (all type info is available).
    pub fn capture_mechanism(
        &self,
        inner: &Pattern,
        deps: &DependencyAnalysis,
        interner: &Interner,
    ) -> CaptureMechanism {
        // `field: x @cap` parses as `(field: x) @cap`; the field is only a navigation
        // constraint, so the value mechanism is that of `x`.
        let pattern = unwrap_field(inner);

        if let Pattern::QuantifiedPattern(quant) = &pattern {
            return match quant.quantifier_kind() {
                // `*` / `+` collect into an array regardless of element shape.
                Some(QuantifierKind::ZeroOrMore | QuantifierKind::OneOrMore) => {
                    CaptureMechanism::Array
                }
                // `?` only adds optionality; the value mechanism is the inner's.
                Some(QuantifierKind::Optional) => quant
                    .inner()
                    .map(|i| self.capture_mechanism(&i, deps, interner))
                    .unwrap_or(CaptureMechanism::Node),
                None => CaptureMechanism::Array,
            };
        }

        // A reference whose definition returns a structured type: the call site does
        // its own Call/Return (and Struct/EndStruct) scoping. A reference to a node/void
        // definition falls through to `Node` — its matched node is captured directly.
        if self.ref_returns_structured(&pattern, deps, interner) {
            return CaptureMechanism::Ref;
        }

        // An empty `{}` is an empty struct scope.
        if is_empty_group(&pattern) {
            return CaptureMechanism::StructScope;
        }

        // Everything else is decided by the inner's inferred data flow, so the type
        // and the emitted effects can't disagree.
        match self.pattern_result(&pattern).map(|info| &info.flow) {
            // Bubbling captures: a sequence/alternation wraps them in a fresh struct
            // scope; a named node instead captures its matched node and lets the
            // children bubble alongside as sibling fields.
            Some(OutputFlow::Fields(_)) => {
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
            Some(OutputFlow::Value(type_id)) if self.is_structured_output(*type_id) => {
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
        let Pattern::Ref(r) = pattern else {
            return false;
        };
        let Some(name) = r.name() else {
            return false;
        };
        let Some(def_id) = deps.def_id_for_name(interner, name.text()) else {
            return false;
        };

        // After inference the definition's registered output type is authoritative;
        // this is the path emission always takes.
        if let Some(output_type) = self.def_output(def_id) {
            return matches!(
                self.type_shape(output_type),
                Some(TypeShape::Struct(_) | TypeShape::Enum(_) | TypeShape::Array { .. })
            );
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

/// Look through a `field: x` wrapper to the value it constrains.
fn unwrap_field(pattern: &Pattern) -> Pattern {
    match pattern {
        Pattern::FieldPattern(f) => f.value().unwrap_or_else(|| pattern.clone()),
        other => other.clone(),
    }
}
