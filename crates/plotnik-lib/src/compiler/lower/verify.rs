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
//!    no dangling references (successors and `Call` targets/returns all resolve).
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
pub use debug_impl::{
    run_root_pruning_verified, run_verified, verify_constructed, verify_fresh_build,
};

#[cfg(not(debug_assertions))]
pub use release_impl::{
    run_root_pruning_verified, run_verified, verify_constructed, verify_fresh_build,
};

#[cfg(all(test, debug_assertions))]
#[path = "verify_tests.rs"]
mod verify_tests;

#[cfg(not(debug_assertions))]
mod release_impl {
    use crate::compiler::lower::LowerInput;
    use crate::compiler::lower::ir::{InstructionIR, NfaGraph};

    /// Run a pass. Verification is compiled out in release builds.
    #[inline(always)]
    pub fn run_verified(
        _name: &str,
        nfa: &mut NfaGraph,
        _ctx: &LowerInput,
        pass: impl FnOnce(&mut NfaGraph),
    ) {
        pass(nfa);
    }

    /// Run a pass that may intentionally prune internal definition roots.
    /// Verification is compiled out in release builds.
    #[inline(always)]
    pub fn run_root_pruning_verified(
        _name: &str,
        nfa: &mut NfaGraph,
        _ctx: &LowerInput,
        pass: impl FnOnce(&mut NfaGraph),
    ) {
        pass(nfa);
    }

    /// No-op in release builds.
    #[inline(always)]
    pub fn verify_constructed(_nfa: &NfaGraph, _ctx: &LowerInput) {}

    /// No-op in release builds.
    #[inline(always)]
    pub fn verify_fresh_build(_instructions: &[InstructionIR]) {}
}

#[cfg(debug_assertions)]
mod debug_impl {
    use std::collections::hash_map::DefaultHasher;
    use std::collections::{HashMap, HashSet};
    use std::hash::{Hash, Hasher};

    use crate::bytecode::{EffectKind, Nav};
    use indexmap::IndexMap;

    use crate::compiler::ids::DefId;
    use crate::compiler::lower::LowerInput;
    use crate::compiler::lower::ir::{
        DefRoute, DefVariant, InstructionIR, Label, MatchIR, NfaGraph, NodeKindConstraint,
        PredicateValueIR, ReturnOutcome,
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
        Return(crate::compiler::lower::ir::ReturnOutcome),
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

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum WalkRoot {
        Entrypoint(DefId),
        Def(DefVariant),
    }

    struct PassSnapshot {
        instructions: Vec<InstructionIR>,
        fingerprints: Vec<(WalkRoot, Label, Fingerprint)>,
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
    fn collect_match_ops(m: &MatchIR, ctx: &LowerInput) -> Vec<SemanticOp> {
        let mut ops = Vec::new();

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
                    let name =
                        id.and_then(|i| ctx.analysis.grammar.kind_name(i, ctx.analysis.interner));
                    ops.push(SemanticOp::MatchNamed(name));
                }
                NodeKindConstraint::Anonymous(id) => {
                    let name =
                        id.and_then(|i| ctx.analysis.grammar.kind_name(i, ctx.analysis.interner));
                    ops.push(SemanticOp::MatchAnon(name));
                }
            }
        }

        if let Some(f) = m.node_field {
            let name = ctx.analysis.grammar.field_name(f, ctx.analysis.interner);
            ops.push(SemanticOp::Field(
                name.unwrap_or_else(|| format!("field#{f}")),
            ));
        }

        for &f in &m.neg_fields {
            let name = ctx.analysis.grammar.field_name(f, ctx.analysis.interner);
            ops.push(SemanticOp::NegField(
                name.unwrap_or_else(|| format!("field#{f}")),
            ));
        }

        if let Some(p) = &m.predicate {
            let value = resolve_predicate_value(&p.value);
            ops.push(SemanticOp::Predicate(p.op.to_byte(), value));
        }

        // Member names aren't resolved at the IR fingerprint stage (that needs the
        // type table); the effect kind alone keys the fingerprint.
        for e in &m.effects {
            ops.push(SemanticOp::Effect(e.kind(), None));
        }

