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
//! `plotnik_rt::Engine`; per-state arms render the target-neutral matcher plan
//! with operands folded to constants. When the VM engine changes shape, the
//! sibling text here must follow — the 06-vm conformance corpus is the tripwire.
//!
//! All emitted shapes live as column-0 raw-string templates at the bottom of
//! this file; the shared template splicer substitutes `@KEY@` placeholders
//! and re-indents.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::bytecode::{EffectKind, PredicateOp};
use crate::compiler::analyze::AnalysisArtifacts;
use crate::compiler::codegen::Config;
use crate::compiler::codegen::plan::{
    CallPlan, CheckPlan, EffectPlan, FlowPlan, KindClass, LimitsPlan, MatchPlan, ModulePlan,
    PredicatePlan, PredicateValuePlan, RegexId, StateId, StateOrigin, StatePlan, StatePlanKind,
};
use crate::compiler::codegen::reader::ReaderGen;
use crate::compiler::lower::ir::{NfaGraph, SemanticNfa};
use crate::compiler::regex::compile_native_dfa;
use crate::compiler::srcgen::lits::{decimal_byte_lines, rust_string};
use crate::compiler::srcgen::names::{shouty_ident, snake_ident};
use crate::compiler::srcgen::sink::Sink;
use crate::compiler::srcgen::template::splice;
use crate::compiler::typegen::rust::TypeModel;
use plotnik_rt::{Limit, Nav, SkipPolicy};

use super::names::{accepts_entry_fn_name, entry_fn_name, safe_entry_fn_name};

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

struct RegexStatic {
    id: RegexId,
    bytes: Vec<u8>,
}

impl RegexStatic {
    fn compile(id: RegexId, pattern: &regex_syntax::hir::Hir) -> Self {
        let bytes = compile_native_dfa(pattern).expect("regex predicate compiled during emit");
        Self { id, bytes }
    }
}

/// Rust-only representation decisions over the neutral matcher plan.
struct RustRepresentation {
    states: Vec<StateInfo>,
    /// Field-id consts: raw id → `F_{NAME}` const name. Keyed by id and
    /// collision-suffixed at insert, so the const namespace stays injective.
    fields: BTreeMap<u16, String>,
    /// Rust uses regex-automata's native serialized sparse-DFA format compiled
    /// from the same normalized semantics dynamic backends print.
    regexes: Vec<RegexStatic>,
}

impl RustRepresentation {
    fn from_plan(plan: &crate::compiler::codegen::plan::MatcherPlan) -> Self {
        let width = plan.label_width();
        let states = plan
            .states()
            .iter()
            .map(|state| {
                let suffix = match state.origin {
                    StateOrigin::Definition => "",
                    StateOrigin::ConsumingDefinition => "_plus",
                    StateOrigin::Entrypoint => "_ep",
                };
                StateInfo {
                    id: state.id.raw(),
                    const_name: format!(
                        "S{:0width$}_{}{}",
                        state.label.0,
                        shouty_ident(&state.definition),
                        suffix.to_uppercase()
                    ),
                    fn_stem: format!(
                        "s{:0width$}_{}{}",
                        state.label.0,
                        snake_ident(&state.definition),
                        suffix
                    ),
                }
            })
            .collect();

        let mut representation = Self {
            states,
            fields: BTreeMap::new(),
            regexes: plan
                .regexes()
                .iter()
                .map(|regex| RegexStatic::compile(regex.id, &regex.normalized))
                .collect(),
        };
        for field in plan.fields() {
            representation.record_field(field.id, &field.name);
        }
        representation
    }

    fn record_field(&mut self, id: u16, display: &str) {
        if self.fields.contains_key(&id) {
            return;
        }
        // Distinct grammar field names can collapse to one SHOUTY form
        // (`fooBar` / `foo_bar`); suffix until free so a collision can never
        // silently alias two ids under one const.
        let mut name = format!("F_{}", shouty_ident(display));
        while self.fields.values().any(|taken| *taken == name) {
            let _ = write!(name, "_{id}");
        }
        self.fields.insert(id, name);
    }

