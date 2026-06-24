//! Debug-only structural verification of compiled IR.
//!
//! Two complementary, zero-cost-in-release checks guard the IR pipeline:
//!
//! 1. An **order-sensitive semantic fingerprint** of the graph reachable from an
//!    entry: the ordered list (DFS pre-order) of per-path hashes. For a
//!    backtracking VM transition *order* is semantics, so the fingerprint is
//!    sensitive to branch priority and to dropped/duplicated successors. Every
//!    optimization pass must preserve it — see [`run_verified`].
//! 2. **Structural invariants** on the instruction list: no duplicate labels and
//!    no dangling references (successors, `Call` targets/returns, `Trampoline`
//!    returns all resolve).
//!
//! The fingerprint normalizes away the two representation changes our passes make
//! legitimately:
//! - it sees through effect-free epsilons (pure control flow), so epsilon
//!   elimination is a no-op;
//! - it coalesces runs of same-mode `Up` navigation, summing their levels and
//!   ignoring interspersed effects (effects commute with pure navigation), so
//!   `collapse_up` — and epsilon elimination parking effects onto Up nodes — is a
//!   no-op.
//!
//! Anything else a pass does to navigation, matching, effects, or branch order
//! changes the fingerprint and trips the check.
//!
//! Cost is bounded: traversal stops after [`MAX_PATHS`] completed paths (a
//! semantic count, stable across passes) and cuts any single path past
//! [`MAX_DEPTH`] nodes (a backstop against pathological epsilon cycles, far above
//! any real path), so debug builds stay usable on large queries.

#[cfg(debug_assertions)]
pub use debug_impl::{run_verified, verify_constructed};

#[cfg(not(debug_assertions))]
pub use release_impl::{run_verified, verify_constructed};

#[cfg(not(debug_assertions))]
mod release_impl {
    use crate::compiler::lower::context::CompileCtx;
    use crate::compiler::lower::ir::CompileResult;

    /// Run a pass. Verification is compiled out in release builds.
    #[inline(always)]
    pub fn run_verified(
        _name: &str,
        result: &mut CompileResult,
        _ctx: &CompileCtx,
        pass: impl FnOnce(&mut CompileResult),
    ) {
        pass(result);
    }

    /// No-op in release builds.
    #[inline(always)]
    pub fn verify_constructed(_result: &CompileResult, _ctx: &CompileCtx) {}
}

#[cfg(debug_assertions)]
mod debug_impl {
    use std::collections::HashSet;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::num::NonZeroU16;

    use crate::bytecode::{EffectKind, Nav};
    use indexmap::IndexMap;

    use crate::compiler::ids::DefId;
    use crate::compiler::lower::context::CompileCtx;
    use crate::compiler::lower::ir::{
        CompileResult, InstructionIR, Label, MatchIR, NodeKindConstraint, PredicateValueIR,
    };

    /// Max completed paths recorded per fingerprint. This counts root-to-leaf
    /// walks, a *semantic* quantity (laser-vision makes it invariant to epsilon
    /// insertion), so truncating here stays consistent across passes.
    const MAX_PATHS: usize = 50_000;

    /// Max nodes along a single path before it is cut. A backstop against a
    /// pathological pure-epsilon cycle (which adds nodes without hitting cycle
    /// detection); far above any real path length.
    const MAX_DEPTH: u32 = 200_000;

    /// Number of paths materialized for the human-readable mismatch diagnostic.
    const DIAG_PATHS: usize = 400;

    /// One observable semantic effect along a path.
    ///
    /// Up navigation is recorded as [`SemanticOp::UpNav`] (mode tag, level) so that
    /// [`coalesce_ups`] can sum adjacent runs; all other navs are recorded verbatim.
    #[derive(Clone, PartialEq, Eq, Hash, Debug)]
    enum SemanticOp {
        Nav(Nav),
        /// Up navigation: (mode tag, level). See [`Nav::up_mode_tag`].
        UpNav(u8, u32),
        MatchNamed(Option<String>),
        MatchAnon(Option<String>),
        MatchAny,
        Field(String),
        NegField(String),
        Predicate(u8, String),
        Effect(EffectKind, Option<String>),
        Call(String),
        Return,
        /// Cycle back-reference detected during traversal.
        CycleRef,
        /// Label referenced but not present in the instruction list.
        DanglingLabel,
        /// Path cut at the depth limit.
        DepthCut,
    }

    type Path = Vec<SemanticOp>;

    /// Order-sensitive fingerprint: per-path hashes in DFS pre-order.
    #[derive(Clone)]
    pub struct Fingerprint {
        hashes: Vec<u64>,
        truncated: bool,
    }

