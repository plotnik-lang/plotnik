//! The query-module emitter: fork-point NFA → Rust source.
//!
//! The output is one self-contained module: typed output structs/enums (the
//! typegen backend's text, verbatim), the `parse`/`matches` surface and
//! per-type trace readers (`reader.rs`), and the compiled matcher itself —
//! shielded inside a nested `mod matcher` so its machinery names (`Flow`,
//! state consts) can never collide with a query's own type names.
//!
//! Emission is deterministic — instructions render in label order, names and
//! tables come from `BTreeMap`s — so the same query always produces the same
//! code (golden-snapshot-able, cache-friendly).
//!
//! Control skeletons (`run`, `backtrack`, `match_retry`) transcribe the VM's
//! `execute_with_stats` / `backtrack` handler-for-handler over the shared
//! `plotnik_rt::Engine`; per-state arms transcribe one IR instruction each
//! with operands folded to constants. When the VM engine changes shape, the
//! sibling text here must follow — the 06-vm conformance corpus is the tripwire.
//!
//! All emitted shapes live as column-0 raw-string templates at the bottom of
//! this file; [`splice`] substitutes `@KEY@` placeholders and re-indents.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::bytecode::{EffectKind, PredicateOp};
use crate::compiler::analyze::AnalysisArtifacts;
use crate::compiler::codegen::Config;
use crate::compiler::codegen::reader::ReaderGen;
use crate::compiler::emit::regex_table::compile_dfa_bytes;
use crate::compiler::emit::string_table::seed_string_table;
use crate::compiler::emit::tables::TypeTableBuilder;
use crate::compiler::emit::type_table::build_type_table;
use crate::compiler::lower::dump::NfaDumper;
use crate::compiler::lower::ir::{
    CallIR, EffectArg, EffectIR, InstructionIR, Label, LabelOrigin, MatchIR, NfaGraph,
    NodeKindConstraint, PredicateIR, PredicateValueIR, SemanticNfa,
};
use crate::compiler::typegen::rust::Config as RustTypesConfig;
use crate::core::{NodeFieldId, NodeKindId};
use plotnik_rt::{Limit, Nav, SkipPolicy};

/// Generate the Rust query module for a compiled query's fork-point NFA.
///
/// The caller guarantees the query compiled successfully (all ids linked, the
/// emit pipeline accepted the same artifacts) and was built *without*
/// inspection — spans are a VM/playground concern and never reach generated
/// code.
pub(crate) fn generate(
    nfa: &SemanticNfa,
    artifacts: AnalysisArtifacts<'_>,
    config: &Config,
) -> String {
    let graph = nfa.raw();
    assert!(
        graph.spans().is_none(),
        "codegen does not support inspection-compiled queries"
    );

    let generator = Generator::new(graph, artifacts, config);
    generator.render()
}

struct StateInfo {
    /// Dense runtime id (the value checkpoints and frames carry).
    id: u16,
    /// `S{label}_{DEF}` const name; the label matches the NFA dump.
    const_name: String,
    /// `s{label}_{def}` stem for per-state helper fns.
    fn_stem: String,
}

#[derive(Clone, Copy)]
enum CandidateFailure {
    StateBacktrack,
    RetryExhausted,
}

impl CandidateFailure {
    fn code(self) -> &'static str {
        match self {
            CandidateFailure::StateBacktrack => "break 'state Flow::Backtrack;",
            CandidateFailure::RetryExhausted => "return None;",
        }
    }
}

struct CandidateCheck {
    comment: String,
    fail: String,
}

impl CandidateCheck {
    fn new(comment: impl Into<String>, fail: impl Into<String>) -> Self {
        Self {
            comment: comment.into(),
            fail: fail.into(),
        }
    }
}

struct ExpectedKind {
    id: NodeKindId,
    name: String,
    named: bool,
}

impl ExpectedKind {
    fn from_constraint(constraint: NodeKindConstraint, dumper: &NfaDumper<'_>) -> Option<Self> {
        match constraint {
            NodeKindConstraint::Named(Some(id)) => Some(Self::new(id, dumper, true)),
            NodeKindConstraint::Anonymous(Some(id)) => Some(Self::new(id, dumper, false)),
            _ => None,
        }
    }

    fn new(id: NodeKindId, dumper: &NfaDumper<'_>, named: bool) -> Self {
        Self {
            id,
            name: dumper.kind_display_name(id),
            named,
        }
    }

    fn is_builtin_error(&self) -> bool {
        self.id == NodeKindId::ERROR
    }

    fn raw_id(&self) -> u16 {
        u16::from(self.id)
    }
}

struct CallArmPlan<'a> {
    state: &'a str,
    target: &'a str,
    next: &'a str,
    nav: Nav,
    field: Option<NodeFieldId>,
    policy: SkipPolicy,
    stays_on_current_node: bool,
}

impl<'a> CallArmPlan<'a> {
    fn new(generator: &'a Generator<'_>, c: &CallIR) -> Self {
        Self {
            state: generator.state(c.label).const_name.as_str(),
            target: generator.state(c.target).const_name.as_str(),
            next: generator.state(c.next).const_name.as_str(),
            nav: c.nav,
            field: c.node_field,
            policy: c.nav.skip_policy(),
            stays_on_current_node: matches!(c.nav, Nav::Stay | Nav::StayExact),
        }
    }

    fn opens_labeled_block(&self) -> bool {
        !self.stays_on_current_node || self.field.is_some()
    }

    fn pushes_retry(&self) -> bool {
        !self.stays_on_current_node && self.policy != SkipPolicy::Exact
    }
}

struct RegexStatic {
    index: usize,
    bytes: Vec<u8>,
}

impl RegexStatic {
    fn compile(index: usize, pattern: &str) -> Self {
        let bytes = compile_dfa_bytes(pattern).expect("regex predicate compiled during emit");
        Self { index, bytes }
    }
}

struct Generator<'a> {
    graph: &'a NfaGraph,
    dumper: NfaDumper<'a>,
    config: &'a Config,
    artifacts: AnalysisArtifacts<'a>,
    /// Member-index layout shared with the bytecode emitter, so generated
    /// `Set`/`EnumOpen` payloads equal the VM's byte-for-byte.
    types: TypeTableBuilder,
    /// Instructions in label order (the dump's order).
    sorted: Vec<&'a InstructionIR>,
    states: BTreeMap<Label, StateInfo>,
    /// Field-id consts: raw id → `F_{NAME}` const name. Keyed by id and
    /// collision-suffixed at insert, so the const namespace stays injective
    /// even if two grammar field names collapse to one SHOUTY form.
    fields: BTreeMap<u16, String>,
    /// Kind ids baked into candidate checks → `(grammar name, is_named)`,
    /// for the generated language-skew assert. The builtin `ERROR` id is
    /// grammar-independent and carries no skew signal, so it is skipped.
    expect_kinds: BTreeMap<u16, ExpectedKind>,
    /// Field ids baked into field checks → grammar name, same purpose.
    expect_fields: BTreeMap<u16, String>,
    /// Regex predicates in first-appearance (label) order: pattern → (index, DFA bytes).
    regexes: BTreeMap<String, RegexStatic>,
    /// Whether any candidate check reads node text (predicates exist).
    any_predicate: bool,
    /// Whether any *retryable* state has a predicate (match_retry reads text).
    any_retry_predicate: bool,
}