    fn state(&self, id: StateId) -> &StateInfo {
        self.states
            .get(usize::from(id.raw()))
            .expect("every planned state has a Rust representation")
    }
}

struct Generator<'a> {
    config: &'a Config,
    plan: ModulePlan<'a>,
    rust: RustRepresentation,
}

impl<'a> Generator<'a> {
    fn new(graph: &'a NfaGraph, artifacts: AnalysisArtifacts<'a>, config: &'a Config) -> Self {
        let limits = LimitsPlan::new(config.limits.steps, config.limits.memory, config.depth);
        let plan = ModulePlan::build(graph, artifacts, limits);
        let rust = RustRepresentation::from_plan(plan.matcher());
        Self { config, plan, rust }
    }

    fn state(&self, id: StateId) -> &StateInfo {
        self.rust.state(id)
    }

    fn field_const(&self, field: u16) -> String {
        self.rust
            .fields
            .get(&field)
            .expect("every rendered field was recorded during operand collection")
            .clone()
    }

    fn regex_static(&self, id: RegexId) -> String {
        format!("RE_{}", id.index())
    }

    fn render(&self) -> String {
        let rust_config = &self.config.rust_types;
        let artifacts = self.plan.artifacts();
        let type_model = TypeModel::new(self.plan.output().clone());
        let readers = ReaderGen::new(artifacts, &type_model, self.plan.replay());

        let mut out = String::new();
        self.header(&mut out);
        out.push('\n');
        out.push_str(&crate::compiler::typegen::rust::emit_model(
            &type_model,
            rust_config,
        ));
        out.push_str(
            &readers.parse_api(
                self.plan
                    .matcher()
                    .entrypoints()
                    .iter()
                    .map(|entry| entry.definition),
            ),
        );
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
        let mut nested = Sink::<()>::new();
        nested.indented(|nested| nested.lines(&machinery));
        out.push_str(nested.plain());
        out.push_str("}\n");
        out
    }

    fn header(&self, out: &mut String) {
        if let Some(identity) = &self.config.grammar_identity {
            let _ = writeln!(out, "// Grammar name: {:?}", identity.name);
            let _ = writeln!(out, "// Grammar SHA-256: {}", identity.sha256);
            let _ = writeln!(out, "// Grammar source: {:?}", identity.source);
        }
        splice(out, "", HEADER, &[("RT", self.config.rt_crate_path())]);
    }

    /// `pub use` every trace entry point at module root, so the public
    /// surface (`{def}_trace`, per [`entry_fn_name`]) doesn't move when the
    /// machinery does.
    fn entry_reexports(&self, out: &mut String) {
        let names: Vec<String> = self
            .plan
            .matcher()
            .entrypoints()
            .iter()
            .map(|entry| entry_fn_name(&entry.name))
            .collect();
        out.push('\n');
        let _ = writeln!(out, "pub use self::matcher::{{{}}};", names.join(", "));
    }

    fn mod_header(&self, out: &mut String, max_reader_frame_bytes: u64) {
        let limits = self.plan.limits();
        splice(
            out,
            "",
            MOD_HEADER,
            &[
                ("RT", self.config.rt_crate_path()),
                ("STEPS", &limit_expr(limits.steps)),
                ("MEMORY", &limit_expr(limits.memory)),
                ("READER_FRAME", &max_reader_frame_bytes.to_string()),
                (
                    "DEPTH",
                    &depth_expr(limits.replay_depth, max_reader_frame_bytes),
                ),
            ],
        );
    }