    impl PartialEq for Fingerprint {
        fn eq(&self, other: &Self) -> bool {
            self.hashes == other.hashes && self.truncated == other.truncated
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum WalkRoot {
        Preamble,
        Def(DefId),
    }

    /// What one node contributes to the walk.
    struct WalkStep {
        /// Effect-free epsilon: contributes no ops and is not marked visited
        /// (laser vision through pure control flow).
        see_through: bool,
        ops: Vec<SemanticOp>,
        /// Continuation labels in priority order; empty means the path ends here.
        succs: Vec<Label>,
    }

    /// Collect the ordered ops a single match contributes.
    fn collect_match_ops(m: &MatchIR, ctx: &CompileCtx) -> Vec<SemanticOp> {
        let mut ops = Vec::new();

        // Member names aren't resolved at the IR fingerprint stage (that needs the
        // type table); the effect kind alone keys the fingerprint.
        for e in &m.pre_effects {
            ops.push(SemanticOp::Effect(e.kind(), None));
        }

        if m.nav != Nav::Epsilon {
            if let Some(mode) = m.nav.up_mode_tag() {
                ops.push(SemanticOp::UpNav(
                    mode,
                    m.nav
                        .up_level()
                        .expect("up_mode_tag returned Some, so nav is an Up variant with a level")
                        as u32,
                ));
            } else {
                ops.push(SemanticOp::Nav(m.nav));
            }

            match &m.node_kind {
                NodeKindConstraint::Any => ops.push(SemanticOp::MatchAny),
                NodeKindConstraint::Named(id) => {
                    let name = id.and_then(|i| ctx.grammar.kind_name(i, ctx.interner));
                    ops.push(SemanticOp::MatchNamed(name));
                }
                NodeKindConstraint::Anonymous(id) => {
                    let name = id.and_then(|i| ctx.grammar.kind_name(i, ctx.interner));
                    ops.push(SemanticOp::MatchAnon(name));
                }
            }
        }

        if let Some(f) = m.node_field {
            let name = ctx.grammar.field_name(f, ctx.interner);
            ops.push(SemanticOp::Field(
                name.unwrap_or_else(|| format!("field#{f}")),
            ));
        }

        for &f in &m.neg_fields {
            let name = NonZeroU16::new(f).and_then(|id| ctx.grammar.field_name(id, ctx.interner));
            ops.push(SemanticOp::NegField(
                name.unwrap_or_else(|| format!("field#{f}")),
            ));
        }

        if let Some(p) = &m.predicate {
            let value = resolve_predicate_value(&p.value);
            ops.push(SemanticOp::Predicate(p.op.to_byte(), value));
        }

        for e in &m.post_effects {
            ops.push(SemanticOp::Effect(e.kind(), None));
        }

        ops
    }

    /// Coalesce runs of same-mode Up navigation into a single summed `UpNav`.
    ///
    /// `Up(a)` then `Up(b)` of the same mode is `Up(a+b)`, and capture effects
    /// commute with pure navigation — so a later `UpNav,MatchAny` pair is folded
    /// into an earlier same-mode one even across interspersed `Effect`s, but not
    /// across a real navigation/match/call (which closes the run). This makes both
    /// `collapse_up` and epsilon-elimination's effect migration invisible to the
    /// fingerprint, while still catching a level that changes by any other means.
    fn coalesce_ups(ops: Vec<SemanticOp>) -> Vec<SemanticOp> {
        let mut out: Vec<SemanticOp> = Vec::with_capacity(ops.len());
        // (mode, index in `out`) of the most recent Up run still open for merging.
        let mut pending: Option<(u8, usize)> = None;
        let mut iter = ops.into_iter().peekable();

        while let Some(op) = iter.next() {
            // An Up node is always `UpNav` immediately followed by `MatchAny`
            // (pure navigation checks no node kind). Only such pairs coalesce.
            let up_pair = match op {
                SemanticOp::UpNav(mode, level)
                    if matches!(iter.peek(), Some(SemanticOp::MatchAny)) =>
                {
                    Some((mode, level))
                }
                _ => None,
            };

            if let Some((mode, level)) = up_pair {
                iter.next();
                let merged = matches!(pending, Some((pm, _)) if pm == mode);
                if merged {
                    let (_, idx) = pending.expect("merged is true only when pending is Some");
                    if let SemanticOp::UpNav(_, plevel) = &mut out[idx] {
                        *plevel = plevel.saturating_add(level);
                    }
                } else {
                    out.push(SemanticOp::UpNav(mode, level));
                    pending = Some((mode, out.len() - 1));
                    out.push(SemanticOp::MatchAny);
                }
            } else if matches!(op, SemanticOp::Effect(..)) {
                out.push(op);
            } else {
                out.push(op);
                pending = None;
            }
        }

        out
    }

    fn hash_path(path: &Path) -> u64 {
        let mut h = DefaultHasher::new();
        path.hash(&mut h);
        h.finish()
    }

    /// Compute one node's contribution. Cycle detection is the walker's job (it
    /// depends on traversal state), so this is a pure function of the graph.
    fn node_step(
        label: Label,
        instr_map: &std::collections::HashMap<Label, &InstructionIR>,
        label_to_def: &std::collections::HashMap<Label, DefId>,
        ctx: &CompileCtx,
    ) -> WalkStep {
        let Some(&instr) = instr_map.get(&label) else {
            return WalkStep {
                see_through: false,
                ops: vec![SemanticOp::DanglingLabel],
                succs: vec![],
            };
        };

        match instr {
            InstructionIR::Match(m) => {
                if m.is_epsilon() && m.pre_effects.is_empty() && m.post_effects.is_empty() {
                    return WalkStep {
                        see_through: true,
                        ops: vec![],
                        succs: m.successors.clone(),
                    };
                }
                WalkStep {
                    see_through: false,
                    ops: collect_match_ops(m, ctx),
                    succs: m.successors.clone(),
                }
            }
            InstructionIR::Call(c) => {
                // Record the callee by stable DefId (label-rename invariant); follow
                // the return continuation rather than descending into the callee.
                let name = label_to_def
                    .get(&c.target)
                    .map(|def_id| format!("def#{}", def_id.as_u32()))
                    .unwrap_or_else(|| format!("label#{}", c.target.0));
                WalkStep {
                    see_through: false,
                    ops: vec![SemanticOp::Call(name)],
                    succs: vec![c.next],
                }
            }
            InstructionIR::Return(_) => WalkStep {
                see_through: false,
                ops: vec![SemanticOp::Return],
                succs: vec![],
            },
            InstructionIR::Trampoline(t) => WalkStep {
                // Part of the preamble; semantically transparent but still marked
                // visited so a (hypothetical) cycle through it terminates.
                see_through: false,
                ops: vec![],
                succs: vec![t.next],
            },
        }
    }

    /// Iterative DFS over the graph reachable from `entry`, invoking `on_path` with
    /// each completed path (coalesced) in pre-order. An explicit stack (no
    /// recursion, so deep IRs can't overflow) carries per-branch op prefixes and
    /// visited snapshots. Returns whether traversal was truncated by a budget.
    fn walk(
        entry: Label,
        instr_map: &std::collections::HashMap<Label, &InstructionIR>,
        label_to_def: &std::collections::HashMap<Label, DefId>,
        ctx: &CompileCtx,
        max_paths: usize,
        mut on_path: impl FnMut(Path),
    ) -> bool {
        let mut count = 0usize;
        let mut truncated = false;

        // (label, ops-so-far, visited set, node depth)
        let mut stack: Vec<(Label, Vec<SemanticOp>, HashSet<Label>, u32)> =
            vec![(entry, Vec::new(), HashSet::new(), 0)];

        while let Some((label, mut ops, mut visited, depth)) = stack.pop() {
            if count >= max_paths {
                truncated = true;
                break;
            }
            if depth >= MAX_DEPTH {
                ops.push(SemanticOp::DepthCut);
                on_path(coalesce_ups(ops));
                count += 1;
                truncated = true;
                continue;
            }
            if visited.contains(&label) {
                ops.push(SemanticOp::CycleRef);
                on_path(coalesce_ups(ops));
                count += 1;
                continue;
            }

            let walk_step = node_step(label, instr_map, label_to_def, ctx);

            if walk_step.see_through {
                // Reversed pushes so successors pop in priority order (pre-order).
                for &succ in walk_step.succs.iter().rev() {
                    stack.push((succ, ops.clone(), visited.clone(), depth + 1));
                }
                continue;
            }

            visited.insert(label);
            ops.extend(walk_step.ops);

            if walk_step.succs.is_empty() {
                on_path(coalesce_ups(ops));
                count += 1;
            } else {
                for &succ in walk_step.succs.iter().rev() {
                    stack.push((succ, ops.clone(), visited.clone(), depth + 1));
                }
            }
        }

        truncated
    }

    fn build_instr_map(
        instructions: &[InstructionIR],
    ) -> std::collections::HashMap<Label, &InstructionIR> {
        instructions.iter().map(|i| (i.label(), i)).collect()
    }

    fn label_to_def_map(
        def_entries: &IndexMap<DefId, Label>,
    ) -> std::collections::HashMap<Label, DefId> {
        def_entries.iter().map(|(&d, &l)| (l, d)).collect()
    }

    fn fingerprint_from_ir(
        instructions: &[InstructionIR],
        entry: Label,
        def_entries: &IndexMap<DefId, Label>,
        ctx: &CompileCtx,
    ) -> Fingerprint {
        let instr_map = build_instr_map(instructions);
        let label_to_def = label_to_def_map(def_entries);
        let mut hashes = Vec::new();
        let truncated = walk(entry, &instr_map, &label_to_def, ctx, MAX_PATHS, |path| {
            hashes.push(hash_path(&path));
        });
        Fingerprint { hashes, truncated }
    }

    fn paths_from_ir(
        instructions: &[InstructionIR],
        entry: Label,
        def_entries: &IndexMap<DefId, Label>,
        ctx: &CompileCtx,
        max: usize,
    ) -> Vec<Path> {
        let instr_map = build_instr_map(instructions);
        let label_to_def = label_to_def_map(def_entries);
        let mut paths = Vec::new();
        walk(entry, &instr_map, &label_to_def, ctx, max, |path| {
            paths.push(path);
        });
        paths
    }

    /// The role an effect plays in value-scope nesting: it opens a scope, closes
    /// the scope a specific kind opened, or is scope-neutral. The match is
    /// exhaustive on purpose — a new `EffectKind` cannot compile until it
    /// declares its scope behaviour here.
    enum ScopeRole {
        Open,
        Close(EffectKind),
    }

    fn scope_role(op: EffectKind) -> Option<ScopeRole> {
        use EffectKind::*;
        let role = match op {
            ArrayOpen | StructOpen | EnumOpen | SuppressBegin => ScopeRole::Open,
            ArrayClose => ScopeRole::Close(ArrayOpen),
            StructClose => ScopeRole::Close(StructOpen),
            EnumClose => ScopeRole::Close(EnumOpen),
            SuppressEnd => ScopeRole::Close(SuppressBegin),
            Node | Push | Set | Null => return None,
        };
        Some(role)
    }

    /// A single path's scope effects must nest like brackets: every close matches
    /// the innermost open, none underflows, and a *completed* path leaves nothing
    /// open. Paths cut by a cycle/depth marker are partial, so their leftover opens
    /// are expected and not flagged.
    fn check_path_scopes(path: &Path) -> Result<(), String> {
        let mut stack: Vec<EffectKind> = Vec::new();
        for op in path {
            let SemanticOp::Effect(opcode, _) = op else {
                continue;
            };
            let opcode = *opcode;
            match scope_role(opcode) {
                None => {}
                Some(ScopeRole::Open) => stack.push(opcode),
                Some(ScopeRole::Close(expected)) => match stack.pop() {
                    Some(top) if top == expected => {}
                    Some(top) => {
                        return Err(format!(
                            "{opcode:?} closes a {expected:?} scope but the innermost open scope is {top:?}"
                        ));
                    }
                    None => {
                        return Err(format!("{opcode:?} has no matching open scope"));
                    }
                },
            }
        }

        let truncated = matches!(
            path.last(),
            Some(SemanticOp::CycleRef | SemanticOp::DanglingLabel | SemanticOp::DepthCut)
        );
        if !truncated && !stack.is_empty() {
            return Err(format!("path ends with unclosed scope(s): {stack:?}"));
        }
        Ok(())
    }

    fn check_scopes(
        instructions: &[InstructionIR],
        entry: Label,
        def_entries: &IndexMap<DefId, Label>,
        ctx: &CompileCtx,
    ) -> Result<(), String> {
        let instr_map = build_instr_map(instructions);
        let label_to_def = label_to_def_map(def_entries);
        let mut err: Option<String> = None;
        walk(entry, &instr_map, &label_to_def, ctx, MAX_PATHS, |path| {
            if err.is_none()
                && let Err(e) = check_path_scopes(&path)
            {
                err = Some(format!("{e}\n  path: {path:?}"));
            }
        });
        match err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Structural invariants independent of any entry point: every label is unique
    /// and every reference resolves to a real instruction.
    fn check_labels(instructions: &[InstructionIR]) -> Result<(), String> {
        let mut index = std::collections::HashMap::with_capacity(instructions.len());
        for (i, instr) in instructions.iter().enumerate() {
            if let Some(prev) = index.insert(instr.label(), i) {
                return Err(format!(
                    "duplicate label {:?} (instructions[{prev}] and [{i}])",
                    instr.label()
                ));
            }
        }

        for instr in instructions {
            for &succ in instr.successors() {
                if !index.contains_key(&succ) {
                    return Err(format!(
                        "dangling successor {:?} referenced by {:?}",
                        succ,
                        instr.label()
                    ));
                }
            }
            // `successors()` omits a Call's target — check it explicitly.
            if let InstructionIR::Call(c) = instr
                && !index.contains_key(&c.target)
            {
                return Err(format!(
                    "dangling call target {:?} referenced by {:?}",
                    c.target, c.label
                ));
            }
        }

        Ok(())
    }

    fn entries(result: &CompileResult) -> Vec<(WalkRoot, Label)> {
        let mut v = vec![(WalkRoot::Preamble, result.preamble_entry)];
        for (&def_id, &label) in &result.def_entries {
            v.push((WalkRoot::Def(def_id), label));
        }
        v
    }

    fn snapshot(result: &CompileResult, ctx: &CompileCtx) -> Vec<(WalkRoot, Label, Fingerprint)> {
        entries(result)
            .into_iter()
            .map(|(key, entry)| {
                let fp = fingerprint_from_ir(&result.instructions, entry, &result.def_entries, ctx);
                (key, entry, fp)
            })
            .collect()
    }

    fn diff_paths(before: &[Path], after: &[Path]) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let _ = writeln!(
            s,
            "  before: {} path(s), after: {} path(s) (diagnostic capped at {DIAG_PATHS})",
            before.len(),
            after.len()
        );
        let n = before.len().max(after.len());
        for i in 0..n {
            if before.get(i) != after.get(i) {
                let _ = writeln!(s, "  first divergence at path #{i}:");
                let _ = writeln!(s, "    before: {:?}", before.get(i));
                let _ = writeln!(s, "    after:  {:?}", after.get(i));
                return s;
            }
        }
        s
    }

    fn verify_after_pass(
        name: &str,
        before_instrs: &[InstructionIR],
        before: &[(WalkRoot, Label, Fingerprint)],
        result: &CompileResult,
        ctx: &CompileCtx,
    ) {
        if let Err(e) = check_labels(&result.instructions) {
            panic!("[verify] pass `{name}` produced malformed IR: {e}");
        }

        for (key, entry, before_fp) in before {
            let after_fp =
                fingerprint_from_ir(&result.instructions, *entry, &result.def_entries, ctx);
            if *before_fp != after_fp {
                let before_paths =
                    paths_from_ir(before_instrs, *entry, &result.def_entries, ctx, DIAG_PATHS);
                let after_paths = paths_from_ir(
                    &result.instructions,
                    *entry,
                    &result.def_entries,
                    ctx,
                    DIAG_PATHS,
                );
                panic!(
                    "[verify] pass `{name}` changed semantics for {key:?}:\n{}",
                    diff_paths(&before_paths, &after_paths)
                );
            }
        }
    }

    /// Run `pass`, then assert it preserved every entry's fingerprint and left the
    /// instruction list structurally sound.
    pub fn run_verified(
        name: &str,
        result: &mut CompileResult,
        ctx: &CompileCtx,
        pass: impl FnOnce(&mut CompileResult),
    ) {
        let before_instrs = result.instructions.clone();
        let before = snapshot(result, ctx);
        pass(result);
        verify_after_pass(name, &before_instrs, &before, result, ctx);
    }

    /// Check the freshly-constructed IR before any pass runs: structural soundness
    /// plus balanced scope effects on every path. Passes preserve the fingerprint
    /// (which carries the full effect sequence), so a construction that balances
    /// here stays balanced through the pipeline.
    pub fn verify_constructed(result: &CompileResult, ctx: &CompileCtx) {
        if let Err(e) = check_labels(&result.instructions) {
            panic!("[verify] construction produced malformed IR: {e}");
        }
        for (key, entry) in entries(result) {
            if let Err(e) = check_scopes(&result.instructions, entry, &result.def_entries, ctx) {
                panic!("[verify] construction produced unbalanced scope effects for {key:?}:\n{e}");
            }
        }
    }

    fn resolve_predicate_value(value: &PredicateValueIR) -> String {
        match value {
            PredicateValueIR::String(text) => text.to_string(),
            PredicateValueIR::Regex(text) => format!("/{text}/"),
        }
    }
}
