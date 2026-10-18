#[cfg(debug_assertions)]
pub use debug_impl::{
    run_root_pruning_verified, run_verified, verify_constructed, verify_fresh_build,
};

#[cfg(not(debug_assertions))]
pub use release_impl::{
    run_root_pruning_verified, run_verified, verify_constructed, verify_fresh_build,
};

#[cfg(not(debug_assertions))]
mod release_impl {
    use crate::compiler::lower::LowerInput;
    use crate::compiler::lower::ir::{InstructionIR, NfaGraph};

    #[inline(always)]
    pub fn run_verified(
        _name: &str,
        nfa: &mut NfaGraph,
        _ctx: &LowerInput,
        pass: impl FnOnce(&mut NfaGraph),
    ) {
        pass(nfa);
    }

    #[inline(always)]
    pub fn run_root_pruning_verified(
        _name: &str,
        nfa: &mut NfaGraph,
        _ctx: &LowerInput,
        pass: impl FnOnce(&mut NfaGraph),
    ) {
        pass(nfa);
    }

    #[inline(always)]
    pub fn verify_constructed(_nfa: &NfaGraph) {}

    #[inline(always)]
    pub fn verify_fresh_build(_instructions: &[InstructionIR]) {}
}

#[cfg(debug_assertions)]
mod debug_impl {
    use std::collections::hash_map::DefaultHasher;
    use std::collections::{HashMap, HashSet};
    use std::hash::{Hash, Hasher};
    use std::ops::ControlFlow;

    use crate::bytecode::{EffectKind, EntryBoundary, Nav};
    use indexmap::IndexMap;

    use crate::compiler::analyze::boundary::ExitPort;
    use crate::compiler::ids::DefId;
    use crate::compiler::lower::LowerInput;
    use crate::compiler::lower::ir::{
        DefRoute, DefSpecialization, EffectArg, EffectIR, InstructionIR, Label, MatchIR, NfaGraph,
        NodeKindConstraint, PortId, PredicateValueIR,
    };

    const MAX_DEPTH: u32 = 200_000;

    const DIAG_PATHS: usize = 400;

    #[derive(Clone, PartialEq, Eq, Hash, Debug)]
    enum SemanticOp {
        Nav(Nav),
        UpNav(u8, u32),
        MatchNamed(Option<String>),
        MatchAnon(Option<String>),
        MatchAny,
        Missing,
        Field(String),
        NegField(String),
        Predicate(u8, String),
        Effect(EffectKind, Option<String>),
        Call(String),
        Return(PortId),
        CycleRef,
        DanglingLabel,
        DepthCut,
    }