        ops
    }

    fn normalize_commuting_effects(ops: Vec<SemanticOp>) -> Vec<SemanticOp> {
        fn flush(
            out: &mut Vec<SemanticOp>,
            effects: &mut Vec<SemanticOp>,
            others: &mut Vec<SemanticOp>,
        ) {
            out.append(effects);
            out.append(others);
        }

        let mut out = Vec::with_capacity(ops.len());
        let mut effects = Vec::new();
        let mut others = Vec::new();

        for op in ops {
            match op {
                SemanticOp::Effect(kind, _) if kind.reads_cursor() => {
                    flush(&mut out, &mut effects, &mut others);
                    out.push(op);
                }
                SemanticOp::Call(_)
                | SemanticOp::Return(_)
                | SemanticOp::CycleRef
                | SemanticOp::DanglingLabel
                | SemanticOp::DepthCut => {
                    flush(&mut out, &mut effects, &mut others);
                    out.push(op);
                }
                SemanticOp::Effect(..) => effects.push(op),
                _ => others.push(op),
            }
        }

        flush(&mut out, &mut effects, &mut others);
        out
    }

    fn normalize_path(ops: Vec<SemanticOp>) -> Vec<SemanticOp> {
        coalesce_ups(normalize_commuting_effects(ops))
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
    struct GraphWalk<'a> {
        instr_map: HashMap<Label, &'a InstructionIR>,
        label_to_def: HashMap<Label, DefId>,
        ctx: &'a LowerInput<'a>,
    }

    impl<'a> GraphWalk<'a> {
        fn new(
            instructions: &'a [InstructionIR],
            def_entries: &IndexMap<DefVariant, Label>,
            ctx: &'a LowerInput<'a>,
        ) -> Self {
            Self {
                instr_map: instructions.iter().map(|i| (i.label(), i)).collect(),
                label_to_def: def_entries
                    .iter()
                    .map(|(variant, &label)| (label, variant.def_id()))
                    .collect(),
                ctx,
            }
        }

        /// Compute one node's contribution. Cycle detection is the walker's job (it
        /// depends on traversal state), so this is a pure function of the graph.
        fn node_step(&self, label: Label) -> WalkStep {
            let Some(&instr) = self.instr_map.get(&label) else {
                return WalkStep {
                    see_through: false,
                    ops: vec![SemanticOp::DanglingLabel],
                    succs: vec![],
                };
            };

            match instr {
                InstructionIR::Match(m) => {
                    if m.is_epsilon() && m.effects.is_empty() {
                        return WalkStep {
                            see_through: true,
                            ops: vec![],
                            succs: m.successors.clone(),
                        };
                    }
                    WalkStep {
                        see_through: false,
                        ops: collect_match_ops(m, self.ctx),
                        succs: m.successors.clone(),
                    }
                }
                InstructionIR::Call(c) => {
                    // Record the callee by stable DefId (label-rename invariant); follow
                    // the return continuation rather than descending into the callee.
                    let name = self
                        .label_to_def
                        .get(&c.target)
                        .map(|def_id| format!("def#{}", def_id.index()))
                        .unwrap_or_else(|| format!("label#{}", c.target.0));
                    WalkStep {
                        see_through: false,
                        ops: vec![SemanticOp::Call(name)],
                        succs: c.return_labels().to_vec(),
                    }
                }
                InstructionIR::Return(return_) => WalkStep {
                    see_through: false,
                    ops: vec![SemanticOp::Return(return_.outcome())],
                    succs: vec![],
                },
            }
        }

        /// Iterative DFS over the graph reachable from `entry`, invoking `on_path` with
        /// each completed path (coalesced) in pre-order. An explicit stack (no
        /// recursion, so deep IRs can't overflow) carries per-branch op prefixes and
        /// visited snapshots. Returns whether traversal was truncated by a budget.
        fn walk(&self, entry: Label, max_paths: usize, mut on_path: impl FnMut(Path)) -> bool {
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
                    on_path(normalize_path(ops));
                    count += 1;
                    truncated = true;
                    continue;
                }
                if visited.contains(&label) {
                    ops.push(SemanticOp::CycleRef);
                    on_path(normalize_path(ops));
                    count += 1;
                    continue;
                }

                let walk_step = self.node_step(label);

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
                    on_path(normalize_path(ops));
                    count += 1;
                    continue;
                }

                for &succ in walk_step.succs.iter().rev() {
                    stack.push((succ, ops.clone(), visited.clone(), depth + 1));
                }
            }

            truncated
        }

        fn fingerprint(&self, entry: Label) -> Fingerprint {
            let mut hashes = Vec::new();
            let truncated = self.walk(entry, MAX_PATHS, |path| {
                hashes.push(hash_path(&path));
            });
            Fingerprint { hashes, truncated }
        }

        fn paths(&self, entry: Label, max: usize) -> Vec<Path> {
            let mut paths = Vec::new();
            self.walk(entry, max, |path| {
                paths.push(path);
            });
            paths
        }

        fn check_scopes(&self, entry: Label) -> Result<(), String> {
            let mut err: Option<String> = None;
            self.walk(entry, MAX_PATHS, |path| {
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
    }

    /// The role an effect plays in value-scope nesting: it opens a scope, closes
    /// the scope a specific kind opened, or is scope-neutral. The match is
    /// exhaustive on purpose — a new `EffectKind` cannot compile until it
    /// declares its scope behaviour here.
    enum ScopeRole {
        Open(EffectKind),
        Close(EffectKind),
    }

    fn scope_role(op: EffectKind) -> Option<ScopeRole> {
        use EffectKind::*;
        let role = match op {
            ListOpen | RecordOpen | VariantOpen | SuppressBegin | ScalarOpen => ScopeRole::Open(op),
            ListClose => ScopeRole::Close(ListOpen),
            RecordClose => ScopeRole::Close(RecordOpen),
            VariantClose => ScopeRole::Close(VariantOpen),
            SuppressEnd => ScopeRole::Close(SuppressBegin),
            StrClose | BoolClose => ScopeRole::Close(ScalarOpen),
            Node | ArrayPush | RecordSet | Absent | ScalarMark | NodeStr | NodeBool | BoolValue
            | SpanStartAt | SpanStart | SpanEnd => {
                return None;
            }
        };
        Some(role)
    }

    /// A single path's scope effects must nest like brackets: every close matches
    /// the innermost open, none underflows, and a *completed* path leaves nothing
    /// open. Paths cut by a cycle/depth marker are partial, so their leftover opens
    /// are expected and not flagged.
    fn check_path_scopes(path: &Path) -> Result<(), String> {
        let mut stack: Vec<EffectKind> = Vec::new();
        let mut spans: Vec<Option<String>> = Vec::new();
        for op in path {
            let SemanticOp::Effect(opcode, payload) = op else {
                continue;
            };
            let opcode = *opcode;
            if matches!(opcode, EffectKind::SpanStartAt | EffectKind::SpanStart) {
                spans.push(payload.clone());
                continue;
            }
            if opcode == EffectKind::SpanEnd {
                match spans.pop() {
                    Some(open) if open == *payload => {}
                    Some(open) => {
                        return Err(format!(
                            "SpanEnd({payload:?}) closes a span but the innermost open span is {open:?}"
                        ));
                    }
                    None => return Err("SpanEnd has no matching open span".to_string()),
                }
                continue;
            }
            match scope_role(opcode) {
                None => {}
                Some(ScopeRole::Open(open)) => stack.push(open),
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
        if !truncated && !spans.is_empty() {
            return Err(format!("path ends with unclosed span(s): {spans:?}"));
        }
        Ok(())
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

    /// Every body must return or accept at the same cursor depth it entered with.
    ///
    /// Calls apply their own navigation at the call site, but the callee is a
    /// separately checked body.
    fn check_depth_neutrality(nfa: &NfaGraph) -> Result<(), String> {
        let instr_map: HashMap<Label, &InstructionIR> =
            nfa.instructions.iter().map(|i| (i.label(), i)).collect();

        for (root, entry) in entries(nfa) {
            let route = match &root {
                WalkRoot::Entrypoint(_) => DefRoute::Caller,
                WalkRoot::Def(variant) => variant.route(),
            };
            check_depth_root(root, entry, route, &instr_map)?;
        }
        Ok(())
    }

    /// Every compiled `Node` effect must run after some real match has
    /// consumed a candidate. This is only a compiler IR check: once the VM's
    /// `Node` effect reads the cursor directly, malformed bytecode can still
    /// produce wrong values, but it cannot reach undefined state.
    fn check_no_node_on_zero_width_paths(nfa: &NfaGraph) -> Result<(), String> {
        let instr_map: HashMap<Label, &InstructionIR> =
            nfa.instructions.iter().map(|i| (i.label(), i)).collect();

        for (root, entry) in entries(nfa) {
            check_zero_width_root(root, entry, &instr_map)?;
        }
        Ok(())
    }

    fn check_span_start_at_placement(instructions: &[InstructionIR]) -> Result<(), String> {
        for instr in instructions {
            let InstructionIR::Match(m) = instr else {
                continue;
            };
            if m.effects
                .iter()
                .any(|effect| effect.kind() == EffectKind::SpanStartAt)
                && m.is_epsilon()
            {
                return Err(format!(
                    "SpanStartAt emitted on epsilon match {:?}; it must start on a consuming match",
                    m.label
                ));
            }
        }

        Ok(())
    }

    fn check_zero_width_root(
        root: WalkRoot,
        entry: Label,
        instr_map: &HashMap<Label, &InstructionIR>,
    ) -> Result<(), String> {
        let mut memo: HashMap<Label, bool> = HashMap::new();
        let mut work = vec![(entry, true)];

        while let Some((label, zero_width)) = work.pop() {
            if let Some(&seen_zero_width) = memo.get(&label)
                && (seen_zero_width || !zero_width)
            {
                continue;
            }
            memo.insert(label, zero_width);

            let instr = instr_map
                .get(&label)
                .copied()
                .ok_or_else(|| format!("{root:?}: dangling label {label:?}"))?;

            match instr {
                InstructionIR::Match(m) => {
                    let after_nav_zero_width = zero_width && m.nav == Nav::Epsilon;
                    if after_nav_zero_width
                        && m.effects.iter().any(|effect| effect.kind().reads_cursor())
                    {
                        return Err(format!(
                            "{root:?}: cursor-reading effect at {:?} is reachable without a consumed node",
                            m.label
                        ));
                    }

                    for &succ in &m.successors {
                        work.push((succ, after_nav_zero_width));
                    }
                }
                InstructionIR::Call(c) => {
                    work.push((c.matched_return(), false));
                    if let Some(zero) = c.zero_return() {
                        work.push((zero, zero_width));
                    }
                }
                InstructionIR::Return(_) => {}
            }
        }

        Ok(())
    }

    fn check_depth_root(
        root: WalkRoot,
        entry: Label,
        route: DefRoute,
        instr_map: &HashMap<Label, &InstructionIR>,
    ) -> Result<(), String> {
        let mut memo: HashMap<Label, i32> = HashMap::new();
        let mut work = vec![(entry, 0i32)];

        while let Some((label, net)) = work.pop() {
            if let Some(&seen) = memo.get(&label) {
                if seen == net {
                    continue;
                }
                return Err(format!(
                    "{root:?}: label {label:?} reached at depths {seen} and {net}"
                ));
            }
            memo.insert(label, net);

            let instr = instr_map
                .get(&label)
                .copied()
                .ok_or_else(|| format!("{root:?}: dangling label {label:?}"))?;

            match instr {
                InstructionIR::Match(m) => {
                    let next_net = net + m.nav.depth_delta();
                    if m.successors.is_empty() {
                        let expected_exit = route
                            .return_depth(ReturnOutcome::Matched)
                            .expect("every body has a matched route");
                        if next_net != expected_exit {
                            return Err(format!(
                                "{root:?}: accepting match {:?} exits at depth {next_net}, expected {expected_exit}",
                                m.label,
                            ));
                        }
                        continue;
                    }
                    for &succ in &m.successors {
                        work.push((succ, next_net));
                    }
                }
                InstructionIR::Call(c) => {
                    work.push((c.matched_return(), net + c.entry_nav().depth_delta()));
                    if let Some(zero) = c.zero_return() {
                        work.push((zero, net));
                    }
                }
                InstructionIR::Return(r) => {
                    if r.entry() != route.return_entry() {
                        return Err(format!(
                            "{root:?}: return {:?} has {:?} entry, expected {:?}",
                            r.label,
                            r.entry(),
                            route.return_entry()
                        ));
                    }
                    let Some(expected_exit) = route.return_depth(r.outcome()) else {
                        return Err(format!(
                            "{root:?}: return {:?} has unsupported {:?} outcome",
                            r.label,
                            r.outcome()
                        ));
                    };
                    if net != expected_exit {
                        return Err(format!(
                            "{root:?}: return {:?} exits at depth {net}, expected {expected_exit}",
                            r.label,
                        ));
                    }
                }
            }
        }

        Ok(())
    }

    pub(super) fn assert_depth_neutrality(nfa: &NfaGraph, context: &str) {
        if let Err(e) = check_depth_neutrality(nfa) {
            panic!("[verify] {context} produced cursor-depth imbalance: {e}");
        }
    }

    #[cfg(test)]
    pub(super) fn assert_no_node_on_zero_width_paths(nfa: &NfaGraph, context: &str) {
        if let Err(e) = check_no_node_on_zero_width_paths(nfa) {
            panic!("[verify] {context} produced zero-width Node effect: {e}");
        }
    }

    fn entries(nfa: &NfaGraph) -> Vec<(WalkRoot, Label)> {
        let mut v = Vec::new();
        for (&def_id, &label) in &nfa.entrypoint_wrappers {
            v.push((WalkRoot::Entrypoint(def_id), label));
        }
        for (variant, &label) in &nfa.def_entries {
            v.push((WalkRoot::Def(variant.clone()), label));
        }
        v
    }

    fn snapshot(nfa: &NfaGraph, ctx: &LowerInput) -> PassSnapshot {
        let walk = GraphWalk::new(&nfa.instructions, &nfa.def_entries, ctx);
        let fingerprints = entries(nfa)
            .into_iter()
            .map(|(key, entry)| {
                let fp = walk.fingerprint(entry);
                (key, entry, fp)
            })
            .collect();
        PassSnapshot {
            instructions: nfa.instructions.clone(),
            fingerprints,
        }
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

    fn verify_after_pass(name: &str, before: &PassSnapshot, nfa: &NfaGraph, ctx: &LowerInput) {
        if let Err(e) = check_labels(&nfa.instructions) {
            panic!("[verify] pass `{name}` produced malformed IR: {e}");
        }
        assert_depth_neutrality(nfa, &format!("pass `{name}`"));
        if let Err(e) = check_no_node_on_zero_width_paths(nfa) {
            panic!("[verify] pass `{name}` produced zero-width Node effect: {e}");
        }

        let before_walk = GraphWalk::new(&before.instructions, &nfa.def_entries, ctx);
        let after_walk = GraphWalk::new(&nfa.instructions, &nfa.def_entries, ctx);
        for (key, entry, before_fp) in &before.fingerprints {
            let after_fp = after_walk.fingerprint(*entry);
            if *before_fp != after_fp {
                let before_paths = before_walk.paths(*entry, DIAG_PATHS);
                let after_paths = after_walk.paths(*entry, DIAG_PATHS);
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
        nfa: &mut NfaGraph,
        ctx: &LowerInput,
        pass: impl FnOnce(&mut NfaGraph),
    ) {
        let before = snapshot(nfa, ctx);
        pass(nfa);
        verify_after_pass(name, &before, nfa, ctx);
    }

    /// Run a pass that may intentionally remove internal definition roots.
    /// Entrypoint behavior and every surviving definition body must remain
    /// unchanged; fingerprints for roots the pass deleted are discarded.
    pub fn run_root_pruning_verified(
        name: &str,
        nfa: &mut NfaGraph,
        ctx: &LowerInput,
        pass: impl FnOnce(&mut NfaGraph),
    ) {
        let mut before = snapshot(nfa, ctx);
        pass(nfa);
        before.fingerprints.retain(|(root, _, _)| match root {
            WalkRoot::Entrypoint(_) => true,
            WalkRoot::Def(variant) => nfa.def_entries.contains_key(variant),
        });
        verify_after_pass(name, &before, nfa, ctx);
    }

    /// Check the freshly-constructed IR before any pass runs: structural soundness
    /// plus balanced scope effects on every path. Passes preserve the fingerprint
    /// (which carries the full effect sequence), so a construction that balances
    /// here stays balanced through the pipeline.
    pub fn verify_constructed(nfa: &NfaGraph, ctx: &LowerInput) {
        if let Err(e) = check_labels(&nfa.instructions) {
            panic!("[verify] construction produced malformed IR: {e}");
        }
        assert_depth_neutrality(nfa, "construction");
        if let Err(e) = check_no_node_on_zero_width_paths(nfa) {
            panic!("[verify] construction produced zero-width Node effect: {e}");
        }
        let walk = GraphWalk::new(&nfa.instructions, &nfa.def_entries, ctx);
        for (key, entry) in entries(nfa) {
            if let Err(e) = walk.check_scopes(entry) {
                panic!("[verify] construction produced unbalanced scope effects for {key:?}:\n{e}");
            }
        }
    }

    /// Check invariants that are only true for brand-new Thompson IR. Later
    /// passes may legitimately move effects across epsilon chains while
    /// preserving the semantic fingerprint.
    pub fn verify_fresh_build(instructions: &[InstructionIR]) {
        if let Err(e) = check_span_start_at_placement(instructions) {
            panic!("[verify] fresh Thompson build misplaced cursor-reading span marker: {e}");
        }
    }

    fn resolve_predicate_value(value: &PredicateValueIR) -> String {
        match value {
            PredicateValueIR::String(text) => text.to_string(),
            PredicateValueIR::Regex(text) => format!("/{text}/"),
        }
    }
}
