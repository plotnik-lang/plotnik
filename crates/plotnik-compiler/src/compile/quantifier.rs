//! Unified quantifier compilation (`?`, `*`, `+` and non-greedy variants).
//!
//! All quantifier paths — plain, array-context, and split-exit — share one
//! `compile_quantified_unified` entry point so greediness and search-nav logic
//! stay in one place.

use crate::analyze::type_check::{TypeId, is_repeating_quantifier};
use crate::bytecode::{EffectIR, Label};
use crate::parser::SyntaxKind;
use crate::parser::ast::{self, Expr};
use plotnik_bytecode::Nav;

use super::Compiler;
use super::capture::{CaptureEffects, check_needs_struct_wrapper, get_row_type_id};
use super::navigation::resumable_search_nav;
use super::scope::CaptureExits;

/// The nav under which a quantifier iteration runs a resumable position search,
/// or `None` for a bounded anchor that matches a single candidate directly.
///
/// Identical to [`resumable_search_nav`] except `StayExact` is also a search.
/// The difference is the consumer, not the nav: a *match-once* item at
/// `StayExact` is positioned by an outer search and matches exactly there, but a
/// quantifier is a *loop* — even from a fixed `StayExact` start (a called def
/// body, an alternation candidate, the entrypoint) it must scan its siblings
/// forward, so it owns a resumable search. A bounded anchor (`*Skip*`/`*Exact`)
/// stays put in both. Folding this case back into `resumable_search_nav` makes
/// alternations double-wrap and regresses; the two must stay distinct.
pub(super) fn quantifier_search_nav(nav: Nav) -> Option<Nav> {
    match nav {
        Nav::StayExact => Some(Nav::StayExact),
        other => resumable_search_nav(Some(other)),
    }
}

/// Quantifier operator classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuantifierKind {
    Optional,
    OptionalNonGreedy,
    Star,
    StarNonGreedy,
    Plus,
    PlusNonGreedy,
}

impl QuantifierKind {
    pub fn from_syntax(kind: SyntaxKind) -> Option<Self> {
        match kind {
            SyntaxKind::Question => Some(Self::Optional),
            SyntaxKind::QuestionQuestion => Some(Self::OptionalNonGreedy),
            SyntaxKind::Star => Some(Self::Star),
            SyntaxKind::StarQuestion => Some(Self::StarNonGreedy),
            SyntaxKind::Plus => Some(Self::Plus),
            SyntaxKind::PlusQuestion => Some(Self::PlusNonGreedy),
            _ => None,
        }
    }

    /// Returns true if this is a greedy quantifier.
    pub fn is_greedy(self) -> bool {
        matches!(self, Self::Optional | Self::Star | Self::Plus)
    }
}

/// Result of parsing a quantified expression.
enum QuantifierParse {
    /// No inner expression found.
    Empty,
    /// Inner expression exists but no valid quantifier operator.
    Plain(Expr),
    /// Valid quantified expression with inner and kind.
    Quantified { inner: Expr, kind: QuantifierKind },
}

fn parse_quantifier(quant: &ast::QuantifiedExpr) -> QuantifierParse {
    let Some(inner) = quant.inner() else {
        return QuantifierParse::Empty;
    };

    let Some(op) = quant.operator() else {
        return QuantifierParse::Plain(inner);
    };

    match QuantifierKind::from_syntax(op.kind()) {
        Some(kind) => QuantifierParse::Quantified { inner, kind },
        None => QuantifierParse::Plain(inner),
    }
}

/// Configuration for unified quantifier compilation.
pub(super) struct QuantifierConfig<'a> {
    pub inner: &'a Expr,
    pub kind: QuantifierKind,
    /// Navigation for the first iteration.
    pub first_nav: Option<Nav>,
    /// Inside an array capture context — each matched element gets a Push.
    pub in_array_context: bool,
    pub element_capture: CaptureEffects,
    /// `Split` carries both the match and skip labels, so a skippable
    /// quantifier cannot be requested without a skip exit.
    pub exits: CaptureExits,
}