impl<'a> Generator<'a> {
    fn new(graph: &'a NfaGraph, artifacts: AnalysisArtifacts<'a>, config: &'a Config) -> Self {
        // Mirror the emit pipeline's table construction exactly: the member
        // layout must match what the module the VM runs carries.
        let strings = seed_string_table(graph).expect("string table built during emit");
        let (types, _strings) =
            build_type_table(&artifacts, strings).expect("type table built during emit");

        let dumper = NfaDumper::new(graph, artifacts);

        let mut sorted: Vec<&InstructionIR> = graph.instructions().iter().collect();
        sorted.sort_by_key(|i| i.label());
        assert!(
            sorted.len() <= u16::MAX as usize + 1,
            "state space exceeds u16 ids"
        );

        let mut generator = Self {
            graph,
            dumper,
            config,
            artifacts,
            types,
            sorted,
            states: BTreeMap::new(),
            fields: BTreeMap::new(),
            expect_kinds: BTreeMap::new(),
            expect_fields: BTreeMap::new(),
            regexes: BTreeMap::new(),
            any_predicate: false,
            any_retry_predicate: false,
        };
        generator.assign_states();
        generator.collect_operands();
        generator
    }

    fn assign_states(&mut self) {
        let width = self.dumper.label_width();
        for (id, instr) in self.sorted.iter().enumerate() {
            let label = instr.label();
            let def = self.dumper.def_name_of(label);
            let suffix = match self
                .graph
                .origin(label)
                .expect("every pre-pack label carries an origin")
            {
                LabelOrigin::Def(_) => "",
                LabelOrigin::ConsumingDef(_) => "_plus",
                LabelOrigin::Wrapper(_) => "_ep",
            };
            let const_name = format!(
                "S{:0width$}_{}{}",
                label.0,
                shouty_ident(def),
                suffix.to_uppercase()
            );
            let fn_stem = format!("s{:0width$}_{}{}", label.0, snake_ident(def), suffix);
            self.states.insert(
                label,
                StateInfo {
                    id: id as u16,
                    const_name,
                    fn_stem,
                },
            );
        }
    }

    fn collect_operands(&mut self) {
        // A handle copy: the refs point into the graph, not into `self`, so the
        // loop can mutate the operand tables freely.
        let sorted = self.sorted.clone();
        for instr in sorted {
            match instr {
                InstructionIR::Match(m) => self.record_match_operands(m),
                InstructionIR::Call(c) => self.record_call_operands(c),
                InstructionIR::Return(_) => {}
            }
        }
    }

    fn record_match_operands(&mut self, m: &MatchIR) {
        self.record_kind(m.node_kind);
        if let Some(field) = m.node_field {
            self.record_field(field);
        }
        for &field in &m.neg_fields {
            self.record_field(field);
        }
        if let Some(pred) = &m.predicate {
            self.record_predicate(m, pred);
        }
    }

    fn record_call_operands(&mut self, c: &CallIR) {
        if let Some(field) = c.node_field {
            self.record_field(field);
        }
    }

    fn record_predicate(&mut self, m: &MatchIR, pred: &PredicateIR) {
        self.any_predicate = true;
        if is_retryable(m) {
            self.any_retry_predicate = true;
        }
        if let PredicateValueIR::Regex(pattern) = &pred.value {
            self.record_regex(pattern);
        }
    }

    fn record_regex(&mut self, pattern: &str) {
        let next_index = self.regexes.len();
        self.regexes
            .entry(pattern.to_string())
            .or_insert_with(|| RegexStatic::compile(next_index, pattern));
    }

    fn record_field(&mut self, field: NodeFieldId) {
        let id = u16::from(field);
        let display = self.dumper.field_display_name(field);
        self.expect_fields.insert(id, display.clone());
        if self.fields.contains_key(&id) {
            return;
        }
        // Distinct grammar field names can collapse to one SHOUTY form
        // (`fooBar` / `foo_bar`); suffix until free so a collision can never
        // silently alias two ids under one const.
        let mut name = format!("F_{}", shouty_ident(&display));
        while self.fields.values().any(|taken| *taken == name) {
            let _ = write!(name, "_{id}");
        }
        self.fields.insert(id, name);
    }

    fn record_kind(&mut self, constraint: NodeKindConstraint) {
        let Some(expected) = ExpectedKind::from_constraint(constraint, &self.dumper) else {
            return;
        };
        if expected.is_builtin_error() {
            return;
        }
        self.expect_kinds.insert(expected.raw_id(), expected);
    }

    fn state(&self, label: Label) -> &StateInfo {
        self.states
            .get(&label)
            .expect("every successor label addresses an instruction")
    }

    fn field_const(&self, field: NodeFieldId) -> String {
        self.fields
            .get(&u16::from(field))
            .expect("every rendered field was recorded during operand collection")
            .clone()
    }

    fn regex_static(&self, pattern: &str) -> String {
        let regex = self
            .regexes
            .get(pattern)
            .expect("regex collected before rendering");
        format!("RE_{}", regex.index)
    }

    /// The absolute member-table index this effect's payload resolves to —
    /// the same `member_base + relative_index` the bytecode emitter writes.
    fn effect_payload(&self, effect: &EffectIR) -> u16 {
        match effect.payload() {
            EffectArg::Literal(value) => {
                u16::try_from(*value).expect("literal effect payload fits u16")
            }
            EffectArg::Member(member) => {
                let base = self
                    .types
                    .member_base(member.parent_type)
                    .expect("effect member parent emitted to the type table");
                base + member.relative_index
            }
        }
    }

    fn render(&self) -> String {
        let rust_config = RustTypesConfig::new()
            .rt_crate(self.config.rt_crate.clone())
            .serde(self.config.serde);
        let readers = ReaderGen::new(self.artifacts, &self.types, &rust_config);

        let mut out = String::new();
        self.header(&mut out);
        out.push('\n');
        out.push_str(&crate::compiler::typegen::rust::emit(
            self.artifacts.type_analysis,
            self.artifacts.dependency_analysis,
            self.artifacts.interner,
            &rust_config,
        ));
        out.push_str(&readers.parse_api(self.graph.entrypoint_wrappers().keys().copied()));
        out.push_str(&readers.readers());
        self.entry_reexports(&mut out);

        let mut machinery = String::new();
        self.mod_header(&mut machinery, readers.max_reader_frame_bytes());
        self.language_check(&mut machinery);
        self.field_consts(&mut machinery);
        self.regex_statics(&mut machinery);
        self.state_consts(&mut machinery);
        self.entry_fns(&mut machinery);
        machinery.push_str(DRIVER_SKELETON);
        self.step_fn(&mut machinery);
        self.cand_fns(&mut machinery);
        self.finish_fns(&mut machinery);
        self.backtrack_fn(&mut machinery);
        self.match_retry_fn(&mut machinery);

        out.push('\n');
        out.push_str("/// The compiled matcher: engine machinery shielded from the query's\n");
        out.push_str("/// type namespace.\n");
        out.push_str("mod matcher {\n");
        for line in machinery.lines() {
            if line.is_empty() {
                out.push('\n');
            } else {
                out.push_str("    ");
                out.push_str(line);
                out.push('\n');
            }
        }
        out.push_str("}\n");
        out
    }

