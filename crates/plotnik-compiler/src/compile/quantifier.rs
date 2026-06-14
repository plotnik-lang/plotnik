//! Unified quantifier compilation.
//!
//! Consolidates the 6+ code paths for quantified expression compilation into
//! a single unified approach with configuration for:
//! - Whether it's inside an array scope
//! - Whether it's skippable (first-child with Down navigation)
//! - Whether skip/match need separate exits

use crate::analyze::type_check::TypeId;
use crate::bytecode::{EffectIR, Label};
use crate::parser::SyntaxKind;
use crate::parser::ast::{self, Expr};
use plotnik_bytecode::Nav;

use super::Compiler;
use super::capture::{CaptureEffects, check_needs_struct_wrapper, get_row_type_id};
use super::navigation::{is_star_or_plus_quantifier, resumable_search_nav};

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
    /// `?` - matches 0 or 1 time
    Optional,
    /// `??` - non-greedy optional
    OptionalNonGreedy,
    /// `*` - matches 0 or more times
    Star,
    /// `*?` - non-greedy star
    StarNonGreedy,
    /// `+` - matches 1 or more times
    Plus,
    /// `+?` - non-greedy plus
    PlusNonGreedy,
}

impl QuantifierKind {
    /// Parse from a SyntaxKind.
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

/// Parse a quantified expression into its components.
///
/// Returns `Empty` if no inner, `Plain` if inner but no quantifier,
/// `Quantified` if both inner and valid quantifier operator.
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
pub struct QuantifierConfig<'a> {
    /// The inner expression to match.
    pub inner: &'a Expr,
    /// The quantifier kind.
    pub kind: QuantifierKind,
    /// Navigation for first iteration.
    pub first_nav: Option<Nav>,
    /// Whether this is inside an array capture context (needs Push on each iteration).
    pub in_array_context: bool,
    /// Capture effects for each iteration (e.g., Push for arrays).
    pub element_capture: CaptureEffects,
    /// When true, skip and match paths need separate exit labels.
    pub needs_split_exits: bool,
    /// Exit for match path (when needs_split_exits is true).
    pub match_exit: Label,
    /// Exit for skip path (when needs_split_exits is true). Only used for skippable quantifiers.
    pub skip_exit: Option<Label>,
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
        let needs_implicit_array = matches!(kind, QuantifierKind::Star | QuantifierKind::Plus)
            && self.is_ref_returning_structured(&inner);

        if needs_implicit_array {
            // Use array scope: Arr → quantifier with Push → EndArr → exit
            // No capture effects on the array itself (no Set), just collect values
            return self.compile_array_scope(
                &Expr::QuantifiedExpr(quant.clone()),
                exit,
                nav_override,
                vec![], // No capture effects (no @name to set)
                capture,
                false, // Not a string capture
            );
        }