    /// The language-skew tables and their assert: every kind and field id in
    /// this module is a numeric bake of the generation-time grammar, so the
    /// first run checks each one against the tree's live language.
    fn language_check(&self, out: &mut String) {
        out.push('\n');
        if let Some(identity) = &self.config.grammar_identity {
            let _ = writeln!(
                out,
                "const GRAMMAR_NAME: &str = {};",
                rust_string(&identity.name)
            );
            let _ = writeln!(
                out,
                "const GRAMMAR_SHA256: &str = {};",
                rust_string(&identity.sha256)
            );
            let _ = writeln!(
                out,
                "const GRAMMAR_SOURCE: &str = {};",
                rust_string(&identity.source)
            );
            out.push('\n');
        }
        out.push_str(
            "/// Node-kind ids baked into the candidate checks: `(id, name, is_named)`\n\
             /// as the generation-time grammar defines them.\n",
        );
        let matcher = self.plan.matcher();
        if matcher.expected_kinds().is_empty() {
            out.push_str("const EXPECTED_KINDS: &[(u16, &str, bool)] = &[];\n");
        } else {
            out.push_str("const EXPECTED_KINDS: &[(u16, &str, bool)] = &[\n");
            for expected in matcher.expected_kinds() {
                let id = expected.id;
                let named = expected.named;
                let name = rust_string(&expected.name);
                let _ = writeln!(out, "    ({id}, {name}, {named}),");
            }
            out.push_str("];\n");
        }
        out.push('\n');
        out.push_str("/// Field ids baked into the field checks: `(id, name)`.\n");
        if matcher.expected_fields().is_empty() {
            out.push_str("const EXPECTED_FIELDS: &[(u16, &str)] = &[];\n");
        } else {
            out.push_str("const EXPECTED_FIELDS: &[(u16, &str)] = &[\n");
            for field in matcher.expected_fields() {
                let id = field.id;
                let name = rust_string(&field.name);
                let _ = writeln!(out, "    ({id}, {name}),");
            }
            out.push_str("];\n");
        }
        out.push('\n');
        let template = if self.config.grammar_identity.is_some() {
            VERIFY_LANGUAGE_WITH_IDENTITY
        } else {
            VERIFY_LANGUAGE
        };
        splice(out, "", template, &[]);
    }

    fn field_consts(&self, out: &mut String) {
        if self.rust.fields.is_empty() {
            return;
        }
        out.push('\n');
        for (id, name) in &self.rust.fields {
            let _ = writeln!(
                out,
                "const {name}: rt::NodeFieldId = rt::NodeFieldId::from_raw({id});"
            );
        }
    }

    fn regex_statics(&self, out: &mut String) {
        for (plan, regex) in self.plan.matcher().regexes().iter().zip(&self.rust.regexes) {
            let pattern = &plan.pattern;
            out.push('\n');
            let _ = writeln!(out, "// /{pattern}/ — serialized sparse DFA");
            let _ = writeln!(
                out,
                "static RE_{}: rt::StaticDfa = rt::StaticDfa::new(&[",
                regex.id.index()
            );
            out.push_str(&decimal_byte_lines(&regex.bytes, 16, "    "));
            out.push_str("]);\n");
        }
    }

    fn state_consts(&self, out: &mut String) {
        out.push('\n');
        out.push_str("// Dense runtime state ids, in NFA label order.\n");
        let mut current: Option<&str> = None;
        for state in self.plan.matcher().states() {
            let def = state.definition.as_str();
            if current != Some(def) {
                let _ = writeln!(out, "// {def}:");
                current = Some(def);
            }
            let info = self.state(state.id);
            let _ = writeln!(out, "const {}: u16 = {};", info.const_name, info.id);
        }
    }

