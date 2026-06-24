use std::collections::{BTreeMap, HashSet};

use crate::bytecode::{EffectKind, Nav};
use crate::compiler::analyze::types::TypeShape;
use crate::compiler::ids::TypeId;
use crate::compiler::lower::ir::{EffectIR, InstructionIR, Label, MemberRef, NodeKindConstraint};
use crate::compiler::parse::ast::{self, Pattern};
use crate::core::Symbol;

use super::NfaBuilder;
use super::capture::{CaptureEffects, PatternCtx};
use super::navigation::{
    AnonymousClassifier, expr_owns_iteration, is_skippable_quantifier, resumable_search_nav,
};

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
    classifier: &AnonymousClassifier<'_>,
) -> Option<Nav> {
    let nav = exact_nav_for_alt_branch(first_nav, search_nav)?;

    if !classifier.expr_may_match_anonymous(Some(body)) {
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
    /// Returns `None` — caller stays conservative — unless the follower's entry is
    /// a single `Match` carrying `NextSkipExtras` on a `Named` node, the one shape
    /// where the upgrade is both safe and needed. Anonymous/`_` followers are
    /// intrinsically extras-only; refs (`Call`) and scope-wrapped followers
    /// (epsilon entry) carry no such instruction and are left for follow-up.
    fn clone_named_follower_skip_entry(&mut self, exit: Label) -> Option<Label> {
        let mut twin = {
            let InstructionIR::Match(m) = self.instructions.iter().find(|i| i.label() == exit)?
            else {
                return None;
            };
            if m.nav != Nav::NextSkipExtras || !matches!(m.node_kind, NodeKindConstraint::Named(_))
            {
                return None;
            }
            m.clone()
        };

        twin.label = self.fresh_label();
        twin.nav = Nav::NextSkip;
        let label = twin.label;
        self.instructions.push(twin.into());
        Some(label)
    }

    /// Per-branch "named" flags plus the soft-skip follower twin — shared by both
    /// alternation kinds. A branch is "named" (eligible for the twin) when it cannot
    /// match an anonymous node and does not own its own iteration. A quantified
    /// branch's zero-match path leaves no named node on the anchor's left, so the
    /// soft-skip upgrade is unsound there. The anonymity test is whole-branch,
    /// matching `nav_for_alt_branch`'s before-anchor classification. The twin is a
    /// `NextSkip` clone of a conservative (`NextSkipExtras`) soft follower, worth
    /// cloning only when at least one branch is itself named.
    fn alt_branch_routing(
        &mut self,
        branches: &[ast::Branch],
        exit: Label,
        classifier: &AnonymousClassifier,
    ) -> BranchRouting {
        let branch_named: Vec<bool> = branches
            .iter()
            .map(|b| {
                b.body().is_some_and(|body| {
                    !expr_owns_iteration(&body) && !classifier.expr_may_match_anonymous(Some(&body))
                })
            })
            .collect();

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
    fn assemble_alt_branches(
        &mut self,
        successors: Vec<Label>,
        search_nav: Option<Nav>,
        exit: Label,
    ) -> Label {
        if successors.is_empty() {
            return exit;
        }

        let alt_entry = if successors.len() == 1 {
            successors[0]
        } else {
            let entry = self.fresh_label();
            self.emit_epsilon(entry, successors);
            entry
        };

        if let Some(nav) = search_nav {
            return self.emit_position_search(nav, alt_entry);
        }

        alt_entry
    }

    /// Union alternation: each branch merges into one struct.
    pub(super) fn compile_union(&mut self, union: &ast::UnionPattern, ctx: PatternCtx) -> Label {
        let PatternCtx {
            exit,
            nav: first_nav,
            capture,
        } = ctx;
        let branches: Vec<_> = union.branches().collect();
        if branches.is_empty() {
            return exit;
        }

        let union_type_id = self
            .ctx
            .type_ctx
            .expect_pattern_result(&Pattern::Union(union.clone()))
            .flow
            .type_id();
        let merged_fields = union_type_id.map(|id| self.ctx.type_ctx.expect_struct_fields(id));

        let search_nav = resumable_search_nav(first_nav);
        let branch_search = AltSearchNav(search_nav);
        let classifier = AnonymousClassifier::new(self.ctx.symbol_table);
        let branch_routing = self.alt_branch_routing(&branches, exit, &classifier);

        let mut successors = Vec::new();
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
                let provided: HashSet<Symbol> = self
                    .ctx
                    .type_ctx
                    .expect_pattern_result(&body)
                    .flow
                    .type_id()
                    .map(|id| self.ctx.type_ctx.expect_struct_fields(id))
                    .map(|f| f.keys().copied().collect())
                    .unwrap_or_default();
                fields
                    .iter()
                    .filter(|(sym, _)| !provided.contains(*sym))
                    .flat_map(|(sym, field_info)| {
                        let name = self.ctx.interner.resolve(*sym);
                        let member_ref = self
                            .lookup_member_in_scope(name)
                            .expect("union bubbling field must resolve in enclosing scope");
                        let set = EffectIR::with_member(EffectKind::Set, member_ref);
                        let is_required_list = !field_info.optional
                            && matches!(
                                self.ctx.type_ctx.expect_type_shape(field_info.type_id),
                                TypeShape::Array { .. }
                            );
                        if is_required_list {
                            vec![EffectIR::start_arr(), EffectIR::end_arr(), set]
                        } else {
                            vec![EffectIR::null(), set]
                        }
                    })
                    .collect()
            } else {
                vec![]
            };

            let branch_nav = nav_for_alt_branch(first_nav, branch_search, &body, &classifier);
            let branch_entry = if is_skippable_quantifier(&body) {
                let exit = if capture.post.is_empty() {
                    branch_exit
                } else {
                    self.emit_effects_epsilon(
                        branch_exit,
                        vec![],
                        CaptureEffects::new_post(capture.post.clone()),
                    )
                };
                let entry = self.dispatch_pattern(
                    &body,
                    PatternCtx {
                        exit,
                        nav: branch_nav,
                        capture: CaptureEffects::default(),
                    },
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

        self.assemble_alt_branches(successors, search_nav, exit)
    }

    /// Enum alternation: each enum branch opens its variant scope
    /// (`EnumOpen`...`EnumClose`) and compiles its payload inside it.
    pub(super) fn compile_enum(&mut self, e: &ast::EnumPattern, ctx: PatternCtx) -> Label {
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
            .type_ctx
            .expect_pattern_result(&Pattern::Enum(e.clone()))
            .flow
            .type_id()
            .expect("an analyzed enum must produce an enum type");

        // BTreeMap order gives stable variant indices independent of AST iteration order.
        let TypeShape::Enum(variants) = self.ctx.type_ctx.expect_type_shape(enum_type_id) else {
            panic!("an analyzed enum must produce an enum type");
        };
        let variant_info: BTreeMap<Symbol, (u16, TypeId)> = variants
            .iter()
            .enumerate()
            .map(|(idx, (&sym, &type_id))| (sym, (idx as u16, type_id)))
            .collect();

        let search_nav = resumable_search_nav(first_nav);
        let branch_search = AltSearchNav(search_nav);
        let classifier = AnonymousClassifier::new(self.ctx.symbol_table);
        let branch_routing = self.alt_branch_routing(&branches, exit, &classifier);

        let mut successors = Vec::new();
        for (branch_idx, branch) in branches.iter().enumerate() {
            let Some(body) = branch.body() else {
                continue;
            };

            let branch_exit = branch_routing.branch_exit(branch_idx, exit);

            let branch_nav = nav_for_alt_branch(first_nav, branch_search, &body, &classifier);

            let label = branch.label().expect("enum branch must have label");
            let (variant_idx, payload_type_id) = self
                .ctx
                .interner
                .get(label.text())
                .and_then(|sym| variant_info.get(&sym))
                .map(|&(idx, type_id)| (idx, type_id))
                .expect("variant must exist for enum branch");

            let e_effect = EffectIR::with_member(
                EffectKind::EnumOpen,
                MemberRef::new(enum_type_id, variant_idx),
            );

            let body_entry = self.with_scope(payload_type_id, |this| {
                if is_skippable_quantifier(&body) {
                    let close_exit = this.emit_effects_epsilon(
                        branch_exit,
                        vec![EffectIR::end_enum()],
                        CaptureEffects::new_post(capture.post.clone()),
                    );
                    let inner_entry = this.dispatch_pattern(
                        &body,
                        PatternCtx {
                            exit: close_exit,
                            nav: branch_nav,
                            capture: CaptureEffects::default(),
                        },
                    );
                    let mut entry_pre = capture.pre.clone();
                    entry_pre.push(e_effect);
                    this.wrap_entry_pre(inner_entry, entry_pre)
                } else {
                    let branch_capture = capture.clone().nest_scope(e_effect, EffectIR::end_enum());
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
        }

        self.assemble_alt_branches(successors, search_nav, exit)
    }
}