    fn header(&self, out: &mut String) {
        splice(out, "", HEADER, &[("RT", &self.config.rt_crate)]);
    }

    /// `pub use` every trace entry point at module root, so the public
    /// surface (`{def}_trace`, per [`entry_fn_name`]) doesn't move when the
    /// machinery does.
    fn entry_reexports(&self, out: &mut String) {
        let names: Vec<String> = self
            .graph
            .entrypoint_wrappers()
            .keys()
            .map(|&def_id| {
                let sym = self.artifacts.dependency_analysis.def_name_sym(def_id);
                entry_fn_name(self.artifacts.interner.resolve(sym))
            })
            .collect();
        out.push('\n');
        let _ = writeln!(out, "pub use self::matcher::{{{}}};", names.join(", "));
    }

    fn mod_header(&self, out: &mut String, max_reader_frame_bytes: u64) {
        splice(
            out,
            "",
            MOD_HEADER,
            &[
                ("RT", &self.config.rt_crate),
                ("STEPS", &limit_expr(self.config.limits.steps)),
                ("MEMORY", &limit_expr(self.config.limits.memory)),
                ("READER_FRAME", &max_reader_frame_bytes.to_string()),
                (
                    "DEPTH",
                    &depth_expr(self.config.depth, max_reader_frame_bytes),
                ),
            ],
        );
    }

    /// The language-skew tables and their assert: every kind and field id in
    /// this module is a numeric bake of the generation-time grammar, so the
    /// first run checks each one against the tree's live language.
    fn language_check(&self, out: &mut String) {
        out.push('\n');
        out.push_str(
            "/// Node-kind ids baked into the candidate checks: `(id, name, is_named)`\n\
             /// as the generation-time grammar defines them.\n",
        );
        if self.expect_kinds.is_empty() {
            out.push_str("const EXPECTED_KINDS: &[(u16, &str, bool)] = &[];\n");
        } else {
            out.push_str("const EXPECTED_KINDS: &[(u16, &str, bool)] = &[\n");
            for (id, expected) in &self.expect_kinds {
                let name = &expected.name;
                let named = expected.named;
                let _ = writeln!(out, "    ({id}, {name:?}, {named}),");
            }
            out.push_str("];\n");
        }
        out.push('\n');
        out.push_str("/// Field ids baked into the field checks: `(id, name)`.\n");
        if self.expect_fields.is_empty() {
            out.push_str("const EXPECTED_FIELDS: &[(u16, &str)] = &[];\n");
        } else {
            out.push_str("const EXPECTED_FIELDS: &[(u16, &str)] = &[\n");
            for (id, name) in &self.expect_fields {
                let _ = writeln!(out, "    ({id}, {name:?}),");
            }
            out.push_str("];\n");
        }
        out.push('\n');
        splice(out, "", VERIFY_LANGUAGE, &[]);
    }

    fn field_consts(&self, out: &mut String) {
        if self.fields.is_empty() {
            return;
        }
        out.push('\n');
        for (id, name) in &self.fields {
            let _ = writeln!(
                out,
                "const {name}: rt::NodeFieldId = rt::NodeFieldId::from_raw({id});"
            );
        }
    }

