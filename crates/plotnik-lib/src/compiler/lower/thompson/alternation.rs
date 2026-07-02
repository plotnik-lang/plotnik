use std::collections::{BTreeMap, HashSet};

use crate::bytecode::{EffectKind, Nav};
use crate::compiler::analyze::types::TypeShape;
use crate::compiler::analyze::types::type_shape::FieldInfo;
use crate::compiler::analyze::types::type_shape::PatternFlow;
use crate::compiler::ids::TypeId;
use crate::compiler::lower::ir::{
    EffectIR, InstructionIR, Label, MatchIR, MemberRef, NodeKindConstraint,
};
use crate::compiler::parse::ast::{self, Pattern};
use crate::core::Symbol;

use super::NfaBuilder;
use super::capture::{CaptureEffects, PatternCtx};
use super::navigation::{AnchorSemantics, pattern_owns_iteration, resumable_search_nav};
use super::scope::{SkipExit, SplitExits};

/// The alternation's resumable search nav (from [`resumable_search_nav`]), kept
/// distinct from a branch's `first_nav` so the two adjacent `Option<Nav>` inputs
/// to [`nav_for_alt_branch`] cannot be transposed. `Some` means the alternation
/// owns the retry wrapper and each branch matches exactly at the candidate
/// (`StayExact`).
#[derive(Clone, Copy)]
struct AltSearchNav(Option<Nav>);

struct BranchRouting {
    branch_named: Vec<bool>,
    named_exit: Option<Label>,
}

impl BranchRouting {
    fn branch_exit(&self, branch_idx: usize, default_exit: Label) -> Label {
        match self.named_exit {
            Some(skip) if self.branch_named[branch_idx] => skip,
            _ => default_exit,
        }
    }
}

fn exact_nav_for_alt_branch(first_nav: Option<Nav>, search_nav: AltSearchNav) -> Option<Nav> {
    if search_nav.0.is_some() {
        return Some(Nav::StayExact);
    }

    let nav = match first_nav {
        None => Nav::StayExact,
        Some(
            nav @ (Nav::DownSkip
            | Nav::DownSkipExtras
            | Nav::NextSkip
            | Nav::NextSkipExtras
            | Nav::UpSkipTrivia(_)
            | Nav::UpSkipExtras(_)),
        ) => nav,
        Some(nav) => nav.to_exact(),
    };
    Some(nav)
}

fn nav_for_alt_branch(
    first_nav: Option<Nav>,
    search_nav: AltSearchNav,
    body: &Pattern,
    anchor_semantics: &AnchorSemantics<'_>,
) -> Option<Nav> {
    let nav = exact_nav_for_alt_branch(first_nav, search_nav)?;

    if !anchor_semantics.pattern_may_match_anonymous(Some(body)) {
        return Some(nav);
    }

    Some(match nav {
        Nav::DownSkip => Nav::DownSkipExtras,
        Nav::NextSkip => Nav::NextSkipExtras,
        nav => nav,
    })
}