        let config = QuantifierConfig {
            inner: &inner,
            kind,
            first_nav: nav_override,
            in_array_context: false,
            element_capture: capture,
            needs_split_exits: false,
            match_exit: exit,
            skip_exit: None,
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
            needs_split_exits: false,
            match_exit: exit,
            skip_exit: None,
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
        // Handle CapturedExpr wrapping
        if let Expr::CapturedExpr(cap) = expr
            && let Some(inner) = cap.inner()
        {
            // Array capture: need special handling with Arr/EndArr
            if is_star_or_plus_quantifier(Some(&inner)) {
                return self.compile_array_capture_with_exits(
                    cap,
                    &inner,
                    match_exit,
                    skip_exit,
                    nav_override,
                    capture,
                );
            }

            // Non-array capture: build capture effects and recurse
            let capture_effects = self.build_capture_effects(cap, Some(&inner));
            let combined = capture.clone().with_post_values(capture_effects);

            return self.compile_skippable_with_exits(
                &inner,
                match_exit,
                skip_exit,
                nav_override,
                combined,
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
        let needs_implicit_array = matches!(kind, QuantifierKind::Star | QuantifierKind::Plus)
            && self.is_ref_returning_structured(&inner);

        if needs_implicit_array {
            return self.compile_implicit_array_with_exits(
                quant,
                match_exit,
                skip_exit,
                nav_override,
                capture,
            );
        }

        // Handle null injection for both passed captures and internal captures
        let skip_with_null = self.emit_null_for_skip_path(skip_exit, &capture);
        let skip_with_internal_null = self.emit_null_for_internal_captures(skip_with_null, &inner);

        let config = QuantifierConfig {
            inner: &inner,
            kind,
            first_nav: nav_override,
            in_array_context: false,
            element_capture: capture,
            needs_split_exits: true,
            match_exit,
            skip_exit: Some(skip_with_internal_null),
        };

        self.compile_quantified_unified(config)
    }

    /// Compile an array capture (star/plus with @capture) as first-child with separate exits.
    ///
    /// For array captures, we need:
    /// - Arr step at entry
    /// - Two EndArr steps: one for skip (→ skip_exit), one for match (→ match_exit)
    /// - Star compiled to route skip to skip_EndArr, loop exit to match_EndArr
    fn compile_array_capture_with_exits(
        &mut self,
        cap: &ast::CapturedExpr,
        inner: &Expr,
        match_exit: Label,
        skip_exit: Label,
        nav_override: Option<Nav>,
        outer_capture: CaptureEffects,
    ) -> Label {
        let capture_effects = self.build_capture_effects(cap, Some(inner));

        // Create two EndArr steps with different continuations
        let match_endarr = self.emit_endarr_step(&capture_effects, &outer_capture.post, match_exit);
        let skip_endarr = self.emit_endarr_step(&capture_effects, &outer_capture.post, skip_exit);

        let push_effects =
            CaptureEffects::new_post(if self.quantifier_needs_node_for_push(inner) {
                let node_eff = if cap.has_string_annotation() {
                    EffectIR::text()
                } else {
                    EffectIR::node()
                };
                vec![node_eff, EffectIR::push()]
            } else {
                vec![EffectIR::push()]
            });
        let inner_entry = self.compile_star_for_array_with_exits(
            inner,
            match_endarr,
            skip_endarr,
            nav_override,
            push_effects,
        );

        // Emit Arr step at entry (with outer pre-effects like Enum)
        self.emit_arr_step(inner_entry, outer_capture.pre)
    }

    /// Compile an implicit array (star/plus without @capture) returning structured type,
    /// as first-child with separate exits.
    ///
    /// Like `compile_array_capture_with_exits` but without explicit capture effects.
    /// Used when `(RefName)*` where RefName returns enum/struct.
    fn compile_implicit_array_with_exits(
        &mut self,
        quant: &ast::QuantifiedExpr,
        match_exit: Label,
        skip_exit: Label,
        nav_override: Option<Nav>,
        outer_capture: CaptureEffects,
    ) -> Label {
        // No capture effects since there's no @name
        let capture_effects = vec![];

        // Create two EndArr steps with different continuations
        let match_endarr = self.emit_endarr_step(&capture_effects, &outer_capture.post, match_exit);
        let skip_endarr = self.emit_endarr_step(&capture_effects, &outer_capture.post, skip_exit);

        // Inner returns structured type, so no Node effect needed - just Push
        let push_effects = CaptureEffects::new_post(vec![EffectIR::push()]);
        let inner_entry = self.compile_star_for_array_with_exits(
            &Expr::QuantifiedExpr(quant.clone()),
            match_endarr,
            skip_endarr,
            nav_override,
            push_effects,
        );

        // Emit Arr step at entry (with outer pre-effects like Enum)
        self.emit_arr_step(inner_entry, outer_capture.pre)
    }

    /// Compile a star quantifier for array context with separate exits.
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
            needs_split_exits: true,
            match_exit,
            skip_exit: Some(skip_exit),
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

    /// Unified quantifier compilation.
    ///
    /// This is the single entry point for all quantifier compilation, handling:
    /// - Star (`*`), Plus (`+`), and Optional (`?`) quantifiers
    /// - Greedy and non-greedy variants
    /// - Array context (with struct wrappers if needed)
    /// - Split exits for first-child skippable patterns
    fn compile_quantified_unified(&mut self, config: QuantifierConfig<'_>) -> Label {
        let QuantifierConfig {
            inner,
            kind,
            first_nav,
            in_array_context,
            element_capture,
            needs_split_exits,
            match_exit,
            skip_exit,
        } = config;

        // Determine if struct wrapper is needed (once, here)
        let needs_struct_wrapper =
            in_array_context && check_needs_struct_wrapper(inner, self.ctx.type_ctx);
        let row_type_id = if in_array_context {
            get_row_type_id(inner, self.ctx.type_ctx)
        } else {
            None
        };

        // Compile body helper - handles struct wrapper logic in one place
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

                // loop_entry → [repeat_iterate, exit]
                self.emit_branch_epsilon_at(loop_entry, repeat_iterate, match_exit, is_greedy);

                first_iterate
            }

            QuantifierKind::Star | QuantifierKind::StarNonGreedy => {
                if needs_split_exits {
                    // Star with split exits: zero-match takes a separate skip path.
                    let skip = skip_exit.expect("split exits requires skip_exit");
                    self.compile_star_with_skip_retry_split_exits(
                        inner,
                        match_exit,
                        skip,
                        first_nav,
                        element_capture,
                        is_greedy,
                        needs_struct_wrapper,
                        row_type_id,
                    )
                } else {
                    let loop_entry = self.fresh_label();
                    let (first_iterate, repeat_iterate) =
                        self.emit_loop_iterations(first_nav_mode, loop_entry, compile_body);

                    // loop_entry → [repeat_iterate, exit]
                    self.emit_branch_epsilon_at(loop_entry, repeat_iterate, match_exit, is_greedy);

                    // entry → [first_iterate, exit]
                    self.emit_branch_epsilon(first_iterate, match_exit, is_greedy)
                }
            }

            QuantifierKind::Optional | QuantifierKind::OptionalNonGreedy => {
                // Build exit-with-null path for when no match is found.
                let skip_with_null = if needs_split_exits {
                    skip_exit.expect("split exits requires skip_exit")
                } else {
                    let null_exit = self.emit_null_for_skip_path(match_exit, &element_capture);
                    self.emit_null_for_internal_captures(null_exit, inner)
                };

                // Match 0 or 1 time: any failure backtracks to the entry epsilon's
                // checkpoint, which restores the pre-navigation cursor and takes
                // skip_with_null.
                let iterate = self.emit_iteration(first_nav_mode, match_exit, compile_body);

                // entry → [iterate, skip_with_null]
                self.emit_branch_epsilon(iterate, skip_with_null, is_greedy)
            }
        }
    }

    /// Helper for star with split exits and skip-retry.
    /// Skip path goes to skip_exit, match path goes to match_exit.
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

        // loop_entry → [repeat_iterate, match_exit]
        self.emit_branch_epsilon_at(loop_entry, repeat_iterate, match_exit, is_greedy);

        // entry → [first_iterate, skip_exit]; zero-match backtracks to the entry
        // epsilon's checkpoint, restoring the pre-nav cursor and taking skip_exit.
        self.emit_branch_epsilon(first_iterate, skip_exit, is_greedy)
    }
}