    fn regex_statics(&self, out: &mut String) {
        let mut ordered: Vec<(&String, &RegexStatic)> = self.regexes.iter().collect();
        ordered.sort_by_key(|(_, regex)| regex.index);
        for (pattern, regex) in ordered {
            out.push('\n');
            let _ = writeln!(out, "// /{pattern}/ — serialized sparse DFA");
            let _ = writeln!(
                out,
                "static RE_{}: rt::StaticDfa = rt::StaticDfa::new(&[",
                regex.index
            );
            for chunk in regex.bytes.chunks(16) {
                let line = chunk
                    .iter()
                    .map(|b| b.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = writeln!(out, "    {line},");
            }
            out.push_str("]);\n");
        }
    }

    fn state_consts(&self, out: &mut String) {
        out.push('\n');
        out.push_str("// Dense runtime state ids, in NFA label order.\n");
        let mut current: Option<&str> = None;
        for instr in &self.sorted {
            let label = instr.label();
            let def = self.dumper.def_name_of(label);
            if current != Some(def) {
                let _ = writeln!(out, "// {def}:");
                current = Some(def);
            }
            let info = self.state(label);
            let _ = writeln!(out, "const {}: u16 = {};", info.const_name, info.id);
        }
    }

    fn entry_fns(&self, out: &mut String) {
        // An unbounded resource emits `false` for its metering const, folding
        // the check out of `run`. With both unbounded there is no ceiling to
        // resolve, so the safe entries pass `NO_LIMITS` rather than pay for a
        // per-call node count that nothing reads.
        let steps_metered = self.config.limits.steps != Limit::Unbounded;
        let memory_metered = self.config.limits.memory != Limit::Unbounded;
        let safe_limits = if steps_metered || memory_metered {
            "resolved_limits(tree)"
        } else {
            "NO_LIMITS"
        };
        let steps_metered = if steps_metered { "true" } else { "false" };
        let memory_metered = if memory_metered { "true" } else { "false" };
        for &label in self.graph.entrypoint_wrappers().values() {
            let def = self.dumper.def_name_of(label);
            let info = self.state(label);
            let subs = [
                ("DEF", def),
                ("FN", &entry_fn_name(def)),
                ("SAFE_FN", &safe_entry_fn_name(def)),
                ("ACCEPTS_FN", &accepts_entry_fn_name(def)),
                ("ENTRY", info.const_name.as_str()),
                ("STEPS_METERED", steps_metered),
                ("MEMORY_METERED", memory_metered),
                ("SAFE_LIMITS", safe_limits),
            ];
            out.push('\n');
            splice(out, "", ENTRY_FN, &subs);
            out.push('\n');
            splice(out, "", ENTRY_FN_SAFE, &subs);
            out.push('\n');
            splice(out, "", ENTRY_ACCEPTS_SAFE, &subs);
        }
    }

    fn step_fn(&self, out: &mut String) {
        let source_param = if self.any_predicate {
            "source"
        } else {
            "_source"
        };
        out.push('\n');
        splice(out, "", STEP_OPEN, &[("SOURCE", source_param)]);
        for instr in &self.sorted {
            for line in self.dumper.render_instruction(instr).lines() {
                let _ = writeln!(out, "        // {}", line.trim_end());
            }
            match instr {
                InstructionIR::Match(m) => self.match_arm(out, m),
                InstructionIR::Call(c) => self.call_arm(out, c),
                InstructionIR::Return(r) => {
                    splice(
                        out,
                        "        ",
                        RETURN_ARM,
                        &[("STATE", &self.state(r.label).const_name)],
                    );
                }
            }
        }
        out.push_str(STEP_CLOSE);
    }

    /// The dispatch arm for a Match instruction. Epsilon runs effects and
    /// branches; everything else navigates, searches candidates, leaves a
    /// retry checkpoint when the engine owns the sibling search, then runs
    /// the shared finish (effects + branch).
    fn match_arm(&self, out: &mut String, m: &MatchIR) {
        let info = self.state(m.label);

        if m.is_epsilon() {
            assert!(
                matches!(m.node_kind, NodeKindConstraint::Any)
                    && !m.missing
                    && m.node_field.is_none()
                    && m.neg_fields.is_empty()
                    && m.predicate.is_none(),
                "epsilon match carries no candidate checks"
            );
            let _ = writeln!(out, "        {} => {{", info.const_name);
            self.effects_and_flow(out, m, "            ");
            out.push_str("        }\n");
            return;
        }

        let has_navigate = !matches!(m.nav, Nav::Stay | Nav::StayExact);
        let has_checks = has_candidate_checks(m);
        let needs_label = has_navigate || has_checks;

        if needs_label {
            let _ = writeln!(out, "        {} => 'state: {{", info.const_name);
        } else {
            let _ = writeln!(out, "        {} => {{", info.const_name);
        }

        if has_navigate {
            splice(
                out,
                "            ",
                NAVIGATE_OR_BACKTRACK,
                &[("NAV", &nav_expr(m.nav))],
            );
        }

        self.candidate_search(out, m, "            ", CandidateFailure::StateBacktrack);
        self.retry_checkpoint(out, m, "            ");

        if is_retryable(m) {
            let _ = writeln!(out, "            finish_{}(eng)", info.fn_stem);
        } else {
            self.effects_and_flow(out, m, "            ");
        }
        out.push_str("        }\n");
    }

    /// The candidate loop: try the current node, step past rejected ones per
    /// the nav's skip policy. An `Exact` policy has exactly one candidate, so
    /// the loop degenerates to a single check.
    fn candidate_search(
        &self,
        out: &mut String,
        m: &MatchIR,
        indent: &str,
        failure: CandidateFailure,
    ) {
        if !has_candidate_checks(m) {
            return;
        }
        let call = self.cand_call(m);
        let fail = failure.code();
        if m.nav.skip_policy() == SkipPolicy::Exact {
            splice(
                out,
                indent,
                CANDIDATE_ONCE,
                &[("CAND", &call), ("FAIL", fail)],
            );
            return;
        }
        splice(
            out,
            indent,
            CANDIDATE_LOOP,
            &[
                ("CAND", &call),
                ("POLICY", &policy_expr(m.nav.skip_policy())),
                ("FAIL", fail),
            ],
        );
    }

    /// Accepting a candidate in an engine-owned sibling search leaves a
    /// match-retry checkpoint, gated on the policy admitting the node into
    /// the pattern's gap — the VM's `push_match_retry_if_resumable`.
    fn retry_checkpoint(&self, out: &mut String, m: &MatchIR, indent: &str) {
        if !is_retryable(m) {
            return;
        }
        splice(
            out,
            indent,
            RETRY_CHECKPOINT,
            &[
                ("POLICY", &policy_expr(m.nav.skip_policy())),
                ("STATE", &self.state(m.label).const_name),
            ],
        );
    }

    fn cand_call(&self, m: &MatchIR) -> String {
        let stem = &self.state(m.label).fn_stem;
        if m.predicate.is_some() {
            format!("cand_{stem}(eng, source)")
        } else {
            format!("cand_{stem}(eng)")
        }
    }

    /// Post-acceptance effects, then the successor branch — inline for states
    /// with no retry, `finish_*` fns where the backtrack path replays them.
    fn effects_and_flow(&self, out: &mut String, m: &MatchIR, indent: &str) {
        for effect in &m.effects {
            self.effect_stmt(out, effect, indent);
        }
        match m.successors.as_slice() {
            [] => {
                let _ = writeln!(out, "{indent}Flow::Accept");
            }
            [next] => {
                let _ = writeln!(out, "{indent}Flow::Jump({})", self.state(*next).const_name);
            }
            [next, alts @ ..] => {
                let alt_names = alts
                    .iter()
                    .map(|alt| self.state(*alt).const_name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = writeln!(out, "{indent}eng.push_branches(&[{alt_names}]);");
                let _ = writeln!(out, "{indent}Flow::Jump({})", self.state(*next).const_name);
            }
        }
    }

    fn effect_stmt(&self, out: &mut String, effect: &EffectIR, indent: &str) {
        let unit = |out: &mut String, variant: &str| {
            let _ = writeln!(
                out,
                "{indent}eng.emit_data(|_| rt::RuntimeEffect::{variant});"
            );
        };
        match effect.kind() {
            EffectKind::Node => {
                let _ = writeln!(
                    out,
                    "{indent}eng.emit_data(|c| rt::RuntimeEffect::Node(c.node()));"
                );
            }
            EffectKind::ArrayOpen => unit(out, "ArrayOpen"),
            EffectKind::Push => unit(out, "Push"),
            EffectKind::ArrayClose => unit(out, "ArrayClose"),
            EffectKind::StructOpen => unit(out, "StructOpen"),
            EffectKind::StructClose => unit(out, "StructClose"),
            EffectKind::EnumClose => unit(out, "EnumClose"),
            EffectKind::Null => unit(out, "Null"),
            EffectKind::Set | EffectKind::EnumOpen => {
                let variant = if effect.kind() == EffectKind::Set {
                    "Set"
                } else {
                    "EnumOpen"
                };
                let _ = writeln!(
                    out,
                    "{indent}eng.emit_data(|_| rt::RuntimeEffect::{variant}({})); // {}",
                    self.effect_payload(effect),
                    self.dumper.effect_display(effect)
                );
            }
            EffectKind::SuppressBegin => {
                let _ = writeln!(out, "{indent}eng.suppress_begin();");
            }
            EffectKind::SuppressEnd => {
                let _ = writeln!(out, "{indent}eng.suppress_end();");
            }
            EffectKind::SpanStartAt | EffectKind::SpanStart | EffectKind::SpanEnd => {
                unreachable!("inspection spans rejected before generation")
            }
        }
    }

    /// The dispatch arm for a Call: navigate (or stay), satisfy the field
    /// constraint, leave a call-retry checkpoint when the nav owns a search,
    /// then enter the callee.
    fn call_arm(&self, out: &mut String, c: &CallIR) {
        let plan = CallArmPlan::new(self, c);

        if plan.opens_labeled_block() {
            let _ = writeln!(out, "        {} => 'state: {{", plan.state);
        } else {
            let _ = writeln!(out, "        {} => {{", plan.state);
        }

        self.call_candidate_setup(out, &plan);
        self.call_retry_checkpoint(out, &plan);

        let _ = writeln!(out, "            eng.enter_frame({});", plan.next);
        let _ = writeln!(out, "            Flow::Jump({})", plan.target);
        out.push_str("        }\n");
    }

    fn call_candidate_setup(&self, out: &mut String, plan: &CallArmPlan<'_>) {
        if plan.stays_on_current_node {
            self.call_field_check(out, plan.field);
            return;
        }

        splice(
            out,
            "            ",
            NAVIGATE_OR_BACKTRACK,
            &[("NAV", &nav_expr(plan.nav))],
        );
        self.call_field_scan(out, plan.field, plan.policy);
    }

    fn call_field_check(&self, out: &mut String, field: Option<NodeFieldId>) {
        let Some(field) = field else {
            return;
        };
        splice(
            out,
            "            ",
            CALL_FIELD_CHECK,
            &[("FIELD", &self.field_const(field))],
        );
    }

    fn call_field_scan(&self, out: &mut String, field: Option<NodeFieldId>, policy: SkipPolicy) {
        let Some(field) = field else {
            return;
        };
        splice(
            out,
            "            ",
            CALL_FIELD_SCAN,
            &[
                ("FIELD", &self.field_const(field)),
                ("POLICY", &policy_expr(policy)),
            ],
        );
    }

    fn call_retry_checkpoint(&self, out: &mut String, plan: &CallArmPlan<'_>) {
        if !plan.pushes_retry() {
            return;
        }

        let field = match plan.field {
            Some(field) => format!("Some({})", self.field_const(field)),
            None => "None".to_string(),
        };
        splice(
            out,
            "            ",
            CALL_RETRY_PUSH,
            &[
                ("STATE", plan.state),
                ("TARGET", plan.target),
                ("NEXT", plan.next),
                ("FIELD", &field),
                ("POLICY", &policy_expr(plan.policy)),
            ],
        );
    }

    fn cand_fns(&self, out: &mut String) {
        for instr in &self.sorted {
            let InstructionIR::Match(m) = instr else {
                continue;
            };
            if m.is_epsilon() || !has_candidate_checks(m) {
                continue;
            }
            self.cand_fn(out, m);
        }
    }

    /// The candidate check, mirroring the VM's `candidate_matches` order:
    /// kind, missing, field, negated fields, predicate.
    fn cand_fn(&self, out: &mut String, m: &MatchIR) {
        let info = self.state(m.label);
        out.push('\n');
        let _ = writeln!(
            out,
            "/// `{}` candidate: `{}`.",
            info.const_name,
            self.dumper.node_pattern_display(m)
        );
        out.push_str("#[inline]\n");
        let source_param = if m.predicate.is_some() {
            ", source: &str"
        } else {
            ""
        };
        let _ = writeln!(
            out,
            "fn cand_{}(eng: &rt::Engine<'_>{source_param}) -> bool {{",
            info.fn_stem
        );
        if needs_node_binding(m) {
            out.push_str("    let node = eng.node();\n");
        }
        for check in self.candidate_checks(m) {
            splice(
                out,
                "    ",
                CHECK,
                &[("COMMENT", &check.comment), ("FAIL", &check.fail)],
            );
        }
        out.push_str("    true\n}\n");
    }

    /// Every check in the VM's `candidate_matches` order.
    fn candidate_checks(&self, m: &MatchIR) -> Vec<CandidateCheck> {
        let mut checks = Vec::new();

        match m.node_kind {
            NodeKindConstraint::Any => {}
            NodeKindConstraint::Named(None) => {
                checks.push(CandidateCheck::new("(_)", "!node.is_named()"));
            }
            NodeKindConstraint::Named(Some(id)) => {
                checks.push(CandidateCheck::new(
                    format!("({})", self.dumper.kind_display_name(id)),
                    format!("node.kind_id() != {} || !node.is_named()", u16::from(id)),
                ));
            }
            NodeKindConstraint::Anonymous(None) => {
                checks.push(CandidateCheck::new("\"_\"", "node.is_named()"));
            }
            NodeKindConstraint::Anonymous(Some(id)) => {
                checks.push(CandidateCheck::new(
                    format!("\"{}\"", self.dumper.kind_display_name(id)),
                    format!("node.kind_id() != {} || node.is_named()", u16::from(id)),
                ));
            }
        }

        if m.missing {
            checks.push(CandidateCheck::new(
                "(MISSING …): only nodes the parser inserted during error recovery".to_string(),
                "!node.is_missing()".to_string(),
            ));
        }

        if let Some(field) = m.node_field {
            checks.push(CandidateCheck::new(
                format!("{}:", self.dumper.field_display_name(field)),
                format!(
                    "eng.cursor().field_id() != Some({})",
                    self.field_const(field)
                ),
            ));
        }

        for &field in &m.neg_fields {
            checks.push(CandidateCheck::new(
                format!("-{}", self.dumper.field_display_name(field)),
                format!(
                    "node.child_by_field_id(u16::from({})).is_some()",
                    self.field_const(field)
                ),
            ));
        }

        if let Some(pred) = &m.predicate {
            checks.push(self.predicate_check(pred));
        }

        checks
    }

    fn predicate_check(&self, pred: &PredicateIR) -> CandidateCheck {
        let text = "rt::node_text(source, &node)";
        match &pred.value {
            PredicateValueIR::String(value) => {
                let lit = format!("{value:?}");
                let fail = match pred.op {
                    PredicateOp::Eq => format!("{text} != {lit}"),
                    PredicateOp::Ne => format!("{text} == {lit}"),
                    PredicateOp::StartsWith => format!("!{text}.starts_with({lit})"),
                    PredicateOp::EndsWith => format!("!{text}.ends_with({lit})"),
                    PredicateOp::Contains => format!("!{text}.contains({lit})"),
                    PredicateOp::RegexMatch | PredicateOp::RegexNoMatch => {
                        unreachable!("regex predicate carries a regex value")
                    }
                };
                CandidateCheck::new(format!("{} {lit}", pred.op.as_str()), fail)
            }
            PredicateValueIR::Regex(pattern) => {
                let re = self.regex_static(pattern);
                let fail = match pred.op {
                    PredicateOp::RegexMatch => format!("!{re}.is_match({text})"),
                    PredicateOp::RegexNoMatch => format!("{re}.is_match({text})"),
                    _ => unreachable!("string predicate carries a string value"),
                };
                CandidateCheck::new(format!("{} /{pattern}/", pred.op.as_str()), fail)
            }
        }
    }

    fn finish_fns(&self, out: &mut String) {
        for instr in &self.sorted {
            let InstructionIR::Match(m) = instr else {
                continue;
            };
            if !is_retryable(m) {
                continue;
            }
            let info = self.state(m.label);
            // `eng` feeds effect emission and branch pushes; a finish that
            // only jumps must not bind it, or every such fn warns.
            let eng = if m.effects.is_empty() && m.successors.len() < 2 {
                "_eng"
            } else {
                "eng"
            };
            out.push('\n');
            splice(
                out,
                "",
                FINISH_FN_OPEN,
                &[
                    ("STATE", &info.const_name),
                    ("STEM", &info.fn_stem),
                    ("ENG", eng),
                ],
            );
            self.effects_and_flow(out, m, "    ");
            out.push_str("}\n");
        }
    }

    /// The backtrack unwind — the VM's `backtrack` with the Match arm
    /// dispatched to generated per-state retries.
    fn backtrack_fn(&self, out: &mut String) {
        let source_arg = if self.any_retry_predicate {
            "source"
        } else {
            "_source"
        };
        out.push('\n');
        splice(out, "", BACKTRACK_SKELETON, &[("SOURCE", source_arg)]);
    }

    /// Per-state match-retry: step past the accepted-but-failed candidate,
    /// re-run the same state's candidate search, replay the finish. Only
    /// sibling-search states can carry a match-retry checkpoint.
    fn match_retry_fn(&self, out: &mut String) {
        let retryable: Vec<&MatchIR> = self
            .sorted
            .iter()
            .filter_map(|instr| match instr {
                InstructionIR::Match(m) if is_retryable(m) => Some(m),
                _ => None,
            })
            .collect();

        let eng_param = if retryable.is_empty() { "_eng" } else { "eng" };
        let source_param = if self.any_retry_predicate {
            "source"
        } else {
            "_source"
        };
        out.push('\n');
        splice(
            out,
            "",
            MATCH_RETRY_OPEN,
            &[("ENG", eng_param), ("SOURCE", source_param)],
        );
        for m in retryable {
            let info = self.state(m.label);
            let _ = writeln!(out, "        {} => {{", info.const_name);
            splice(
                out,
                "            ",
                RETRY_ADVANCE,
                &[("POLICY", &policy_expr(m.nav.skip_policy()))],
            );
            self.candidate_search(out, m, "            ", CandidateFailure::RetryExhausted);
            self.retry_checkpoint(out, m, "            ");
            let _ = writeln!(out, "            Some(finish_{}(eng))", info.fn_stem);
            out.push_str("        }\n");
        }
        out.push_str(MATCH_RETRY_CLOSE);
    }
}

/// Whether acceptance at this state is a revisitable choice point (the VM's
/// `push_match_retry_if_resumable` static half; the `admits` gate is runtime).
fn is_retryable(m: &MatchIR) -> bool {
    !m.is_epsilon() && m.nav.is_sibling_search()
}

/// Whether the state checks anything about the candidate node. A bare
/// wildcard accepts the first candidate its navigation lands on.
fn has_candidate_checks(m: &MatchIR) -> bool {
    !matches!(m.node_kind, NodeKindConstraint::Any)
        || m.missing
        || m.node_field.is_some()
        || !m.neg_fields.is_empty()
        || m.predicate.is_some()
}

/// Whether the candidate fn reads the node itself (field checks go through
/// the cursor instead).
fn needs_node_binding(m: &MatchIR) -> bool {
    !matches!(m.node_kind, NodeKindConstraint::Any)
        || m.missing
        || !m.neg_fields.is_empty()
        || m.predicate.is_some()
}

/// `Nav` as a generated-code expression; the `Debug` form matches the variant
/// syntax (`Up(2)`), pinned by `nav_expr_matches_debug` in the tests.
pub(super) fn nav_expr(nav: Nav) -> String {
    format!("rt::Nav::{nav:?}")
}

fn policy_expr(policy: SkipPolicy) -> String {
    format!("rt::SkipPolicy::{policy:?}")
}

/// PascalCase → SHOUTY_SNAKE (`FooBar` → `FOO_BAR`, `HTTPServer` → `HTTP_SERVER`).
pub(super) fn shouty_ident(name: &str) -> String {
    case_segments(name)
        .map(|s| s.to_uppercase())
        .collect::<Vec<_>>()
        .join("_")
}

/// PascalCase → snake_case (`FooBar` → `foo_bar`).
pub(super) fn snake_ident(name: &str) -> String {
    case_segments(name)
        .map(|s| s.to_lowercase())
        .collect::<Vec<_>>()
        .join("_")
}

/// The public function a generated matcher exposes for a definition
/// (`FooBar` → `foo_bar_trace`). Part of the generated-code contract:
/// callers that only know the definition name resolve the symbol through
/// this, never by re-deriving the casing.
pub fn entry_fn_name(def_name: &str) -> String {
    format!("{}_trace", snake_ident(def_name))
}

/// The safe sibling of [`entry_fn_name`], `pub(super)` inside the matcher
/// module — the safe `parse` surface reaches the driver through it. It applies
/// the module's compiled-in limit policy, which may be unbounded (hence `_safe`,
/// not `_metered`: metering is per-resource and folds out when unbounded).
pub(super) fn safe_entry_fn_name(def_name: &str) -> String {
    format!("{}_safe", snake_ident(def_name))
}

/// Private yes/no entry point for safe `matches`: applies the compiled-in
/// policy with data effects suppressed so no typed output is built.
pub(super) fn accepts_entry_fn_name(def_name: &str) -> String {
    format!("{}_accepts", snake_ident(def_name))
}

/// `Limit` as a generated-code expression; the `Debug` form matches the
/// variant syntax (`Of(3)`), pinned by `limit_expr_matches_debug` in the tests.
pub(super) fn limit_expr(limit: Limit) -> String {
    format!("rt::Limit::{limit:?}")
}

/// The replay-depth policy as the generated `MAX_REPLAY_DEPTH` initializer.
/// Resolved at generation time — the ceiling guards the native stack, which
/// does not scale with the input, so there is nothing to resolve per run.
pub(super) fn depth_expr(limit: Limit, max_reader_frame_bytes: u64) -> String {
    match limit {
        Limit::Auto => format!("Some(rt::replay_depth_auto({max_reader_frame_bytes}))"),
        Limit::Of(n) => format!("Some({n})"),
        Limit::Unbounded => "None".to_string(),
    }
}

/// Split camel/Pascal humps: a boundary opens before an uppercase following a
/// non-uppercase, and before the last uppercase of an acronym run
/// (`HTTPServer` → `HTTP`, `Server`).
fn case_segments(name: &str) -> impl Iterator<Item = String> {
    let chars: Vec<char> = name.chars().collect();
    let mut segments = Vec::new();
    let mut current = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if c == '_' {
            if !current.is_empty() {
                segments.push(std::mem::take(&mut current));
            }
            continue;
        }
        let prev_upper = i > 0 && chars[i - 1].is_uppercase();
        let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
        let boundary = c.is_uppercase() && !current.is_empty() && (!prev_upper || next_lower);
        if boundary {
            segments.push(std::mem::take(&mut current));
        }
        current.push(c);
    }
    if !current.is_empty() {
        segments.push(current);
    }
    segments.into_iter()
}

/// Splice a template into `out`: substitute `@KEY@` placeholders, then indent
/// every non-empty line. Templates are written at column 0 so the emitted
/// shape reads directly in this file.
fn splice(out: &mut String, indent: &str, template: &str, subs: &[(&str, &str)]) {
    let mut text = template.trim_matches('\n').to_string();
    for (key, value) in subs {
        text = text.replace(&format!("@{key}@"), value);
    }
    for line in text.lines() {
        if line.is_empty() {
            out.push('\n');
            continue;
        }
        out.push_str(indent);
        out.push_str(line);
        out.push('\n');
    }
}

const HEADER: &str = r#"
// Generated Plotnik query module: typed output types, `parse`/`matches` entry
// points, per-type trace readers, and the compiled matcher (`mod matcher`).
// Matcher states mirror the NFA dump's labels 1:1 (`S{label}_{DEF}`), and every
// dispatch arm carries its instruction in the dump format
// (docs/binary-format/08-dump-format.md).

use @RT@ as rt;
"#;

const MOD_HEADER: &str = r#"
use @RT@ as rt;

/// The limit policy compiled into the safe entry points, resolved against
/// each input's node count. Chosen at generation time, never at the call
/// site: the query is trusted, the input is not.
const LIMITS: rt::RuntimeLimitSpec = rt::RuntimeLimitSpec {
    steps: @STEPS@,
    memory: @MEMORY@,
};

/// No ceilings — what the unmetered trace entry points run under.
const NO_LIMITS: rt::ResolvedRuntimeLimits = rt::ResolvedRuntimeLimits {
    max_steps: None,
    max_memory: None,
};

/// Bitmask selecting the dispatch steps on which the memory ceiling is
/// sampled; must be a power of two minus one. Twin of the VM's constant.
const MEMORY_SAMPLE_MASK: u64 = 1024 - 1;

/// Conservative maximum native-stack bytes used by one typed replay reader
/// frame before runtime padding.
pub(super) const MAX_READER_FRAME_BYTES: u64 = @READER_FRAME@;

/// Ceiling on recursive typed replay for safe `parse` (`None` opts out). The
/// matcher itself is iterative; only reader recursion enters this guard.
pub(super) const MAX_REPLAY_DEPTH: Option<u64> = @DEPTH@;

/// Resolve [`LIMITS`] against this input's node count, exactly like
/// `VM::builder(...).build()` resolves the VM's.
fn resolved_limits(tree: &rt::Tree) -> rt::ResolvedRuntimeLimits {
    let source_nodes = u32::try_from(tree.root_node().descendant_count()).unwrap_or(u32::MAX);
    LIMITS.resolve(source_nodes)
}
"#;

const VERIFY_LANGUAGE: &str = r#"
/// A parser built from any other grammar version could renumber the baked
/// kind/field ids and silently mis-match, so mismatches panic: version skew
/// between the generation-time grammar and the runtime parser is a build
/// mistake, not a runtime condition to recover from.
///
/// Every `run` checks its own tree — the walk is a handful of id lookups,
/// noise next to a match — so the guarantee holds per call, not per process:
/// a process that mixes languages (or grammar versions of one language) must
/// fail on the wrong tree, not only on the first one it ever saw.
fn verify_language(tree: &rt::Tree) {
    let language = tree.language();
    for &(id, name, named) in EXPECTED_KINDS {
        let found = language.node_kind_for_id(id);
        if found != Some(name) || language.node_kind_is_named(id) != named {
            panic!(
                "grammar version skew: this query module was generated against a \
                 grammar where node kind {id} is {name:?}, but the tree's language \
                 says {found:?} — rebuild the module with the grammar of the parser \
                 that produced the tree",
            );
        }
    }
    for &(id, name) in EXPECTED_FIELDS {
        let found = language.field_name_for_id(id);
        if found != Some(name) {
            panic!(
                "grammar version skew: this query module was generated against a \
                 grammar where field {id} is {name:?}, but the tree's language \
                 says {found:?} — rebuild the module with the grammar of the parser \
                 that produced the tree",
            );
        }
    }
}
"#;

const ENTRY_FN: &str = r#"
/// Match the `@DEF@` entrypoint against `tree`. `Some` carries the committed
/// capture trace — the same effect stream the VM commits for this query.
pub fn @FN@<'t>(tree: &'t rt::Tree, source: &str) -> Option<rt::EffectLog<'t>> {
    let outcome = run::<false, false, true>(tree, source, @ENTRY@, NO_LIMITS);
    outcome.expect("an unmetered run cannot exceed a limit")
}
"#;

