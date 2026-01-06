//! Unified quantifier compilation.
//!
//! Consolidates the 6+ code paths for quantified expression compilation into
//! a single unified approach with configuration for:
//! - Whether it's inside an array scope
//! - Whether it's skippable (first-child with Down navigation)
//! - Whether skip/match need separate exits

use crate::analyze::type_check::TypeId;
use crate::bytecode::Nav;
use crate::bytecode::ir::{EffectIR, Label};
use crate::parser::ast::{self, Expr};
use crate::parser::cst::SyntaxKind;

use super::Compiler;
use super::capture::{CaptureEffects, check_needs_struct_wrapper, get_row_type_id};
use super::navigation::is_star_or_plus_quantifier;

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
            let mut combined = CaptureEffects {
                pre: capture.pre.clone(),
                post: capture_effects,
            };
            combined.post.extend(capture.post);

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

        let push_effects = CaptureEffects {
            pre: vec![],
            post: if self.quantifier_needs_node_for_push(inner) {
                let node_eff = if cap.has_string_annotation() {
                    EffectIR::text()
                } else {
                    EffectIR::node()
                };
                vec![node_eff, EffectIR::push()]
            } else {
                vec![EffectIR::push()]
            },
        };
        let inner_entry = self.compile_star_for_array_with_exits(
            inner,
            match_endarr,
            skip_endarr,
            nav_override,
            push_effects,
        );

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
            in_array_context && check_needs_struct_wrapper(inner, self.type_ctx);
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

        match kind {
            QuantifierKind::Plus | QuantifierKind::PlusNonGreedy => {
                // Plus with skip-retry: must match at least once
                // First iteration has no exit fallback (backtrack propagates to caller)
                let loop_entry = self.fresh_label();

                // Compile body ONCE with Nav::StayExact (exact match at current position,
                // skip-retry handles advancement if all branches fail)
                let body_entry = compile_body(self, Some(Nav::StayExact), loop_entry);

                // First iteration: skip-retry but NO exit (must match at least one)
                let first_nav_mode = first_nav.unwrap_or(Nav::Down);
                let first_iterate = self.compile_skip_retry_iteration_no_exit(
                    first_nav_mode,
                    body_entry,
                    is_greedy,
                );

                // Repeat iteration: skip-retry with exit option
                let repeat_iterate =
                    self.compile_skip_retry_iteration(Nav::Next, body_entry, match_exit, is_greedy);

                // loop_entry → [repeat_iterate, exit]
                self.emit_branch_epsilon_at(loop_entry, repeat_iterate, match_exit, is_greedy);

                first_iterate
            }

            QuantifierKind::Star | QuantifierKind::StarNonGreedy => {
                if needs_split_exits {
                    // Star with split exits: uses skip-retry with separate exit paths
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
                    // Regular star with skip-retry:
                    // When pattern fails (even on descendant), retry with next sibling
                    let loop_entry = self.fresh_label();

                    // Compile body ONCE with Nav::StayExact (exact match at current position,
                    // skip-retry handles advancement if all branches fail)
                    let body_entry = compile_body(self, Some(Nav::StayExact), loop_entry);

                    // First iteration: Down navigation with skip-retry
                    let first_nav_mode = first_nav.unwrap_or(Nav::Down);
                    let first_iterate = self.compile_skip_retry_iteration(
                        first_nav_mode,
                        body_entry,
                        match_exit,
                        is_greedy,
                    );

                    // Repeat iteration: Next navigation with skip-retry
                    let repeat_iterate = self.compile_skip_retry_iteration(
                        Nav::Next,
                        body_entry,
                        match_exit,
                        is_greedy,
                    );

                    // loop_entry → [repeat_iterate, exit]
                    self.emit_branch_epsilon_at(loop_entry, repeat_iterate, match_exit, is_greedy);

                    // entry → [first_iterate, exit]
                    self.emit_branch_epsilon(first_iterate, match_exit, is_greedy)
                }
            }

            QuantifierKind::Optional | QuantifierKind::OptionalNonGreedy => {
                // Optional with skip-retry: matches 0 or 1 time
                // Compile body with Nav::StayExact (exact match at current position)
                let body_entry = compile_body(self, Some(Nav::StayExact), match_exit);

                // Build exit-with-null path for when no match found
                let skip_with_null = if needs_split_exits {
                    skip_exit.expect("split exits requires skip_exit")
                } else {
                    let null_exit = self.emit_null_for_skip_path(match_exit, &element_capture);
                    self.emit_null_for_internal_captures(null_exit, inner)
                };

                // Skip-retry iteration leading to null exit
                let first_nav_mode = first_nav.unwrap_or(Nav::Down);
                let iterate = self.compile_skip_retry_iteration(
                    first_nav_mode,
                    body_entry,
                    skip_with_null,
                    is_greedy,
                );

                // entry → [iterate, skip_with_null]
                self.emit_branch_epsilon(iterate, skip_with_null, is_greedy)
            }
        }
    }

    /// Compile a "try-skip-retry" iteration for quantifiers.
    ///
    /// Structure:
    /// ```text
    ///   navigate: Match(nav, wildcard) → try
    ///   try: epsilon → [body, retry_or_exit]
    ///   retry_or_exit: epsilon → [retry_nav, exit]
    ///   retry_nav: Match(Nav::Next, wildcard) → try
    /// ```
    ///
    /// When the body fails (even deep inside on a descendant), we backtrack to `try`,
    /// which falls through to `retry_or_exit`, advancing to the next sibling and retrying.
    /// Only when siblings are exhausted do we take the exit path.
    ///
    /// Returns the navigate label (entry point for this iteration).
    fn compile_skip_retry_iteration(
        &mut self,
        nav: Nav,
        body_entry: Label,
        exit: Label,
        is_greedy: bool,
    ) -> Label {
        let try_label = self.fresh_label();
        let retry_or_exit = self.fresh_label();

        // retry_nav: advance and loop back to try
        let retry_nav = self.fresh_label();
        self.emit_wildcard_nav(retry_nav, Nav::Next, try_label);

        // retry_or_exit: epsilon → [retry_nav, exit]
        self.emit_branch_epsilon_at(retry_or_exit, retry_nav, exit, is_greedy);

        // try: epsilon → [body, retry_or_exit]
        self.emit_branch_epsilon_at(try_label, body_entry, retry_or_exit, is_greedy);

        // navigate: wildcard nav → try
        let navigate = self.fresh_label();
        self.emit_wildcard_nav(navigate, nav, try_label);

        navigate
    }

    /// Like `compile_skip_retry_iteration` but with no exit fallback.
    ///
    /// Used for Plus quantifier's first iteration where at least one match is required.
    /// If all siblings fail, backtrack propagates to caller (quantifier fails).
    fn compile_skip_retry_iteration_no_exit(
        &mut self,
        nav: Nav,
        body_entry: Label,
        is_greedy: bool,
    ) -> Label {
        let try_label = self.fresh_label();

        // retry_nav: advance and loop back to try (no exit option)
        let retry_nav = self.fresh_label();
        self.emit_wildcard_nav(retry_nav, Nav::Next, try_label);

        // try: epsilon → [body, retry_nav]
        // If pattern fails, we advance and retry. If no more siblings,
        // retry_nav's navigation fails and we backtrack to outer checkpoint.
        self.emit_branch_epsilon_at(try_label, body_entry, retry_nav, is_greedy);

        // navigate: wildcard nav → try
        let navigate = self.fresh_label();
        self.emit_wildcard_nav(navigate, nav, try_label);

        navigate
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
        let loop_entry = self.fresh_label();

        // Compile body ONCE with Nav::StayExact (exact match at current position,
        // skip-retry handles advancement if all branches fail)
        let body_entry = if needs_struct_wrapper {
            self.compile_struct_for_array(inner, loop_entry, Some(Nav::StayExact), row_type_id)
        } else {
            self.compile_expr_inner(inner, loop_entry, Some(Nav::StayExact), capture)
        };

        // First iteration: skip-retry with skip_exit as fallback
        let first_nav_mode = nav_override.unwrap_or(Nav::Down);
        let first_iterate =
            self.compile_skip_retry_iteration(first_nav_mode, body_entry, skip_exit, is_greedy);

        // Repeat iteration: skip-retry with match_exit as fallback
        let repeat_iterate =
            self.compile_skip_retry_iteration(Nav::Next, body_entry, match_exit, is_greedy);

        // loop_entry → [repeat_iterate, match_exit]
        self.emit_branch_epsilon_at(loop_entry, repeat_iterate, match_exit, is_greedy);

        // entry → [first_iterate, skip_exit]
        self.emit_branch_epsilon(first_iterate, skip_exit, is_greedy)
    }
}