impl Compiler<'_> {
    /// Compile a quantified expression with capture effects (passed to body).
    pub(super) fn compile_quantified_inner(
        &mut self,
        quant: &ast::QuantifiedExpr,
        exit: Label,
        nav_override: Option<Nav>,
        capture: CaptureEffects,
    ) -> Label {
        let (inner, kind) = match parse_quantifier(quant) {
            QuantifierParse::Empty => return exit,
            QuantifierParse::Plain(inner) => {
                return self.compile_expr_inner(&inner, exit, nav_override, capture);
            }
            QuantifierParse::Quantified { inner, kind } => (inner, kind),
        };

        // When the inner returns a structured type (enum/struct) and this is a star/plus
        // quantifier without explicit capture, we still need array scope (Arr/Push/EndArr)
        // because the type system expects an array of these values.
        let needs_implicit_array =
            is_repeating_quantifier(quant) && self.is_ref_returning_structured(&inner);

        if needs_implicit_array {
            // No Set on the array itself — collect structured values via Push only.
            return self.compile_array_capture(
                &Expr::QuantifiedExpr(quant.clone()),
                nav_override,
                vec![],
                capture,
                CaptureExits::Single(exit),
            );
        }

        let config = QuantifierConfig {
            inner: &inner,
            kind,
            first_nav: nav_override,
            in_array_context: false,
            element_capture: capture,
            exits: CaptureExits::Single(exit),
        };

        self.compile_quantified_unified(config)
    }

    /// Compile a quantified expression for array capture with element-level effects.
    ///
    /// The element_capture effects (typically [Push]) are placed on each iteration.
    pub(super) fn compile_quantified_for_array(
        &mut self,
        quant: &ast::QuantifiedExpr,
        exit: Label,
        nav_override: Option<Nav>,
        element_capture: CaptureEffects,
    ) -> Label {
        let (inner, kind) = match parse_quantifier(quant) {
            QuantifierParse::Empty => return exit,
            QuantifierParse::Plain(inner) => {
                return self.compile_expr_inner(&inner, exit, nav_override, element_capture);
            }
            QuantifierParse::Quantified { inner, kind } => (inner, kind),
        };

        let config = QuantifierConfig {
            inner: &inner,
            kind,
            first_nav: nav_override,
            in_array_context: true,
            element_capture,
            exits: CaptureExits::Single(exit),
        };

        self.compile_quantified_unified(config)
    }

    /// Compile a skippable expression (optional/star) with separate exits for skip/match paths.
    pub(super) fn compile_skippable_with_exits(
        &mut self,
        expr: &Expr,
        match_exit: Label,
        skip_exit: Label,
        nav_override: Option<Nav>,
        capture: CaptureEffects,
    ) -> Label {
        // A captured optional/star at this navigating position shares the single
        // mechanism dispatch with the ordinary capture path (`compile_captured`),
        // split exits and all, so the two can never drift — the gap behind both
        // #470 and the suppressive `@_` panic. It emits the scope that matches the
        // declared type (`Obj`/`Arr`/`Suppress`), closing it on both exits.
        if let Expr::CapturedExpr(cap) = expr
            && let Some(inner) = cap.inner()
        {
            return self.compile_captured(
                cap,
                Some(inner),
                nav_override,
                capture,
                CaptureExits::Split {
                    match_exit,
                    skip_exit,
                },
            );
        }

        // Must be a QuantifiedExpr at this point
        let Expr::QuantifiedExpr(quant) = expr else {
            return self.compile_expr_inner(expr, match_exit, nav_override, capture);
        };

        let (inner, kind) = match parse_quantifier(quant) {
            QuantifierParse::Empty => return match_exit,
            QuantifierParse::Plain(inner) => {
                return self.compile_expr_inner(&inner, match_exit, nav_override, capture);
            }
            QuantifierParse::Quantified { inner, kind } => (inner, kind),
        };

        // When the inner returns a structured type (enum/struct) and this is a star/plus
        // quantifier without explicit capture, we still need array scope (Arr/Push/EndArr)
        // with split exits for the skip/match paths.
        let needs_implicit_array =
            is_repeating_quantifier(quant) && self.is_ref_returning_structured(&inner);

        if needs_implicit_array {
            return self.compile_array_capture(
                &Expr::QuantifiedExpr(quant.clone()),
                nav_override,
                vec![],
                capture,
                CaptureExits::Split {
                    match_exit,
                    skip_exit,
                },
            );
        }

        let skip_with_null = self.emit_null_for_skip_path(skip_exit, &capture);
        let skip_with_internal_null = self.emit_null_for_internal_captures(skip_with_null, &inner);

        let config = QuantifierConfig {
            inner: &inner,
            kind,
            first_nav: nav_override,
            in_array_context: false,
            element_capture: capture,
            exits: CaptureExits::Split {
                match_exit,
                skip_exit: skip_with_internal_null,
            },
        };

        self.compile_quantified_unified(config)
    }

    /// Compile an array capture (`(x)* @cap`) or an uncaptured implicit array
    /// (`(R)*` where `R` returns a structured type) — `Arr → quantifier (with Push)
    /// → EndArr+capture → exit(s)`. With `Single` exits the loop falls straight
    /// through; with `Split` exits a zero-match takes `skip_exit` and a loop-exit
    /// takes `match_exit`, each closing the array. `capture_effects` is built once
    /// by the caller (empty for an implicit array); the matched element's
    /// `Node` is pushed only when the element is not already a structured
    /// value ([`quantifier_needs_node_for_push`](Self::quantifier_needs_node_for_push)).
    pub(super) fn compile_array_capture(
        &mut self,
        inner: &Expr,
        nav_override: Option<Nav>,
        capture_effects: Vec<EffectIR>,
        outer_capture: CaptureEffects,
        exits: CaptureExits,
    ) -> Label {
        let push_effects =
            CaptureEffects::new_post(if self.quantifier_needs_node_for_push(inner) {
                vec![EffectIR::node(), EffectIR::push()]
            } else {
                vec![EffectIR::push()]
            });

        let inner_entry = match exits {
            CaptureExits::Single(exit) => {
                let endarr = self.emit_endarr_step(&capture_effects, &outer_capture.post, exit);
                if let Expr::QuantifiedExpr(quant) = inner {
                    self.compile_quantified_for_array(quant, endarr, nav_override, push_effects)
                } else {
                    self.compile_expr_with_nav(inner, endarr, nav_override)
                }
            }
            CaptureExits::Split {
                match_exit,
                skip_exit,
            } => {
                let match_endarr =
                    self.emit_endarr_step(&capture_effects, &outer_capture.post, match_exit);
                let skip_endarr =
                    self.emit_endarr_step(&capture_effects, &outer_capture.post, skip_exit);
                self.compile_star_for_array_with_exits(
                    inner,
                    match_endarr,
                    skip_endarr,
                    nav_override,
                    push_effects,
                )
            }
        };

        // Emit Arr step at entry (with outer pre-effects like Enum)
        self.emit_arr_step(inner_entry, outer_capture.pre)
    }

    fn compile_star_for_array_with_exits(
        &mut self,
        expr: &Expr,
        match_exit: Label,
        skip_exit: Label,
        nav_override: Option<Nav>,
        capture: CaptureEffects,
    ) -> Label {
        let Expr::QuantifiedExpr(quant) = expr else {
            return self.compile_expr_inner(expr, match_exit, nav_override, capture);
        };

        let (inner, kind) = match parse_quantifier(quant) {
            QuantifierParse::Empty => return match_exit,
            QuantifierParse::Plain(inner) => {
                return self.compile_expr_inner(&inner, match_exit, nav_override, capture);
            }
            QuantifierParse::Quantified { inner, kind } => (inner, kind),
        };

        let config = QuantifierConfig {
            inner: &inner,
            kind,
            first_nav: nav_override,
            in_array_context: true,
            element_capture: capture,
            exits: CaptureExits::Split {
                match_exit,
                skip_exit,
            },
        };

        self.compile_quantified_unified(config)
    }

    /// Emit a single quantifier iteration (`?`, or the leading match of `*`/`+`):
    /// reach one element via `nav`, match `compile_body` there, exit to `exit`.
    ///
    /// A search nav (see [`quantifier_search_nav`]) wraps a `StayExact` body in
    /// [`emit_position_search`](Self::emit_position_search), which owns a
    /// checkpoint so the search can resume at a later sibling when a following
    /// pattern fails. A bounded (anchored/exact) nav is applied to the body
    /// directly: it has a single candidate, so the VM's own `continue_search`
    /// enforces the skip policy and the iteration never advances past a named
    /// sibling.
    fn emit_iteration(
        &mut self,
        nav: Nav,
        exit: Label,
        compile_body: impl Fn(&mut Self, Nav, Label) -> Label,
    ) -> Label {
        match quantifier_search_nav(nav) {
            Some(search) => {
                let body = compile_body(self, Nav::StayExact, exit);
                self.emit_position_search(search, body)
            }
            None => compile_body(self, nav, exit),
        }
    }

    /// Emit the first and repeat iterations of a looping quantifier (`*`/`+`),
    /// both looping back to `loop_entry`.
    ///
    /// A resumable search nav shares one `StayExact` body behind two position
    /// searches, so the repeat reuses the body and resumes via the same
    /// mechanism as the first. A bounded nav instead compiles the body twice —
    /// the first iteration applies `first_nav`, the repeat applies its
    /// [`sibling_continuation`](Nav::sibling_continuation) — so repeated matches
    /// are bounded sibling steps (back-to-back) rather than a forward search.
    fn emit_loop_iterations(
        &mut self,
        first_nav: Nav,
        loop_entry: Label,
        compile_body: impl Fn(&mut Self, Nav, Label) -> Label,
    ) -> (Label, Label) {
        let repeat_nav = first_nav.sibling_continuation();
        if let Some(first_search) = quantifier_search_nav(first_nav) {
            let body = compile_body(self, Nav::StayExact, loop_entry);
            // Invariant guarded by `quantifier_tests::search_nav_repeats_as_search`.
            let repeat_search =
                quantifier_search_nav(repeat_nav).expect("a search nav repeats as a search");
            (
                self.emit_position_search(first_search, body),
                self.emit_position_search(repeat_search, body),
            )
        } else {
            (
                compile_body(self, first_nav, loop_entry),
                compile_body(self, repeat_nav, loop_entry),
            )
        }
    }

    /// Single dispatch point for all quantifier shapes; see [`QuantifierConfig`] for knobs.
    fn compile_quantified_unified(&mut self, config: QuantifierConfig<'_>) -> Label {
        let QuantifierConfig {
            inner,
            kind,
            first_nav,
            in_array_context,
            element_capture,
            exits,
        } = config;

        let match_exit = exits.match_exit();

        let needs_struct_wrapper =
            in_array_context && check_needs_struct_wrapper(inner, self.ctx.type_ctx);
        let row_type_id = if in_array_context {
            get_row_type_id(inner, self.ctx.type_ctx)
        } else {
            None
        };

        let compile_body = |this: &mut Self, nav: Nav, exit: Label| -> Label {
            if needs_struct_wrapper {
                this.compile_struct_for_array(inner, exit, Some(nav), row_type_id)
            } else if in_array_context {
                this.compile_with_optional_scope(row_type_id, |t| {
                    t.compile_expr_inner(inner, exit, Some(nav), element_capture.clone())
                })
            } else {
                this.compile_expr_inner(inner, exit, Some(nav), element_capture.clone())
            }
        };

        let is_greedy = kind.is_greedy();
        let first_nav_mode = first_nav.unwrap_or(Nav::Down);

        match kind {
            QuantifierKind::Plus | QuantifierKind::PlusNonGreedy => {
                // Plus: must match at least once. The first iteration has no exit
                // fallback, so a total failure backtracks to the caller.
                let loop_entry = self.fresh_label();
                let (first_iterate, repeat_iterate) =
                    self.emit_loop_iterations(first_nav_mode, loop_entry, compile_body);

                self.emit_branch_epsilon_at(loop_entry, repeat_iterate, match_exit, is_greedy);

                first_iterate
            }

            QuantifierKind::Star | QuantifierKind::StarNonGreedy => match exits {
                CaptureExits::Split {
                    match_exit,
                    skip_exit,
                } => self.compile_star_with_skip_retry_split_exits(
                    inner,
                    match_exit,
                    skip_exit,
                    first_nav,
                    element_capture,
                    is_greedy,
                    needs_struct_wrapper,
                    row_type_id,
                ),
                CaptureExits::Single(_) => {
                    let loop_entry = self.fresh_label();
                    let (first_iterate, repeat_iterate) =
                        self.emit_loop_iterations(first_nav_mode, loop_entry, compile_body);

                    self.emit_branch_epsilon_at(loop_entry, repeat_iterate, match_exit, is_greedy);
                    self.emit_branch_epsilon(first_iterate, match_exit, is_greedy)
                }
            },

            QuantifierKind::Optional | QuantifierKind::OptionalNonGreedy => {
                let skip_with_null = match exits {
                    CaptureExits::Split { skip_exit, .. } => skip_exit,
                    CaptureExits::Single(_) => {
                        let null_exit = self.emit_null_for_skip_path(match_exit, &element_capture);
                        self.emit_null_for_internal_captures(null_exit, inner)
                    }
                };

                // Any failure backtracks to the entry epsilon's checkpoint, restoring
                // the pre-navigation cursor and taking skip_with_null.
                let iterate = self.emit_iteration(first_nav_mode, match_exit, compile_body);
                self.emit_branch_epsilon(iterate, skip_with_null, is_greedy)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn compile_star_with_skip_retry_split_exits(
        &mut self,
        inner: &Expr,
        match_exit: Label,
        skip_exit: Label,
        nav_override: Option<Nav>,
        capture: CaptureEffects,
        is_greedy: bool,
        needs_struct_wrapper: bool,
        row_type_id: Option<TypeId>,
    ) -> Label {
        let compile_body = |this: &mut Self, nav: Nav, exit: Label| -> Label {
            if needs_struct_wrapper {
                this.compile_struct_for_array(inner, exit, Some(nav), row_type_id)
            } else {
                this.compile_expr_inner(inner, exit, Some(nav), capture.clone())
            }
        };

        let loop_entry = self.fresh_label();
        let first_nav_mode = nav_override.unwrap_or(Nav::Down);
        let (first_iterate, repeat_iterate) =
            self.emit_loop_iterations(first_nav_mode, loop_entry, compile_body);

        self.emit_branch_epsilon_at(loop_entry, repeat_iterate, match_exit, is_greedy);

        // zero-match backtracks to the entry epsilon's checkpoint, restoring the
        // pre-nav cursor and taking skip_exit.
        self.emit_branch_epsilon(first_iterate, skip_exit, is_greedy)
    }
}