// The metering const generics (`@STEPS_METERED@`, `@MEMORY_METERED@`) are fixed
// at generation time from the compiled-in policy: an unbounded resource emits
// `false`, folding its check out of the monomorphized `run`. When both are
// unbounded there is nothing to resolve, so the entries pass `NO_LIMITS` and
// skip the per-call node count entirely.
const ENTRY_FN_SAFE: &str = r#"
/// [`@FN@`] under the module's compiled-in limits ([`LIMITS`]).
pub(super) fn @SAFE_FN@<'t>(
    tree: &'t rt::Tree,
    source: &str,
) -> Result<Option<rt::EffectLog<'t>>, rt::LimitExceeded> {
    run::<@STEPS_METERED@, @MEMORY_METERED@, true>(tree, source, @ENTRY@, @SAFE_LIMITS@)
}
"#;

const ENTRY_ACCEPTS_SAFE: &str = r#"
/// Whether `@DEF@` accepts, under [`LIMITS`], with data effects suppressed.
pub(super) fn @ACCEPTS_FN@(tree: &rt::Tree, source: &str) -> Result<bool, rt::LimitExceeded> {
    Ok(run::<@STEPS_METERED@, @MEMORY_METERED@, false>(tree, source, @ENTRY@, @SAFE_LIMITS@)?.is_some())
}
"#;