impl NfaBuilder<'_> {
    /// Clone a soft-anchor follower into a named-node-only retry entry.
    ///
    /// Sequence lowering has already compiled the follower after the alternation's
    /// `exit`. Soft anchors normally conservatively skip extras only
    /// (`NextSkipExtras`) because an anonymous-token left side must not skip
    /// anonymous tokens before the follower. When a branch definitely matched a
    /// named node, the soft-anchor rule allows anonymous-token skipping too, so a
    /// `NextSkip` clone of that follower preserves the intended soft-anchor
    /// semantics for named branches without weakening anonymous branches.
    ///
    /// The clone is intentionally narrow: it reuses the already-compiled follower's
    /// successor, effects, predicate, field constraints, etc., so only navigation
    /// changes. That keeps the branch-specific tweak local and avoids recompiling a
    /// sibling suffix with duplicated effects. The clone is appended after its
    /// successor has already been emitted; label references are symbolic IR, so
    /// the order is irrelevant until packing.
    ///
    /// A captured/tagged alternation does not exit straight into the follower:
    /// capture lowering interposes effect epsilons (`Set`, scope closes) between
    /// alternation exit and follower (#472). The walk below sees through that
    /// chain and clones it along with the follower, so each branch runs the
    /// chain's effects exactly once — via the named twin or the conservative
    /// original, never both.
    ///
    /// Returns `None` — caller stays conservative — unless the chain is
    /// single-successor epsilons ending at a `Match` carrying `NextSkipExtras` on
    /// a `Named` node, the one shape where the upgrade is both safe and needed.
    /// The `Named` check matters because `NextSkipExtras` is ambiguous: it also
    /// appears when the *follower* may match anonymous nodes, and then extras-only
    /// skipping is correct even after a named branch. Anonymous/`_` followers fail
    /// that check; a ref follower (`Call`) is skipped because the IR alone cannot
    /// prove the callee never matches an anonymous node.
    fn clone_named_follower_skip_entry(&mut self, exit: Label) -> Option<Label> {
        let mut chain: Vec<MatchIR> = Vec::new();
        let mut seen = HashSet::new();
        let mut cursor = exit;
        let mut twin = loop {
            if !seen.insert(cursor) {
                return None;
            }
            let InstructionIR::Match(m) = self.instructions.iter().find(|i| i.label() == cursor)?
            else {
                return None;
            };
            if !m.is_epsilon() {
                break m.clone();
            }
            let &[next] = m.successors.as_slice() else {
                return None;
            };
            chain.push(m.clone());
            cursor = next;
        };

        if twin.nav != Nav::NextSkipExtras || !matches!(twin.node_kind, NodeKindConstraint::Named(_))
        {
            return None;
        }

        twin.label = self.fresh_label();
        twin.nav = Nav::NextSkip;
        let mut entry = twin.label;
        self.instructions.push(twin.into());
        for mut eps in chain.into_iter().rev() {
            eps.label = self.fresh_label();
            eps.successors = vec![entry];
            entry = eps.label;
            self.instructions.push(eps.into());
        }
        Some(entry)
    }

    /// Per-branch "named" flags plus the soft-skip follower twin — shared by both
    /// alternation kinds. A branch is "named" (eligible for the twin) when it cannot
    /// match an anonymous node and does not own its own iteration. A quantified
    /// branch's zero-match path leaves no named node on the anchor's left, so the
    /// soft-skip upgrade is unsound there. The anonymity test is whole-branch,
    /// matching `nav_for_alt_branch`'s before-anchor classification. The twin is a
    /// `NextSkip` clone of a conservative (`NextSkipExtras`) soft follower, worth
    /// cloning only when at least one branch is itself named.
    fn alt_branch_routing(&mut self, branches: &[ast::Branch], exit: Label) -> BranchRouting {
        let branch_named: Vec<bool> = {
            let anchor_semantics = &self.anchor_semantics;
            branches
                .iter()
                .map(|b| {
                    b.body().is_some_and(|body| {
                        !pattern_owns_iteration(&body)
                            && !anchor_semantics.pattern_may_match_anonymous(Some(&body))
                    })
                })
                .collect()
        };

        let named_exit = branch_named
            .iter()
            .any(|&named| named)
            .then(|| self.clone_named_follower_skip_entry(exit))
            .flatten();

        BranchRouting {
            branch_named,
            named_exit,
        }
    }

    /// A resumable search nav (`Down`/`Next`/`Stay`) gets one position-search retry
    /// wrapper around the fanned-in branches; otherwise each branch already performed
    /// its own exact navigation.
    ///
    /// `zero_width` holds the lifted zero-width continuations of nullable
    /// branches (see [`compile_union_branches`](Self::compile_union_branches)).
    /// They sit outside the position search — a zero-width outcome needs no
    /// candidate node — and after it: consuming matches, at any candidate and
    /// in any branch, are preferred over a zero-width one.
    fn assemble_alt_branches(
        &mut self,
        successors: Vec<Label>,
        zero_width: Vec<Label>,
        search_nav: Option<Nav>,
        exit: Label,
    ) -> Label {
        if successors.is_empty() && zero_width.is_empty() {
            return exit;
        }

        let real_entry = if successors.is_empty() {
            None
        } else {
            let alt_entry = if successors.len() == 1 {
                successors[0]
            } else {
                let entry = self.fresh_label();
                self.emit_epsilon(entry, successors);
                entry
            };
            Some(match search_nav {
                Some(nav) => self.emit_position_search(nav, alt_entry),
                None => alt_entry,
            })
        };

        let mut alternatives: Vec<Label> = real_entry.into_iter().chain(zero_width).collect();
        if alternatives.len() == 1 {
            return alternatives.remove(0);
        }
        let entry = self.fresh_label();
        self.emit_epsilon(entry, alternatives);
        entry
    }

    /// Union alternation: each branch merges into one struct.
    pub(super) fn compile_union(&mut self, union: &ast::UnionPattern, ctx: PatternCtx) -> Label {
        let skip_exit = SkipExit::To(ctx.exit);
        self.compile_union_with_exits(union, ctx, skip_exit)
    }

    /// [`compile_union`](Self::compile_union) with a distinct zero-width
    /// continuation (a skippable sequence item, or a pruned iteration element).
    pub(super) fn compile_union_with_exits(
        &mut self,
        union: &ast::UnionPattern,
        ctx: PatternCtx,
        skip_exit: SkipExit,
    ) -> Label {
        let branches: Vec<_> = union.branches().collect();
        self.compile_union_branches(&Pattern::Union(union.clone()), &branches, ctx, skip_exit)
    }

    /// A labeled alternation nothing consumes: the labels are inert (inference
    /// degraded it to a union and warned), so it compiles exactly like one —
    /// branch captures set into the enclosing scope, no variant tagging.
    pub(super) fn compile_degraded_enum(&mut self, e: &ast::EnumPattern, ctx: PatternCtx) -> Label {
        let skip_exit = SkipExit::To(ctx.exit);
        self.compile_degraded_enum_with_exits(e, ctx, skip_exit)
    }

    /// [`compile_degraded_enum`](Self::compile_degraded_enum) with a distinct
    /// zero-width continuation.
    pub(super) fn compile_degraded_enum_with_exits(
        &mut self,
        e: &ast::EnumPattern,
        ctx: PatternCtx,
        skip_exit: SkipExit,
    ) -> Label {
        let branches: Vec<_> = e.branches().collect();
        self.compile_union_branches(&Pattern::Enum(e.clone()), &branches, ctx, skip_exit)
    }

    /// Shared lowering for union alternations and degraded (unconsumed) enum
    /// alternations. `alternation` is the pattern whose inferred result carries
    /// the merged output struct.
    ///
    /// A nullable branch compiles pruned ([`SkipExit::Fail`]) so its body only
    /// matches by consuming; its zero-width outcome is lifted to one shared
    /// alternative after the candidate search — a pure-effect epsilon that
    /// defaults every merged field and exits to `skip_exit` with the cursor
    /// untouched. That gives the zero-width path a life outside the search
    /// (it needs no candidate node) and an honest cursor for any follower.
    fn compile_union_branches(
        &mut self,
        alternation: &Pattern,
        branches: &[ast::Branch],
        ctx: PatternCtx,
        skip_exit: SkipExit,
    ) -> Label {
        let PatternCtx {
            exit,
            nav: first_nav,
            capture,
        } = ctx;
        if branches.is_empty() {
            return exit;
        }

        // In a suppressed region there is no output shape to keep stable (and a
        // consumed enum routed here still flows `Value(enum)`, not a struct).
        let union_type_id = if self.is_suppressed() {
            None
        } else {
            self.ctx
                .analysis
                .type_analysis
                .expect_pattern_result(alternation)
                .flow
                .type_id()
        };
        let merged_fields =
            union_type_id.map(|id| self.ctx.analysis.type_analysis.expect_struct_fields(id));

        let search_nav = resumable_search_nav(first_nav);
        let branch_search = AltSearchNav(search_nav);
        let branch_routing = self.alt_branch_routing(branches, exit);

        let mut successors = Vec::new();
        let mut any_nullable = false;
        for (branch_idx, branch) in branches.iter().enumerate() {
            let Some(body) = branch.body() else {
                continue;
            };

            let branch_exit = branch_routing.branch_exit(branch_idx, exit);

            // Inject a default for every merged field this branch does not itself
            // produce, so the output shape stays stable. "Produces" means a top-level
            // (bubbling) field — a capture nested in a child scope (`{...} @row`)
            // belongs to that scope, not here. The branch's inferred bubble is the
            // single source of truth; a syntactic capture walk would miscount nested
            // names and drop a needed default.
            let null_effects: Vec<EffectIR> = if let Some(fields) = merged_fields {
                // Only bubbling fields count as provided; a `Value` branch (a
                // bare reference, suppressed) contributes nothing here.
                let provided: HashSet<Symbol> = match &self
                    .ctx
                    .analysis
                    .type_analysis
                    .expect_pattern_result(&body)
                    .flow
                {
                    PatternFlow::Fields(id) => self
                        .ctx
                        .analysis
                        .type_analysis
                        .expect_struct_fields(*id)
                        .keys()
                        .copied()
                        .collect(),
                    _ => HashSet::new(),
                };
                self.union_default_effects(fields, &provided)
            } else {
                vec![]
            };

            let branch_nav =
                nav_for_alt_branch(first_nav, branch_search, &body, &self.anchor_semantics);
            let branch_entry = if self.pattern_is_nullable(&body) {
                any_nullable = true;
                // Pruned body: merged effects stay on dominating epsilons —
                // the body's partial-skip paths must not drop them.
                let exit = if capture.post.is_empty() {
                    branch_exit
                } else {
                    self.emit_effects_epsilon(
                        branch_exit,
                        vec![],
                        CaptureEffects::new_post(capture.post.clone()),
                    )
                };
                let entry = self.compile_skippable_with_exits(
                    &body,
                    SplitExits {
                        match_exit: exit,
                        skip_exit: SkipExit::Fail,
                    },
                    branch_nav,
                    CaptureEffects::default(),
                );
                let mut pre = capture.pre.clone();
                pre.extend(null_effects);
                self.wrap_entry_pre(entry, pre)
            } else {
                let branch_capture = capture.clone().with_pre_values(null_effects);
                self.dispatch_pattern(
                    &body,
                    PatternCtx {
                        exit: branch_exit,
                        nav: branch_nav,
                        capture: branch_capture,
                    },
                )
            };
            successors.push(branch_entry);
        }

        // One shared zero-width alternative: whichever nullable branch matched
        // zero-width, the union output is the same — every merged field at its
        // default.
        let zero_width = match skip_exit {
            SkipExit::To(skip) if any_nullable => {
                let defaults = merged_fields
                    .map(|fields| self.union_default_effects(fields, &HashSet::new()))
                    .unwrap_or_default();
                let mut pre = capture.pre.clone();
                pre.extend(defaults);
                vec![self.emit_zero_width_step(skip, pre, capture.post.clone())]
            }
            _ => vec![],
        };

        self.assemble_alt_branches(successors, zero_width, search_nav, exit)
    }

    /// `[Null, Set]` (or `[Arr, EndArr, Set]` for a required list) for every
    /// merged field not in `provided`, resolved against the enclosing scope —
    /// the output a path that skips those captures owes.
    fn union_default_effects(
        &self,
        fields: &BTreeMap<Symbol, FieldInfo>,
        provided: &HashSet<Symbol>,
    ) -> Vec<EffectIR> {
        fields
            .iter()
            .filter(|(sym, _)| !provided.contains(*sym))
            .flat_map(|(sym, field_info)| {
                let name = self.ctx.analysis.interner.resolve(*sym);
                let member_ref = self
                    .lookup_member_in_scope(name)
                    .expect("union bubbling field must resolve in enclosing scope");
                let set = EffectIR::with_member(EffectKind::Set, member_ref);
                if self.field_defaults_to_empty_list(field_info) {
                    vec![EffectIR::start_arr(), EffectIR::end_arr(), set]
                } else {
                    vec![EffectIR::null(), set]
                }
            })
            .collect()
    }

    /// Defaults for every field of an enum variant's payload struct, with
    /// member refs built against the payload type itself. Empty for a
    /// tag-only variant (no payload struct).
    fn payload_default_effects(&self, payload_type_id: TypeId) -> Vec<EffectIR> {
        let Some(fields) = self.ctx.analysis.type_analysis.struct_fields(payload_type_id) else {
            return vec![];
        };
        fields
            .iter()
            .enumerate()
            .flat_map(|(idx, (_, field_info))| {
                let member_ref = MemberRef::new(payload_type_id, idx as u16);
                let set = EffectIR::with_member(EffectKind::Set, member_ref);
                if self.field_defaults_to_empty_list(field_info) {
                    vec![EffectIR::start_arr(), EffectIR::end_arr(), set]
                } else {
                    vec![EffectIR::null(), set]
                }
            })
            .collect()
    }

    /// A required list defaults to `[]`, everything else to `null`.
    fn field_defaults_to_empty_list(&self, field_info: &FieldInfo) -> bool {
        !field_info.optional
            && matches!(
                self.ctx
                    .analysis
                    .type_analysis
                    .expect_type_shape(field_info.type_id),
                TypeShape::Array { .. }
            )
    }

    /// A pure-effect epsilon for a lifted zero-width outcome: `pre` runs in
    /// the enclosing scope (opens + defaults), `post` closes it; the cursor
    /// stays untouched.
    fn emit_zero_width_step(
        &mut self,
        exit: Label,
        pre: Vec<EffectIR>,
        post: Vec<EffectIR>,
    ) -> Label {
        if pre.is_empty() && post.is_empty() {
            return exit;
        }
        let label = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(label, exit)
                .pre_effects(pre)
                .post_effects(post)
                .into(),
        );
        label
    }

    /// Enum alternation: each enum branch opens its variant scope
    /// (`EnumOpen`...`EnumClose`) and compiles its payload inside it.
    pub(super) fn compile_enum(&mut self, e: &ast::EnumPattern, ctx: PatternCtx) -> Label {
        let skip_exit = SkipExit::To(ctx.exit);
        self.compile_enum_with_exits(e, ctx, skip_exit)
    }

    /// [`compile_enum`](Self::compile_enum) with a distinct zero-width
    /// continuation. A nullable branch compiles pruned; its zero-width outcome
    /// is lifted to a per-branch alternative after the candidate search — the
    /// variant tags with every payload field at its default (see
    /// [`compile_union_branches`](Self::compile_union_branches)).
    pub(super) fn compile_enum_with_exits(
        &mut self,
        e: &ast::EnumPattern,
        ctx: PatternCtx,
        skip_exit: SkipExit,
    ) -> Label {
        let PatternCtx {
            exit,
            nav: first_nav,
            capture,
        } = ctx;
        let branches: Vec<_> = e.branches().collect();
        if branches.is_empty() {
            return exit;
        }

        let enum_type_id = self
            .ctx
            .analysis
            .type_analysis
            .expect_pattern_result(&Pattern::Enum(e.clone()))
            .flow
            .type_id()
            .expect("an analyzed enum must produce an enum type");

        // BTreeMap order gives stable variant indices independent of AST iteration order.
        let TypeShape::Enum(variants) = self
            .ctx
            .analysis
            .type_analysis
            .expect_type_shape(enum_type_id)
        else {
            panic!("an analyzed enum must produce an enum type");
        };
        let variant_info: BTreeMap<Symbol, (u16, TypeId)> = variants
            .iter()
            .enumerate()
            .map(|(idx, (&sym, &type_id))| (sym, (idx as u16, type_id)))
            .collect();

        let search_nav = resumable_search_nav(first_nav);
        let branch_search = AltSearchNav(search_nav);
        let branch_routing = self.alt_branch_routing(&branches, exit);

        let mut successors = Vec::new();
        let mut zero_width = Vec::new();
        for (branch_idx, branch) in branches.iter().enumerate() {
            let Some(body) = branch.body() else {
                continue;
            };

            let branch_exit = branch_routing.branch_exit(branch_idx, exit);

            let branch_nav =
                nav_for_alt_branch(first_nav, branch_search, &body, &self.anchor_semantics);

            let label = branch.label().expect("enum branch must have label");
            let (variant_idx, payload_type_id) = self
                .ctx
                .analysis
                .interner
                .get(label.text())
                .and_then(|sym| variant_info.get(&sym))
                .map(|&(idx, type_id)| (idx, type_id))
                .expect("variant must exist for enum branch");

            let e_effect = EffectIR::with_member(
                EffectKind::EnumOpen,
                MemberRef::new(enum_type_id, variant_idx),
            );

            let branch_nullable = self.pattern_is_nullable(&body);
            let body_entry = self.with_scope(payload_type_id, |this| {
                if branch_nullable {
                    let close_exit = this.emit_effects_epsilon(
                        branch_exit,
                        vec![EffectIR::end_enum()],
                        CaptureEffects::new_post(capture.post.clone()),
                    );
                    let inner_entry = this.compile_skippable_with_exits(
                        &body,
                        SplitExits {
                            match_exit: close_exit,
                            skip_exit: SkipExit::Fail,
                        },
                        branch_nav,
                        CaptureEffects::default(),
                    );
                    let mut entry_pre = capture.pre.clone();
                    entry_pre.push(e_effect.clone());
                    this.wrap_entry_pre(inner_entry, entry_pre)
                } else {
                    let branch_capture = capture
                        .clone()
                        .nest_scope(e_effect.clone(), EffectIR::end_enum());
                    this.dispatch_pattern(
                        &body,
                        PatternCtx {
                            exit: branch_exit,
                            nav: branch_nav,
                            capture: branch_capture,
                        },
                    )
                }
            });

            successors.push(body_entry);

            if branch_nullable && let SkipExit::To(skip) = skip_exit {
                let mut pre = capture.pre.clone();
                pre.push(e_effect);
                pre.extend(self.payload_default_effects(payload_type_id));
                let mut post = vec![EffectIR::end_enum()];
                post.extend(capture.post.iter().cloned());
                zero_width.push(self.emit_zero_width_step(skip, pre, post));
            }
        }

        self.assemble_alt_branches(successors, zero_width, search_nav, exit)
    }
}
