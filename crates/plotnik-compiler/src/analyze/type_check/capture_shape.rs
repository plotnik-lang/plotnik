//! Single source of truth for "what value shape does a capture hold".
//!
//! Inference (`infer.rs`) and emission (`compile/`) both have to decide what a
//! `@capture` produces. Historically they re-derived this from overlapping but
//! divergent syntactic predicates, which is exactly what let the declared type
//! and the emitted effects disagree (issue #420). This classifier answers the
//! question once, reading the inner expression's already-inferred type, so both
//! sides stay in lockstep.

use plotnik_core::Interner;

use crate::parser::{Pattern, QuantifiedPattern, SyntaxKind, is_empty_group};

use super::context::TypeContext;
use super::types::{QuantifierKind, TYPE_NODE, TypeFlow, TypeId, TypeShape};

/// How a captured value is produced — the bridge between the inferred type and
/// the emitted effects.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaptureKind {
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

/// Classify the value mechanism of a captured inner expression.
///
/// Reads the inner's cached type info, so it is valid both during bottom-up
/// inference (a capture's inner is inferred before the capture itself) and
/// during emission (all type info is available).
pub fn capture_kind(inner: &Pattern, ctx: &TypeContext, interner: &Interner) -> CaptureKind {
    // `field: x @cap` parses as `(field: x) @cap`; the field is only a navigation
    // constraint, so the value mechanism is that of `x`.
    let pattern = unwrap_field(inner);

    if let Pattern::QuantifiedPattern(quant) = &pattern {
        return match quantifier_arity(quant) {
            // `*` / `+` collect into an array regardless of element shape.
            Some(QuantifierKind::ZeroOrMore | QuantifierKind::OneOrMore) => CaptureKind::Array,
            // `?` only adds optionality; the value mechanism is the inner's.
            Some(QuantifierKind::Optional) => quant
                .inner()
                .map(|i| capture_kind(&i, ctx, interner))
                .unwrap_or(CaptureKind::Node),
            None => CaptureKind::Array,
        };
    }

    // A reference whose definition returns a structured type: the call site does
    // its own Call/Return (and Struct/EndStruct) scoping. A reference to a node/void
    // definition falls through to `Node` — its matched node is captured directly.
    if ref_returns_structured(&pattern, ctx, interner) {
        return CaptureKind::Ref;
    }

    // An empty `{}` is an empty struct scope.
    if is_empty_group(&pattern) {
        return CaptureKind::StructScope;
    }

    // Everything else is decided by the inner's inferred data flow, so the type
    // and the emitted effects can't disagree.
    match ctx.term_info(&pattern).map(|info| &info.flow) {
        // Bubbling captures: a sequence/alternation wraps them in a fresh struct
        // scope; a named node instead captures its matched node and lets the
        // children bubble alongside as sibling fields.
        Some(TypeFlow::Fields(_)) => {
            // Only a union alternation flows `Fields` here; an enum flows `Scalar`
            // and is handled below, so it must not appear in this arm.
            if matches!(pattern, Pattern::SeqPattern(_) | Pattern::Union(_)) {
                CaptureKind::StructScope
            } else {
                CaptureKind::Node
            }
        }
        // A structured scalar is left pending by the inner itself — an enum
        // alternation (`Enum`/`EndEnum`) or a named node forwarding a structured
        // output child.
        Some(TypeFlow::Scalar(type_id)) if produces_output(*type_id, ctx) => {
            CaptureKind::SetAfter
        }
        // Void, or a plain scalar node: the matched node is captured directly.
        _ => CaptureKind::Node,
    }
}

/// Whether a type is a meaningful structured output (enum/struct/ref, or an
/// array/optional thereof). Plain `Node` is not — it is the matched node,
/// captured directly.
pub fn produces_output(type_id: TypeId, ctx: &TypeContext) -> bool {
    match ctx.type_shape(type_id) {
        Some(TypeShape::Enum(_) | TypeShape::Struct(_) | TypeShape::Ref(_)) => true,
        Some(TypeShape::Array { element, .. }) => {
            *element != TYPE_NODE && produces_output(*element, ctx)
        }
        Some(TypeShape::Optional(inner)) => *inner != TYPE_NODE && produces_output(*inner, ctx),
        _ => false,
    }
}

/// Look through a `field: x` wrapper to the value it constrains.
fn unwrap_field(pattern: &Pattern) -> Pattern {
    match pattern {
        Pattern::FieldPattern(f) => f.value().unwrap_or_else(|| pattern.clone()),
        other => other.clone(),
    }
}

/// Classify a quantifier operator into its arity — the single source of truth for
/// which quantifier `SyntaxKind`s repeat. `capture_kind` (here), the arity
/// inference in `infer.rs`, and the implicit-array gate in `compile/quantifier.rs`
/// all read this, so the type system and the emitter can never disagree on whether
/// a quantifier collects an array. `None` only for a malformed quantifier with no
/// operator (the parser guarantees a valid `QuantifiedPattern` carries one).
pub(crate) fn quantifier_arity(quant: &QuantifiedPattern) -> Option<QuantifierKind> {
    Some(match quant.operator()?.kind() {
        SyntaxKind::Question | SyntaxKind::QuestionQuestion => QuantifierKind::Optional,
        SyntaxKind::Star | SyntaxKind::StarQuestion => QuantifierKind::ZeroOrMore,
        SyntaxKind::Plus | SyntaxKind::PlusQuestion => QuantifierKind::OneOrMore,
        _ => return None,
    })
}

/// Whether a quantifier repeats (`*`/`+`, greedy or not) — i.e. collects an array,
/// as opposed to `?`. Gating an implicit array scope on the greedy kinds alone
/// drops it for the non-greedy twins (#469), so this reads [`quantifier_arity`]
/// rather than re-listing the operators.
pub fn is_repeating_quantifier(quant: &QuantifiedPattern) -> bool {
    matches!(
        quantifier_arity(quant),
        Some(QuantifierKind::ZeroOrMore | QuantifierKind::OneOrMore)
    )
}

/// Whether `pattern` is a reference to a definition that returns a structured type.
fn ref_returns_structured(pattern: &Pattern, ctx: &TypeContext, interner: &Interner) -> bool {
    let Pattern::Ref(r) = pattern else {
        return false;
    };
    let Some(name) = r.name() else {
        return false;
    };
    let Some(def_id) = ctx.def_id_for_name(interner, name.text()) else {
        return false;
    };

    // After inference the definition's registered output type is authoritative;
    // this is the path emission always takes.
    if let Some(def_type) = ctx.def_type(def_id) {
        return matches!(
            ctx.type_shape(def_type),
            Some(TypeShape::Struct(_) | TypeShape::Enum(_) | TypeShape::Array { .. })
        );
    }

    // During inference a leaf definition may not be registered yet — the visitor
    // walks every definition in a file before any output type is set. Fall back to
    // the reference's own transparently-inferred flow: a structured result either
    // bubbles its fields (struct) or is a structured scalar (enum/array).
    match ctx.term_info(pattern).map(|info| &info.flow) {
        Some(TypeFlow::Fields(_)) => true,
        Some(TypeFlow::Scalar(t)) => produces_output(*t, ctx),
        _ => false,
    }
}