const DRIVER_SKELETON: &str = r#"
/// What a dispatched state hands back to the driver loop.
enum Flow {
    /// Continue at this state.
    Jump(u16),
    /// The entrypoint accepted; the effect log is the committed trace.
    Accept,
    /// The state failed; unwind the checkpoint stack.
    Backtrack,
}

/// How the backtrack unwind resumed execution.
enum Unwound {
    Resumed(u16),
    Accepted,
    NoMatch,
}

/// One dispatch loop serves every entrypoint; `entry` selects the wrapper.
/// `METERED_STEPS` and `METERED_MEMORY` gate the two budget checks
/// independently: each folds away when its resource is unbounded, so a fully
/// unbounded policy compiles to a plain loop that never reads `heap_bytes`.
/// When either is on, the loop head transcribes the VM's `execute_with_stats`.
/// `TRACE` controls whether data effects are recorded; `matches` disables it to
/// avoid output allocation and replay-depth failures. (No let-chains: generated
/// code targets the embedding crate's edition.)
fn run<'t, const METERED_STEPS: bool, const METERED_MEMORY: bool, const TRACE: bool>(
    tree: &'t rt::Tree,
    source: &str,
    entry: u16,
    limits: rt::ResolvedRuntimeLimits,
) -> Result<Option<rt::EffectLog<'t>>, rt::LimitExceeded> {
    verify_language(tree);
    let mut eng = if TRACE {
        rt::Engine::new(tree.walk())
    } else {
        rt::Engine::new_data_suppressed(tree.walk())
    };
    let mut steps: u64 = 0;
    let mut ip = entry;
    loop {
        if METERED_STEPS || METERED_MEMORY {
            // Step ceiling: bound total work. Folded out when steps are
            // unbounded; the counter still advances under a memory-only
            // policy because the sample cadence below rides on it.
            if METERED_STEPS {
                if let Some(max) = limits.max_steps {
                    if steps >= max {
                        return Err(rt::LimitExceeded::Steps(max));
                    }
                }
            }
            steps += 1;
            // Memory ceiling: the live runtime heap, sampled every
            // `MEMORY_SAMPLE_MASK + 1` dispatches. Per-step growth is bounded,
            // so the unobserved overshoot is noise (see the VM loop). Folded
            // out when memory is unbounded, so no `heap_bytes` read survives.
            if METERED_MEMORY && steps & MEMORY_SAMPLE_MASK == 0 {
                let used = eng.heap_bytes();
                if let Some(max) = limits.max_memory {
                    if used > max {
                        return Err(rt::LimitExceeded::Memory { used, limit: max });
                    }
                }
            }
        }
        match step(&mut eng, source, ip) {
            Flow::Jump(next) => ip = next,
            Flow::Accept => return Ok(Some(eng.into_effects())),
            Flow::Backtrack => match backtrack(&mut eng, source) {
                Unwound::Resumed(next) => ip = next,
                Unwound::Accepted => return Ok(Some(eng.into_effects())),
                Unwound::NoMatch => return Ok(None),
            },
        }
    }
}
"#;