    fn entry_fns(&self, out: &mut String) {
        // An unbounded resource emits `false` for its metering const, folding
        // the check out of `run`. With both unbounded there is no ceiling to
        // resolve, so the safe entries pass `NO_LIMITS` rather than pay for a
        // per-call node count that nothing reads.
        let limits = self.plan.limits();
        let steps_metered = limits.steps != Limit::Unbounded;
        let memory_metered = limits.memory != Limit::Unbounded;
        let safe_limits = if steps_metered || memory_metered {
            "resolved_limits(tree)"
        } else {
            "NO_LIMITS"
        };
        let steps_metered = if steps_metered { "true" } else { "false" };
        let memory_metered = if memory_metered { "true" } else { "false" };
        for entry in self.plan.matcher().entrypoints() {
            let def = entry.name.as_str();
            let info = self.state(entry.entry);
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
        let source_param = if self.plan.matcher().any_predicate() {
            "source"
        } else {
            "_source"
        };
        out.push('\n');
        splice(out, "", STEP_OPEN, &[("SOURCE", source_param)]);
        for state in self.plan.matcher().states() {
            for line in state.provenance.lines() {
                let _ = writeln!(out, "        // {}", line.trim_end());
            }
            match &state.kind {
                StatePlanKind::Epsilon { effects, flow } => {
                    self.epsilon_arm(out, state, effects, flow);
                }
                StatePlanKind::Match(plan) => self.match_arm(out, state, plan),
                StatePlanKind::Call(plan) => self.call_arm(out, state, plan),
                StatePlanKind::Return => {
                    splice(
                        out,
                        "        ",
                        RETURN_ARM,
                        &[("STATE", &self.state(state.id).const_name)],
                    );
                }
            }
        }
        out.push_str(STEP_CLOSE);
    }

    fn epsilon_arm(
        &self,
        out: &mut String,
        state: &StatePlan,
        effects: &[EffectPlan],
        flow: &FlowPlan,
    ) {
        let info = self.state(state.id);
        let _ = writeln!(out, "        {} => {{", info.const_name);
        self.effects_and_flow(out, effects, flow, "            ");
        out.push_str("        }\n");
    }

    /// The dispatch arm for a Match instruction. Epsilon runs effects and
    /// branches; everything else navigates, searches candidates, leaves a
    /// retry checkpoint when the engine owns the sibling search, then runs
    /// the shared finish (effects + branch).
    fn match_arm(&self, out: &mut String, state: &StatePlan, plan: &MatchPlan) {
        let info = self.state(state.id);

        if plan.can_fail_before_flow() {
            let _ = writeln!(out, "        {} => 'state: {{", info.const_name);
        } else {
            let _ = writeln!(out, "        {} => {{", info.const_name);
        }

        if plan.navigates() {
            splice(
                out,
                "            ",
                NAVIGATE_OR_BACKTRACK,
                &[("NAV", &nav_expr(plan.nav))],
            );
        }

        self.candidate_search(
            out,
            state,
            plan,
            "            ",
            CandidateFailure::StateBacktrack,
        );
        self.retry_checkpoint(out, state, plan, "            ");

        if plan.retry.is_some() {
            let _ = writeln!(out, "            finish_{}(eng)", info.fn_stem);
        } else {
            self.effects_and_flow(out, &plan.effects, &plan.flow, "            ");
        }
        out.push_str("        }\n");
    }

    /// The candidate loop: try the current node, step past rejected ones per
    /// the nav's skip policy. An `Exact` policy has exactly one candidate, so
    /// the loop degenerates to a single check.
    fn candidate_search(
        &self,
        out: &mut String,
        state: &StatePlan,
        plan: &MatchPlan,
        indent: &str,
        failure: CandidateFailure,
    ) {
        if !plan.has_candidate_checks() {
            return;
        }
        let call = self.cand_call(state, plan);
        let fail = failure.code();
        if plan.search == SkipPolicy::Exact {
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
                ("POLICY", &policy_expr(plan.search)),
                ("FAIL", fail),
            ],
        );
    }

    /// Accepting a candidate in an engine-owned sibling search leaves a
    /// match-retry checkpoint, gated on the policy admitting the node into
    /// the pattern's gap — the VM's `push_match_retry_if_resumable`.
    fn retry_checkpoint(
        &self,
        out: &mut String,
        state: &StatePlan,
        plan: &MatchPlan,
        indent: &str,
    ) {
        let Some(policy) = plan.retry else {
            return;
        };
        splice(
            out,
            indent,
            RETRY_CHECKPOINT,
            &[
                ("POLICY", &policy_expr(policy)),
                ("STATE", &self.state(state.id).const_name),
            ],
        );
    }

