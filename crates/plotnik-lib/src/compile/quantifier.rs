//! Unified quantifier compilation.
//!
//! Consolidates the 6+ code paths for quantified expression compilation into
//! a single unified approach with configuration for:
//! - Whether it's inside an array scope
//! - Whether it's skippable (first-child with Down navigation)
//! - Whether skip/match need separate exits

use crate::bytecode::ir::{EffectIR, Label};
use crate::bytecode::{EffectOpcode, Nav};
use crate::parser::ast::{self, Expr};
use crate::parser::cst::SyntaxKind;
use crate::analyze::type_check::TypeId;

use super::capture::{check_needs_struct_wrapper, get_row_type_id, CaptureEffects};
use super::navigation::{is_star_or_plus_quantifier, repeat_nav_for};
use super::Compiler;

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
    /// Compile a quantified expression: `a?`, `a*`, `a+`.
    pub(super) fn compile_quantified(&mut self, quant: &ast::QuantifiedExpr, exit: Label) -> Label {
        self.compile_quantified_inner(quant, exit, None, CaptureEffects::default())
    }

    /// Compile a quantified expression with capture effects (passed to body).
    pub(super) fn compile_quantified_inner(
        &mut self,
        quant: &ast::QuantifiedExpr,
        exit: Label,
        nav_override: Option<Nav>,
        capture: CaptureEffects,
    ) -> Label {
        let Some(inner) = quant.inner() else {
            return exit;
        };

        let Some(op) = quant.operator() else {
            return self.compile_expr_inner(&inner, exit, nav_override, capture);
        };

        let Some(kind) = QuantifierKind::from_syntax(op.kind()) else {
            return self.compile_expr_inner(&inner, exit, nav_override, capture);
        };

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
        let Some(inner) = quant.inner() else {
            return exit;
        };

        let Some(op) = quant.operator() else {
            return self.compile_expr_inner(&inner, exit, nav_override, element_capture);
        };

        let Some(kind) = QuantifierKind::from_syntax(op.kind()) else {
            return self.compile_expr_inner(&inner, exit, nav_override, element_capture);
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
                    cap, &inner, match_exit, skip_exit, nav_override, capture,
                );
            }

            // Non-array capture: build capture effects and recurse
            let capture_effects = self.build_capture_effects(cap, Some(&inner));
            let mut combined = CaptureEffects { post: capture_effects };
            combined.post.extend(capture.post);

            return self.compile_skippable_with_exits(
                &inner, match_exit, skip_exit, nav_override, combined,
            );
        }

        // Must be a QuantifiedExpr at this point
        let Expr::QuantifiedExpr(quant) = expr else {
            return self.compile_expr_inner(expr, match_exit, nav_override, capture);
        };

        let Some(inner) = quant.inner() else {
            return match_exit;
        };

        let Some(op) = quant.operator() else {
            return self.compile_expr_inner(&inner, match_exit, nav_override, capture);
        };

        let Some(kind) = QuantifierKind::from_syntax(op.kind()) else {
            return self.compile_expr_inner(&inner, match_exit, nav_override, capture);
        };

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

        // Compile inner star with Push effects and split exits
        let push_effects = CaptureEffects {
            post: vec![EffectIR::simple(EffectOpcode::Push, 0)],
        };
        let inner_entry =
            self.compile_star_for_array_with_exits(inner, match_endarr, skip_endarr, nav_override, push_effects);

        // Emit Arr step at entry
        self.emit_arr_step(inner_entry)
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

        let Some(inner) = quant.inner() else {
            return match_exit;
        };

        let Some(op) = quant.operator() else {
            return self.compile_expr_inner(&inner, match_exit, nav_override, capture);
        };

        let Some(kind) = QuantifierKind::from_syntax(op.kind()) else {
            return self.compile_expr_inner(&inner, match_exit, nav_override, capture);
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
        let needs_struct_wrapper = in_array_context && check_needs_struct_wrapper(inner, self.type_ctx);
        let row_type_id = if in_array_context {
            get_row_type_id(inner, self.type_ctx)
        } else {
            None
        };

        // Compile body helper - handles struct wrapper logic in one place
        let compile_body = |this: &mut Self, nav: Option<Nav>, exit: Label| -> Label {
            if needs_struct_wrapper {
                this.compile_struct_for_array(inner, exit, nav, row_type_id)
            } else if in_array_context {
                this.compile_with_optional_scope(row_type_id, |t| {
                    t.compile_expr_inner(inner, exit, nav, element_capture.clone())
                })
            } else {
                this.compile_expr_inner(inner, exit, nav, element_capture.clone())
            }
        };

        let is_greedy = kind.is_greedy();
        let repeat_nav = repeat_nav_for(first_nav);

        match kind {
            QuantifierKind::Plus | QuantifierKind::PlusNonGreedy => {
                // +: first_body → loop → [repeat_body, exit]
                let loop_entry = self.fresh_label();

                let first_body_entry = compile_body(self, first_nav, loop_entry);
                let repeat_body_entry = compile_body(self, repeat_nav, loop_entry);

                let successors = if is_greedy {
                    vec![repeat_body_entry, match_exit]
                } else {
                    vec![match_exit, repeat_body_entry]
                };
                self.emit_epsilon(loop_entry, successors);

                first_body_entry
            }

            QuantifierKind::Star | QuantifierKind::StarNonGreedy => {
                if needs_split_exits {
                    // Star with split exits: entry → [first_body → loop → [repeat_body, match_exit], skip_exit]
                    let skip = skip_exit.expect("split exits requires skip_exit");
                    self.compile_star_with_split_exits(
                        inner, match_exit, skip, first_nav, element_capture, is_greedy,
                        needs_struct_wrapper, row_type_id,
                    )
                } else {
                    // Regular star: entry → [first_body → loop → [repeat_body, exit], exit]
                    let loop_entry = self.fresh_label();

                    let first_body_entry = compile_body(self, first_nav, loop_entry);
                    let repeat_body_entry = compile_body(self, repeat_nav, loop_entry);

                    // Entry point branches: first iteration or exit
                    let entry = self.fresh_label();
                    let successors = if is_greedy {
                        vec![first_body_entry, match_exit]
                    } else {
                        vec![match_exit, first_body_entry]
                    };
                    self.emit_epsilon(entry, successors);

                    // Loop point branches: repeat iteration or exit
                    let loop_successors = if is_greedy {
                        vec![repeat_body_entry, match_exit]
                    } else {
                        vec![match_exit, repeat_body_entry]
                    };
                    self.emit_epsilon(loop_entry, loop_successors);

                    entry
                }
            }

            QuantifierKind::Optional | QuantifierKind::OptionalNonGreedy => {
                // ?: branch to body or exit
                let body_entry = compile_body(self, first_nav, match_exit);

                if needs_split_exits {
                    // Split exits for optional
                    let skip = skip_exit.expect("split exits requires skip_exit");
                    self.emit_branch_epsilon(body_entry, skip, is_greedy)
                } else {
                    // Regular optional with null emission for skipped captures
                    // First handle captures passed as effects (e.g., `(x)? @cap`)
                    let skip_with_null = self.emit_null_for_skip_path(match_exit, &element_capture);
                    // Then handle internal captures (e.g., `{(x) @cap}?`)
                    let skip_with_internal_null = self.emit_null_for_internal_captures(skip_with_null, inner);
                    self.emit_branch_epsilon(body_entry, skip_with_internal_null, is_greedy)
                }
            }
        }
    }

    /// Helper for star with split exits - handles the complex case where skip and match
    /// paths need different continuations.
    #[allow(clippy::too_many_arguments)]
    fn compile_star_with_split_exits(
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
        let loop_entry = self.fresh_label();
        let repeat_nav = repeat_nav_for(nav_override);

        let first_body = if needs_struct_wrapper {
            self.compile_struct_for_array(inner, loop_entry, nav_override, row_type_id)
        } else {
            self.compile_expr_inner(inner, loop_entry, nav_override, capture.clone())
        };

        let repeat_body = if needs_struct_wrapper {
            self.compile_struct_for_array(inner, loop_entry, repeat_nav, row_type_id)
        } else {
            self.compile_expr_inner(inner, loop_entry, repeat_nav, capture)
        };

        let entry = self.emit_branch_epsilon(first_body, skip_exit, is_greedy);
        self.emit_branch_epsilon_at(loop_entry, repeat_body, match_exit, is_greedy);
        entry
    }
}