const STEP_OPEN: &str = r#"
fn step<'t>(eng: &mut rt::Engine<'t>, @SOURCE@: &str, ip: u16) -> Flow {
    match ip {
"#;

const STEP_CLOSE: &str = "        _ => unreachable!(\"ip {ip} is not a generated state\"),
    }
}
";

const NAVIGATE_OR_BACKTRACK: &str = r#"
if eng.cursor_mut().navigate(@NAV@).is_none() {
    break 'state Flow::Backtrack;
}
"#;

const CANDIDATE_ONCE: &str = r#"
if !@CAND@ {
    @FAIL@
}
"#;

const CANDIDATE_LOOP: &str = r#"
loop {
    if @CAND@ {
        break;
    }
    if !eng.cursor_mut().continue_search(@POLICY@) {
        @FAIL@
    }
}
"#;

const RETRY_CHECKPOINT: &str = r#"
if @POLICY@.admits(&eng.node()) {
    eng.push_checkpoint(rt::Checkpoint::match_retry(eng.checkpoint_state(), @STATE@));
}
"#;

const CHECK: &str = r#"
// @COMMENT@
if @FAIL@ {
    return false;
}
"#;

const CALL_FIELD_CHECK: &str = r#"
if eng.cursor().field_id() != Some(@FIELD@) {
    break 'state Flow::Backtrack;
}
"#;