    type Path = Vec<SemanticOp>;

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
        Entrypoint(crate::compiler::lower::ir::EntrypointKey, EntryBoundary),
        Def(DefSpecialization),
    }

    struct PassSnapshot {
        instructions: Vec<InstructionIR>,
        fingerprints: Vec<(WalkRoot, Label, Fingerprint)>,
    }

    struct NodeContribution {
        see_through: bool,
        ops: Vec<SemanticOp>,
        succs: Vec<Label>,
    }

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

        if m.missing {
            ops.push(SemanticOp::Missing);
        }

        if let Some(f) = m.node_field {
            let name = ctx
                .analysis
                .grammar
                .field_name(f, ctx.analysis.interner)
                .unwrap_or_else(|| {
                    panic!(
                        "semantic NFA verification found node field {f:?}, but the selected grammar \
                         binding has no name for that field"
                    )
                });
            ops.push(SemanticOp::Field(name.to_string()));
        }

        for &f in &m.neg_fields {
            let name = ctx
                .analysis
                .grammar
                .field_name(f, ctx.analysis.interner)
                .unwrap_or_else(|| {
                    panic!(
                        "semantic NFA verification found negated field {f:?}, but the selected \
                         grammar binding has no name for that field"
                    )
                });
            ops.push(SemanticOp::NegField(name.to_string()));
        }

        if let Some(p) = &m.predicate {
            let value = resolve_predicate_value(&p.value);
            ops.push(SemanticOp::Predicate(p.op.to_byte(), value));
        }

        for e in &m.effects {
            let argument = match e.argument() {
                EffectArg::Literal(value) => value.to_string(),
                EffectArg::Member(member) => format!("M{}", member.raw()),
            };
            ops.push(SemanticOp::Effect(e.kind(), Some(argument)));
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

    fn coalesce_ups(ops: Vec<SemanticOp>) -> Vec<SemanticOp> {
        let mut out: Vec<SemanticOp> = Vec::with_capacity(ops.len());
        let mut pending: Option<(u8, usize)> = None;
        let mut iter = ops.into_iter().peekable();

        while let Some(op) = iter.next() {
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

    struct GraphWalk<'a> {
        instr_map: HashMap<Label, &'a InstructionIR>,
        label_to_def: HashMap<Label, DefId>,
        ctx: &'a LowerInput<'a>,
    }

    impl<'a> GraphWalk<'a> {
        fn new(
            instructions: &'a [InstructionIR],
            def_entries: &IndexMap<DefSpecialization, Label>,
            ctx: &'a LowerInput<'a>,
        ) -> Self {
            Self {
                instr_map: instructions.iter().map(|i| (i.label(), i)).collect(),
                label_to_def: def_entries
                    .iter()
                    .map(|(specialization, &label)| (label, specialization.def_id()))
                    .collect(),
                ctx,
            }
        }

        fn node_contribution(&self, label: Label) -> NodeContribution {
            let Some(&instr) = self.instr_map.get(&label) else {
                return NodeContribution {
                    see_through: false,
                    ops: vec![SemanticOp::DanglingLabel],
                    succs: vec![],
                };
            };

            match instr {
                InstructionIR::Match(m) => {
                    if m.is_epsilon() && m.effects.is_empty() {
                        return NodeContribution {
                            see_through: true,
                            ops: vec![],
                            succs: m.successors.clone(),
                        };
                    }
                    NodeContribution {
                        see_through: false,
                        ops: collect_match_ops(m, self.ctx),
                        succs: m.successors.clone(),
                    }
                }
                InstructionIR::Call(c) => {
                    let def_id = self.label_to_def.get(&c.target).unwrap_or_else(|| {
                        panic!(
                            "semantic NFA verification found call target {:?}, but that label is \
                                 not a definition-specialization entry",
                            c.target
                        )
                    });
                    let name = format!("def#{}", def_id.index());
                    NodeContribution {
                        see_through: false,
                        ops: vec![SemanticOp::Call(name)],
                        succs: c.return_labels().to_vec(),
                    }
                }
                InstructionIR::Return(return_) => NodeContribution {
                    see_through: false,
                    ops: vec![SemanticOp::Return(return_.port)],
                    succs: vec![],
                },
            }
        }

        fn walk(&self, entry: Label, mut on_path: impl FnMut(Path) -> ControlFlow<()>) -> bool {
            let mut truncated = false;

            let mut stack: Vec<(Label, Vec<SemanticOp>, HashSet<Label>, u32)> =
                vec![(entry, Vec::new(), HashSet::new(), 0)];

            while let Some((label, mut ops, mut visited, depth)) = stack.pop() {
                if depth >= MAX_DEPTH {
                    ops.push(SemanticOp::DepthCut);
                    if on_path(normalize_path(ops)).is_break() {
                        return true;
                    }
                    truncated = true;
                    continue;
                }
                if visited.contains(&label) {
                    ops.push(SemanticOp::CycleRef);
                    if on_path(normalize_path(ops)).is_break() {
                        return true;
                    }
                    continue;
                }

                let contribution = self.node_contribution(label);

                if contribution.see_through {
                    for &succ in contribution.succs.iter().rev() {
                        stack.push((succ, ops.clone(), visited.clone(), depth + 1));
                    }
                    continue;
                }

                visited.insert(label);
                ops.extend(contribution.ops);

                if contribution.succs.is_empty() {
                    if on_path(normalize_path(ops)).is_break() {
                        return true;
                    }
                    continue;
                }

                for &succ in contribution.succs.iter().rev() {
                    stack.push((succ, ops.clone(), visited.clone(), depth + 1));
                }
            }

            truncated
        }

        fn fingerprint(&self, entry: Label) -> Fingerprint {
            let mut hashes = Vec::new();
            let truncated = self.walk(entry, |path| {
                hashes.push(hash_path(&path));
                ControlFlow::Continue(())
            });
            Fingerprint { hashes, truncated }
        }

        fn paths(&self, entry: Label, max: usize) -> Vec<Path> {
            let mut paths = Vec::new();
            self.walk(entry, |path| {
                paths.push(path);
                if paths.len() >= max {
                    return ControlFlow::Break(());
                }

                ControlFlow::Continue(())
            });
            paths
        }
    }

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
            TextClose | BoolClose => ScopeRole::Close(ScalarOpen),
            Node | ArrayPush | RecordSet | Absent | ScalarMark | NodeText | NodeBool
            | BoolValue | SpanStartAt | SpanStart | SpanEnd => {
                return None;
            }
        };
        Some(role)
    }

    #[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
    struct ScopeState {
        scopes: Vec<EffectKind>,
        spans: Vec<EffectArg>,
    }

    impl ScopeState {
        fn apply(&mut self, effects: &[EffectIR]) -> Result<(), String> {
            for effect in effects {
                let opcode = effect.kind();
                if matches!(opcode, EffectKind::SpanStartAt | EffectKind::SpanStart) {
                    self.spans.push(effect.argument().clone());
                    continue;
                }

                if opcode == EffectKind::SpanEnd {
                    let expected = effect.argument();
                    match self.spans.pop() {
                        Some(open) if open == *expected => {}
                        Some(open) => {
                            return Err(format!(
                                "SpanEnd({expected:?}) closes a span but the innermost open span is {open:?}"
                            ));
                        }
                        None => return Err("SpanEnd has no matching open span".to_string()),
                    }
                    continue;
                }

                match scope_role(opcode) {
                    None => {}
                    Some(ScopeRole::Open(open)) => self.scopes.push(open),
                    Some(ScopeRole::Close(expected)) => match self.scopes.pop() {
                        Some(top) if top == expected => {}
                        Some(top) => {
                            return Err(format!(
                                "{opcode:?} closes a {expected:?} scope but the innermost open scope is {top:?}"
                            ));
                        }
                        None => return Err(format!("{opcode:?} has no matching open scope")),
                    },
                }
            }

            Ok(())
        }
    }

    enum ScopeVisit {
        Enter(Label, ScopeState),
        Exit(Label),
    }

    struct ScopeCheck<'a> {
        instructions: HashMap<Label, &'a InstructionIR>,
        checked: HashSet<(Label, ScopeState)>,
    }

    impl<'a> ScopeCheck<'a> {
        fn new(instructions: &'a [InstructionIR]) -> Self {
            Self {
                instructions: instructions.iter().map(|i| (i.label(), i)).collect(),
                checked: HashSet::new(),
            }
        }

        fn check(&mut self, entry: Label) -> Result<(), String> {
            let mut active = HashMap::new();
            let mut work = vec![ScopeVisit::Enter(entry, ScopeState::default())];

            while let Some(visit) = work.pop() {
                let (label, mut state) = match visit {
                    ScopeVisit::Enter(label, state) => (label, state),
                    ScopeVisit::Exit(label) => {
                        let state = active
                            .remove(&label)
                            .expect("exiting an active scope label");
                        self.checked.insert((label, state));
                        continue;
                    }
                };

                if let Some(incoming) = active.get(&label) {
                    if incoming != &state {
                        return Err(format!(
                            "cycle at {label:?} changes scope state from {incoming:?} to {state:?}"
                        ));
                    }

                    continue;
                }

                if self.checked.contains(&(label, state.clone())) {
                    continue;
                }

                active.insert(label, state.clone());
                work.push(ScopeVisit::Exit(label));

                let instr = self.instructions.get(&label).expect("labels are validated");
                if let InstructionIR::Match(m) = instr {
                    state
                        .apply(&m.effects)
                        .map_err(|e| format!("{label:?}: {e}"))?;
                }

                if instr.successors().is_empty() {
                    if !state.scopes.is_empty() || !state.spans.is_empty() {
                        return Err(format!(
                            "path ends at {label:?} with unclosed scopes: {state:?}"
                        ));
                    }

                    continue;
                }

                for &succ in instr.successors().iter().rev() {
                    work.push(ScopeVisit::Enter(succ, state.clone()));
                }
            }

            Ok(())
        }
    }

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

    fn check_depth_neutrality(nfa: &NfaGraph) -> Result<(), String> {
        let instr_map: HashMap<Label, &InstructionIR> =
            nfa.instructions.iter().map(|i| (i.label(), i)).collect();

        for (root, entry) in entries(nfa) {
            let (route, ports) = match &root {
                WalkRoot::Entrypoint(..) => (DefRoute::Caller, vec![ExitPort::ConsumedOtherNone]),
                WalkRoot::Def(specialization) => (
                    specialization.route(),
                    specialization.ports().ports().to_vec(),
                ),
            };
            check_depth_root(root, entry, route, &ports, &instr_map, nfa)?;
        }
        Ok(())
    }

    fn check_no_node_on_empty_paths(nfa: &NfaGraph) -> Result<(), String> {
        let instr_map: HashMap<Label, &InstructionIR> =
            nfa.instructions.iter().map(|i| (i.label(), i)).collect();

        for (&def_id, entry) in &nfa.entrypoints {
            check_empty_root(
                WalkRoot::Entrypoint(def_id, entry.boundary),
                entry.target,
                entry.boundary == EntryBoundary::Node,
                &instr_map,
                nfa,
            )?;
        }
        for (specialization, &entry) in &nfa.def_entries {
            check_empty_root(
                WalkRoot::Def(specialization.clone()),
                entry,
                false,
                &instr_map,
                nfa,
            )?;
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

    fn check_empty_root(
        root: WalkRoot,
        entry: Label,
        boundary_reads_cursor: bool,
        instr_map: &HashMap<Label, &InstructionIR>,
        nfa: &NfaGraph,
    ) -> Result<(), String> {
        let mut memo: HashMap<Label, bool> = HashMap::new();
        let mut work = vec![(entry, true)];

        while let Some((label, empty_path)) = work.pop() {
            if let Some(&seen_empty_path) = memo.get(&label)
                && (seen_empty_path || !empty_path)
            {
                continue;
            }
            memo.insert(label, empty_path);

            let instr = instr_map
                .get(&label)
                .copied()
                .ok_or_else(|| format!("{root:?}: dangling label {label:?}"))?;

            match instr {
                InstructionIR::Match(m) => {
                    let after_nav_empty = empty_path && m.nav == Nav::Epsilon;
                    if after_nav_empty
                        && (boundary_reads_cursor && m.successors.is_empty()
                            || m.effects.iter().any(|effect| effect.kind().reads_cursor()))
                    {
                        return Err(format!(
                            "{root:?}: cursor-reading effect at {:?} is reachable without a consumed node",
                            m.label
                        ));
                    }

                    for &succ in &m.successors {
                        work.push((succ, after_nav_empty));
                    }
                }
                InstructionIR::Call(c) => {
                    let specialization =
                        nfa.specialization_for_entry(c.target).ok_or_else(|| {
                            format!(
                                "{root:?}: call {:?} targets non-definition {:?}",
                                c.label, c.target
                            )
                        })?;
                    for (&continuation, &port) in
                        c.returns.iter().zip(specialization.ports().ports())
                    {
                        work.push((
                            continuation,
                            if port.consumed() { false } else { empty_path },
                        ));
                    }
                }
                InstructionIR::Return(r) if boundary_reads_cursor && empty_path => {
                    return Err(format!(
                        "{root:?}: entry Node effect at {:?} is reachable without a consumed node",
                        r.label
                    ));
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
        ports: &[ExitPort],
        instr_map: &HashMap<Label, &InstructionIR>,
        nfa: &NfaGraph,
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
                            .return_depth(true)
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
                    let specialization =
                        nfa.specialization_for_entry(c.target).ok_or_else(|| {
                            format!(
                                "{root:?}: call {:?} targets non-definition {:?}",
                                c.label, c.target
                            )
                        })?;
                    for (&continuation, &port) in
                        c.returns.iter().zip(specialization.ports().ports())
                    {
                        let delta =
                            c.entry.continuation_depth(port.consumed()).ok_or_else(|| {
                                format!(
                                    "{root:?}: caller-owned call {:?} exposes an empty port",
                                    c.label
                                )
                            })?;
                        work.push((continuation, net + delta));
                    }
                }
                InstructionIR::Return(r) => {
                    let Some(port) = ports.get(r.port.index()).copied() else {
                        return Err(format!(
                            "{root:?}: return {:?} uses undeclared port {}",
                            r.label,
                            r.port.to_byte()
                        ));
                    };
                    let Some(expected_exit) = route.return_depth(port.consumed()) else {
                        return Err(format!(
                            "{root:?}: return {:?} has an unsupported empty port",
                            r.label
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

    fn entries(nfa: &NfaGraph) -> Vec<(WalkRoot, Label)> {
        let mut v = Vec::new();
        for (&def_id, entry) in &nfa.entrypoints {
            v.push((WalkRoot::Entrypoint(def_id, entry.boundary), entry.target));
        }
        for (specialization, &label) in &nfa.def_entries {
            v.push((WalkRoot::Def(specialization.clone()), label));
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
        if let Err(e) = check_no_node_on_empty_paths(nfa) {
            panic!("[verify] pass `{name}` produced empty-match Node effect: {e}");
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

    pub fn run_root_pruning_verified(
        name: &str,
        nfa: &mut NfaGraph,
        ctx: &LowerInput,
        pass: impl FnOnce(&mut NfaGraph),
    ) {
        let mut before = snapshot(nfa, ctx);
        pass(nfa);
        before.fingerprints.retain(|(root, _, _)| match root {
            WalkRoot::Entrypoint(..) => true,
            WalkRoot::Def(specialization) => nfa.def_entries.contains_key(specialization),
        });
        verify_after_pass(name, &before, nfa, ctx);
    }

    pub fn verify_constructed(nfa: &NfaGraph) {
        if let Err(e) = check_labels(&nfa.instructions) {
            panic!("[verify] construction produced malformed IR: {e}");
        }
        assert_depth_neutrality(nfa, "construction");
        if let Err(e) = check_no_node_on_empty_paths(nfa) {
            panic!("[verify] construction produced empty-match Node effect: {e}");
        }
        let mut scopes = ScopeCheck::new(&nfa.instructions);
        for (key, entry) in entries(nfa) {
            if let Err(e) = scopes.check(entry) {
                panic!("[verify] construction produced unbalanced scope effects for {key:?}:\n{e}");
            }
        }
    }

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