    fn cand_call(&self, state: &StatePlan, plan: &MatchPlan) -> String {
        let stem = &self.state(state.id).fn_stem;
        if plan.has_predicate() {
            format!("cand_{stem}(eng, source)")
        } else {
            format!("cand_{stem}(eng)")
        }
    }

    /// Post-acceptance effects, then the successor branch — inline for states
    /// with no retry, `finish_*` fns where the backtrack path replays them.
    fn effects_and_flow(
        &self,
        out: &mut String,
        effects: &[EffectPlan],
        flow: &FlowPlan,
        indent: &str,
    ) {
        for effect in effects {
            self.effect_stmt(out, effect, indent);
        }
        match flow {
            FlowPlan::Accept => {
                let _ = writeln!(out, "{indent}Flow::Accept");
            }
            FlowPlan::Jump(next) => {
                let _ = writeln!(out, "{indent}Flow::Jump({})", self.state(*next).const_name);
            }
            FlowPlan::Branch { next, alternatives } => {
                let alt_names = alternatives
                    .iter()
                    .map(|alt| self.state(*alt).const_name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = writeln!(out, "{indent}eng.push_branches(&[{alt_names}]);");
                let _ = writeln!(out, "{indent}Flow::Jump({})", self.state(*next).const_name);
            }
        }
    }

    fn effect_stmt(&self, out: &mut String, effect: &EffectPlan, indent: &str) {
        let unit = |out: &mut String, variant: &str| {
            let _ = writeln!(
                out,
                "{indent}eng.emit_data(|_| rt::RuntimeEffect::{variant});"
            );
        };
        match effect.kind {
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
                let variant = if effect.kind == EffectKind::Set {
                    "Set"
                } else {
                    "EnumOpen"
                };
                let _ = writeln!(
                    out,
                    "{indent}eng.emit_data(|_| rt::RuntimeEffect::{variant}({})); // {}",
                    effect.payload, effect.display
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
    fn call_arm(&self, out: &mut String, state_plan: &StatePlan, plan: &CallPlan) {
        let state = &self.state(state_plan.id).const_name;
        let target = &self.state(plan.target).const_name;
        let next = &self.state(plan.next).const_name;
        let stays_on_current_node = plan.stays_on_current_node();

        if plan.can_fail_before_flow() {
            let _ = writeln!(out, "        {state} => 'state: {{");
        } else {
            let _ = writeln!(out, "        {state} => {{");
        }

        if stays_on_current_node {
            if let Some(field) = plan.field {
                splice(
                    out,
                    "            ",
                    CALL_FIELD_CHECK,
                    &[("FIELD", &self.field_const(field))],
                );
            }
        } else {
            splice(
                out,
                "            ",
                NAVIGATE_OR_BACKTRACK,
                &[("NAV", &nav_expr(plan.nav))],
            );
            if let Some(field) = plan.field {
                splice(
                    out,
                    "            ",
                    CALL_FIELD_SCAN,
                    &[
                        ("FIELD", &self.field_const(field)),
                        ("POLICY", &policy_expr(plan.search)),
                    ],
                );
            }
        }

        if let Some(policy) = plan.retry {
            let field = match plan.field {
                Some(field) => format!("Some({})", self.field_const(field)),
                None => "None".to_string(),
            };
            splice(
                out,
                "            ",
                CALL_RETRY_PUSH,
                &[
                    ("STATE", state),
                    ("TARGET", target),
                    ("NEXT", next),
                    ("FIELD", &field),
                    ("POLICY", &policy_expr(policy)),
                ],
            );
        }

        let _ = writeln!(out, "            eng.enter_frame({next});");
        let _ = writeln!(out, "            Flow::Jump({target})");
        out.push_str("        }\n");
    }

    fn cand_fns(&self, out: &mut String) {
        for state in self.plan.matcher().states() {
            let StatePlanKind::Match(plan) = &state.kind else {
                continue;
            };
            if !plan.has_candidate_checks() {
                continue;
            }
            self.cand_fn(out, state, plan);
        }
    }

    /// The candidate check, mirroring the VM's `candidate_matches` order:
    /// kind, missing, field, negated fields, predicate.
    fn cand_fn(&self, out: &mut String, state: &StatePlan, plan: &MatchPlan) {
        let info = self.state(state.id);
        out.push('\n');
        let _ = writeln!(
            out,
            "/// `{}` candidate: `{}`.",
            info.const_name, plan.candidate_pattern
        );
        out.push_str("#[inline]\n");
        let source_param = if plan.has_predicate() {
            ", source: &str"
        } else {
            ""
        };
        let _ = writeln!(
            out,
            "fn cand_{}(eng: &rt::Engine<'_>{source_param}) -> bool {{",
            info.fn_stem
        );
        if plan.needs_node_binding() {
            out.push_str("    let node = eng.node();\n");
        }
        for check in self.candidate_checks(&plan.checks) {
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
    fn candidate_checks(&self, plans: &[CheckPlan]) -> Vec<CandidateCheck> {
        plans
            .iter()
            .map(|plan| match plan {
                CheckPlan::Kind(kind) => {
                    let (comment, named_failure) = match kind.class {
                        KindClass::Named => ("(_)", "!node.is_named()"),
                        KindClass::Anonymous => ("\"_\"", "node.is_named()"),
                    };
                    let Some(id) = kind.id else {
                        return CandidateCheck::new(comment, named_failure);
                    };
                    let name = kind
                        .name
                        .as_deref()
                        .expect("specific kind check carries its grammar name");
                    match kind.class {
                        KindClass::Named => CandidateCheck::new(
                            format!("({name})"),
                            format!("node.kind_id() != {id} || !node.is_named()"),
                        ),
                        KindClass::Anonymous => CandidateCheck::new(
                            format!("\"{name}\""),
                            format!("node.kind_id() != {id} || node.is_named()"),
                        ),
                    }
                }
                CheckPlan::Missing => CandidateCheck::new(
                    "(MISSING …): only nodes the parser inserted during error recovery",
                    "!node.is_missing()",
                ),
                CheckPlan::Field(field) => CandidateCheck::new(
                    format!("{}:", field.name),
                    format!(
                        "eng.cursor().field_id() != Some({})",
                        self.field_const(field.id)
                    ),
                ),
                CheckPlan::NegField(field) => CandidateCheck::new(
                    format!("-{}", field.name),
                    format!(
                        "node.child_by_field_id(u16::from({})).is_some()",
                        self.field_const(field.id)
                    ),
                ),
                CheckPlan::Predicate(predicate) => self.predicate_check(predicate),
            })
            .collect()
    }

    fn predicate_check(&self, predicate: &PredicatePlan) -> CandidateCheck {
        let text = "rt::node_text(source, &node)";
        match &predicate.value {
            PredicateValuePlan::String(value) => {
                let lit = rust_string(value);
                let fail = match predicate.op {
                    PredicateOp::Eq => format!("{text} != {lit}"),
                    PredicateOp::Ne => format!("{text} == {lit}"),
                    PredicateOp::StartsWith => format!("!{text}.starts_with({lit})"),
                    PredicateOp::EndsWith => format!("!{text}.ends_with({lit})"),
                    PredicateOp::Contains => format!("!{text}.contains({lit})"),
                    PredicateOp::RegexMatch | PredicateOp::RegexNoMatch => {
                        unreachable!("regex predicate carries a regex value")
                    }
                };
                CandidateCheck::new(format!("{} {lit}", predicate.op.as_str()), fail)
            }
            PredicateValuePlan::Regex { id, pattern } => {
                let re = self.regex_static(*id);
                let fail = match predicate.op {
                    PredicateOp::RegexMatch => format!("!{re}.is_match({text})"),
                    PredicateOp::RegexNoMatch => format!("{re}.is_match({text})"),
                    _ => unreachable!("string predicate carries a string value"),
                };
                CandidateCheck::new(format!("{} /{pattern}/", predicate.op.as_str()), fail)
            }
        }
    }

    fn finish_fns(&self, out: &mut String) {
        for state in self.plan.matcher().states() {
            let StatePlanKind::Match(plan) = &state.kind else {
                continue;
            };
            if plan.retry.is_none() {
                continue;
            }
            let info = self.state(state.id);
            // `eng` feeds effect emission and branch pushes; a finish that
            // only jumps must not bind it, or every such fn warns.
            let eng = if plan.effects.is_empty() && !matches!(plan.flow, FlowPlan::Branch { .. }) {
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
            self.effects_and_flow(out, &plan.effects, &plan.flow, "    ");
            out.push_str("}\n");
        }
    }

    /// The backtrack unwind — the VM's `backtrack` with the Match arm
    /// dispatched to generated per-state retries.
    fn backtrack_fn(&self, out: &mut String) {
        let source_arg = if self.plan.matcher().any_retry_predicate() {
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
        let retryable: Vec<(&StatePlan, &MatchPlan)> = self
            .plan
            .matcher()
            .states()
            .iter()
            .filter_map(|state| match &state.kind {
                StatePlanKind::Match(plan) if plan.retry.is_some() => Some((state, plan)),
                _ => None,
            })
            .collect();

        let eng_param = if retryable.is_empty() { "_eng" } else { "eng" };
        let source_param = if self.plan.matcher().any_retry_predicate() {
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
        for (state, plan) in retryable {
            let info = self.state(state.id);
            let policy = plan
                .retry
                .expect("retryable match carries its exact skip policy");
            let _ = writeln!(out, "        {} => {{", info.const_name);
            splice(
                out,
                "            ",
                RETRY_ADVANCE,
                &[("POLICY", &policy_expr(policy))],
            );
            self.candidate_search(
                out,
                state,
                plan,
                "            ",
                CandidateFailure::RetryExhausted,
            );
            self.retry_checkpoint(out, state, plan, "            ");
            let _ = writeln!(out, "            Some(finish_{}(eng))", info.fn_stem);
            out.push_str("        }\n");
        }
        out.push_str(MATCH_RETRY_CLOSE);
    }
}

/// `Nav` as a generated-code expression; the `Debug` form matches the variant
/// syntax (`Up(2)`), pinned by `nav_expr_matches_debug` in the tests.
pub(super) fn nav_expr(nav: Nav) -> String {
    format!("rt::Nav::{nav:?}")
}

fn policy_expr(policy: SkipPolicy) -> String {
    format!("rt::SkipPolicy::{policy:?}")
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

const VERIFY_LANGUAGE_WITH_IDENTITY: &str = r#"
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
                "grammar version skew: this query module was generated against {} \
                 ({}, grammar.json SHA-256 {}) where node kind {id} is {name:?}, \
                 but the tree's language says {found:?} — regenerate against the \
                 grammar.json belonging to the parser that produced the tree",
                GRAMMAR_NAME,
                GRAMMAR_SOURCE,
                GRAMMAR_SHA256,
            );
        }
    }
    for &(id, name) in EXPECTED_FIELDS {
        let found = language.field_name_for_id(id);
        if found != Some(name) {
            panic!(
                "grammar version skew: this query module was generated against {} \
                 ({}, grammar.json SHA-256 {}) where field {id} is {name:?}, but \
                 the tree's language says {found:?} — regenerate against the \
                 grammar.json belonging to the parser that produced the tree",
                GRAMMAR_NAME,
                GRAMMAR_SOURCE,
                GRAMMAR_SHA256,
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