const CALL_FIELD_SCAN: &str = r#"
loop {
    if eng.cursor().field_id() == Some(@FIELD@) {
        break;
    }
    if !eng.cursor_mut().continue_search(@POLICY@) {
        break 'state Flow::Backtrack;
    }
}
"#;

const CALL_RETRY_PUSH: &str = r#"
eng.push_checkpoint(rt::Checkpoint::call_retry(
    eng.checkpoint_state(),
    @STATE@,
    rt::CallResume { target: @TARGET@, next: @NEXT@, field: @FIELD@, policy: @POLICY@ },
));
"#;

const RETURN_ARM: &str = r#"
@STATE@ => {
    if eng.frames_empty() {
        Flow::Accept
    } else {
        Flow::Jump(eng.exit_frame())
    }
}
"#;

const FINISH_FN_OPEN: &str = r#"
/// `@STATE@` post-acceptance: effects, then branch. Shared by the dispatch
/// path and the match-retry resume, so a retried candidate replays exactly
/// what the original acceptance would have.
#[inline]
fn finish_@STEM@(@ENG@: &mut rt::Engine<'_>) -> Flow {
"#;

const BACKTRACK_SKELETON: &str = r#"
/// Unwind the checkpoint stack: branch alternatives resume dispatch, Call and
/// Match checkpoints advance their sibling search and re-enter. Loops, never
/// recurses — a run of exhausted retries unwinds in one call.
fn backtrack<'t>(eng: &mut rt::Engine<'t>, @SOURCE@: &str) -> Unwound {
    'unwind: loop {
        let Some((cp, snapshot)) = eng.pop_checkpoint() else {
            return Unwound::NoMatch;
        };
        eng.restore_checkpoint_state(cp.state, snapshot);

        match cp.resume {
            rt::Resume::Branch => return Unwound::Resumed(cp.ip),

            // Call retry: advance to the next candidate satisfying the field
            // constraint, then re-enter the callee. Exhausted siblings keep
            // unwinding to an earlier checkpoint.
            rt::Resume::Call(resume) => {
                if !eng.cursor_mut().continue_search(resume.policy) {
                    continue 'unwind;
                }
                if let Some(field_id) = resume.field {
                    loop {
                        if eng.cursor().field_id() == Some(field_id) {
                            break;
                        }
                        if !eng.cursor_mut().continue_search(resume.policy) {
                            continue 'unwind;
                        }
                    }
                }
                eng.push_checkpoint(rt::Checkpoint::call_retry(
                    eng.checkpoint_state(),
                    cp.ip,
                    resume,
                ));
                eng.enter_frame(resume.next);
                return Unwound::Resumed(resume.target);
            }

            // Match retry: step past the accepted-but-failed candidate and
            // re-run that state's sibling search from there.
            rt::Resume::Match => match match_retry(eng, @SOURCE@, cp.ip) {
                Some(Flow::Jump(next)) => return Unwound::Resumed(next),
                Some(Flow::Accept) => return Unwound::Accepted,
                Some(Flow::Backtrack) => unreachable!("finish never backtracks"),
                None => continue 'unwind,
            },
        }
    }
}
"#;

const MATCH_RETRY_OPEN: &str = r#"
fn match_retry<'t>(@ENG@: &mut rt::Engine<'t>, @SOURCE@: &str, ip: u16) -> Option<Flow> {
    match ip {
"#;

const MATCH_RETRY_CLOSE: &str = "        _ => unreachable!(\"match-retry checkpoint ip {ip} must address a sibling-search Match\"),
    }
}
";

const RETRY_ADVANCE: &str = r#"
if !eng.cursor_mut().continue_search(@POLICY@) {
    return None;
}
"#;
