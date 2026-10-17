use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::bytecode::{EffectKind, EntryBoundary, PredicateOp};
use crate::compiler::emit::plan::{
    CallPlan, CheckPlan, CodegenPlan, EffectPlan, FlowPlan, KindClass, MatchPlan, PredicatePlan,
    PredicateValuePlan, RegexId, StateId, StateOrigin, StatePlan, StatePlanKind,
};
use crate::compiler::emit::sink::Sink;
use crate::compiler::emit::targets::rust::Config;
use crate::compiler::emit::targets::rust::TypeModel;
use crate::compiler::emit::targets::rust::ident::{shouty_ident, snake_ident};
use crate::compiler::emit::targets::rust::literal::{decimal_byte_lines, rust_string};
use crate::compiler::emit::targets::rust::model::ResultModel;
use crate::compiler::emit::targets::rust::publish::{EntryPoint, PublishGen};
use crate::compiler::emit::targets::rust::template::splice;
use crate::compiler::regex::compile_native_dfa;
use crate::core::NodeFieldId;
use plotnik_runtime::{GENERATED_VALUE_CAP, Limit, Nav, SkipPolicy};

pub(crate) fn generate(plan: &CodegenPlan<'_>, config: &Config) -> Result<String, String> {
    let generator = Generator::new(plan, config)?;
    let types = TypeModel::new(*plan.result());
    let model = ResultModel::new(plan.result(), &types, plan.decode());
    check_value_sizes(&model)?;
    Ok(generator.render(&model))
}

/// Generated values are built and moved by value on the native stack, so
/// every result type must stay below the runtime's per-value cap.
fn check_value_sizes(model: &ResultModel) -> Result<(), String> {
    for item in model.items() {
        let ty = match item.shape {
            super::model::ItemShape::Alias(ty) => ty,
            _ => match item.ty {
                Some(ty) => ty,
                None => continue,
            },
        };
        let estimate = model.size_estimate(ty);
        if estimate > GENERATED_VALUE_CAP as u64 {
            return Err(format!(
                "result type `{}` is too large for generated Rust: an estimated {estimate} \
                 bytes by value, over the {GENERATED_VALUE_CAP}-byte limit",
                item.display
            ));
        }
    }
    Ok(())
}

struct StateInfo {
    id: u16,
    const_name: String,
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
            CandidateFailure::RetryExhausted => "return Ok(None);",
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
    fn compile(
        id: RegexId,
        source: &str,
        pattern: &regex_syntax::hir::Hir,
    ) -> Result<Self, String> {
        let bytes = compile_native_dfa(pattern)
            .map_err(|error| format!("regex compile error for {source:?}: {error}"))?;
        Ok(Self { id, bytes })
    }
}

struct RustRepresentation {
    states: Vec<StateInfo>,
    fields: BTreeMap<NodeFieldId, String>,
    regexes: Vec<RegexStatic>,
}

impl RustRepresentation {
    fn from_plan(plan: &crate::compiler::emit::plan::MatcherPlan) -> Result<Self, String> {
        let width = plan.label_width();
        let states = plan
            .states()
            .iter()
            .map(|state| {
                let suffix = match state.origin {
                    StateOrigin::Definition => "",
                    StateOrigin::ConsumingDefinition => "_plus",
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

        let regexes = plan
            .regexes()
            .iter()
            .map(|regex| RegexStatic::compile(regex.id, &regex.pattern, &regex.normalized))
            .collect::<Result<Vec<_>, _>>()?;
        let mut representation = Self {
            states,
            fields: BTreeMap::new(),
            regexes,
        };
        for field in plan.fields() {
            representation.record_field(field.id, &field.name);
        }
        Ok(representation)
    }

    fn record_field(&mut self, id: NodeFieldId, display: &str) {
        if self.fields.contains_key(&id) {
            return;
        }
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
    plan: &'a CodegenPlan<'a>,
    rust: RustRepresentation,
    has_calls: bool,
}

impl<'a> Generator<'a> {
    fn new(plan: &'a CodegenPlan<'a>, config: &'a Config) -> Result<Self, String> {
        let rust = RustRepresentation::from_plan(plan.matcher())?;
        let has_calls = plan
            .matcher()
            .states()
            .iter()
            .any(|state| matches!(state.kind, StatePlanKind::Call(_)));
        Ok(Self {
            config,
            plan,
            rust,
            has_calls,
        })
    }

    fn state(&self, id: StateId) -> &StateInfo {
        self.rust.state(id)
    }

    fn field_const(&self, field: NodeFieldId) -> String {
        self.rust
            .fields
            .get(&field)
            .expect("every rendered field was recorded during operand collection")
            .clone()
    }

    fn regex_static(&self, id: RegexId) -> String {
        format!("RE_{}", id.index())
    }

    fn render(&self, model: &ResultModel) -> String {
        let publish = PublishGen::new(model);
        let entries = self.entry_points(model);

        let mut out = String::new();
        self.header(&mut out);
        out.push('\n');
        out.push_str(&crate::compiler::emit::targets::rust::emit_model(
            model,
            &self.config.rust_types,
        ));
        out.push('\n');
        out.push_str(&publish.type_checks());
        out.push_str(&publish.json_impls());
        out.push_str(&publish.execution_api(&entries));
        out.push_str(&publish.publisher(&entries));

        let mut machinery = String::new();
        self.mod_header(&mut machinery);
        self.language_check(&mut machinery);
        self.field_consts(&mut machinery);
        self.regex_statics(&mut machinery);
        self.state_consts(&mut machinery);
        if self.has_calls {
            self.call_return_fn(&mut machinery);
        }
        self.entry_types(&mut machinery, &entries);
        self.driver_fn(&mut machinery);
        self.dispatch_fn(&mut machinery);
        self.cand_fns(&mut machinery);
        self.finish_fns(&mut machinery);
        self.backtrack_fn(&mut machinery);
        self.match_retry_fn(&mut machinery);

        out.push('\n');
        out.push_str("/// The compiled matcher: engine machinery shielded from the query's\n");
        out.push_str("/// type namespace.\n");
        out.push_str("mod matcher {\n");
        let mut nested = Sink::new();
        nested.indented(|nested| nested.lines(&machinery));
        out.push_str(nested.plain());
        out.push_str("}\n");
        out
    }

    fn entry_points<'m>(&self, model: &'m ResultModel) -> Vec<EntryPoint<'m>> {
        let definitions = self.plan.result().definitions;
        self.plan
            .matcher()
            .entrypoints()
            .iter()
            .map(|entry| {
                let item = model.item(definitions.definition(entry.definition).name());
                EntryPoint {
                    item,
                    entry: entry_type(&item.ident),
                }
            })
            .collect()
    }

    fn header(&self, out: &mut String) {
        if let Some(identity) = &self.config.grammar_identity {
            let _ = writeln!(out, "// Grammar name: {:?}", identity.name);
            let _ = writeln!(out, "// Grammar SHA-256: {}", identity.sha256);
            let _ = writeln!(out, "// Grammar source: {:?}", identity.source);
        }
        splice(
            out,
            "",
            HEADER,
            &[
                ("RT", self.config.rt_crate_path()),
                ("ABI", &plotnik_runtime::RUNTIME_ABI.to_string()),
            ],
        );
    }

    fn mod_header(&self, out: &mut String) {
        let limits = self.config.limits;
        splice(
            out,
            "",
            MOD_HEADER,
            &[
                ("RT", self.config.rt_crate_path()),
                ("FUEL", &limit_expr(limits.fuel_limit)),
                ("MEMORY", &limit_expr(limits.memory)),
            ],
        );
    }

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
        render_const_slice(
            out,
            "const EXPECTED_KINDS: &[(u16, &str, bool)]",
            matcher.expected_kinds().iter().map(|expected| {
                let id = expected.id;
                let named = expected.named;
                let name = rust_string(&expected.name);
                format!("({id}, {name}, {named})")
            }),
            2,
        );
        out.push('\n');
        out.push_str("/// Field ids baked into the field checks: `(id, name)`.\n");
        let mut expected_fields = matcher.fields().iter().collect::<Vec<_>>();
        expected_fields.sort_unstable_by_key(|field| field.id);
        render_const_slice(
            out,
            "const EXPECTED_FIELDS: &[(u16, &str)]",
            expected_fields.into_iter().map(|field| {
                let id = field.id;
                let name = rust_string(&field.name);
                format!("({id}, {name})")
            }),
            3,
        );
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
            out.push_str("#[rustfmt::skip]\n");
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

    fn call_return_fn(&self, out: &mut String) {
        out.push('\n');
        out.push_str(
            "/// Resolve a callee-local return port through the immutable call-site map.\n",
        );
        out.push_str("#[inline]\n");
        out.push_str("fn call_return(call_site: u16, port: rt::PortId) -> u16 {\n");
        out.push_str("    match (call_site, port.to_byte()) {\n");
        for state in self.plan.matcher().states() {
            let StatePlanKind::Call(plan) = &state.kind else {
                continue;
            };
            let call_site = &self.state(state.id).const_name;
            for (port, &target) in plan.returns.iter().enumerate() {
                let target = &self.state(target).const_name;
                let _ = writeln!(out, "        ({call_site}, {port}) => {target},");
            }
        }
        out.push_str("        _ => unreachable!(\n");
        out.push_str("            \"call site {call_site} has no return port {}\",\n");
        out.push_str("            port.to_byte()\n");
        out.push_str("        ),\n");
        out.push_str("    }\n");
        out.push_str("}\n");
    }

    fn entry_types(&self, out: &mut String, points: &[EntryPoint<'_>]) {
        for (entry, point) in self.plan.matcher().entrypoints().iter().zip(points) {
            let boundary = match entry.boundary {
                EntryBoundary::Passthrough => "EntryBoundary::new(false, false)",
                EntryBoundary::Node => "EntryBoundary::new(false, true)",
                EntryBoundary::Record => "EntryBoundary::new(true, false)",
            };
            let state = &self.state(entry.entry).const_name;
            let compact = format!("run(eng, m, source, {state}, {boundary})");
            let call = if compact.len() + 8 <= 100 {
                compact
            } else {
                format!(
                    "run(\n            eng,\n            m,\n            source,\n            {state},\n            {boundary},\n        )"
                )
            };
            out.push('\n');
            splice(
                out,
                "",
                ENTRY_TYPE,
                &[
                    ("DEF", entry.name.as_str()),
                    ("TYPE", &point.entry),
                    ("CALL", &call),
                ],
            );
        }
    }

    fn driver_fn(&self, out: &mut String) {
        splice(out, "", DRIVER_SKELETON, &[]);
    }

    fn dispatch_fn(&self, out: &mut String) {
        let source_param = if self.plan.matcher().any_predicate() {
            "source"
        } else {
            "_source"
        };
        let meter = if self.dispatch_meters() { "m" } else { "_m" };
        out.push('\n');
        splice(
            out,
            "",
            DISPATCH_OPEN,
            &[("SOURCE", source_param), ("METER", meter)],
        );
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
                StatePlanKind::Return(port) => {
                    let port = port.to_byte();
                    let top_level = if port == 0 {
                        "Flow::Accept".to_owned()
                    } else {
                        format!("panic!(\"entrypoint returned through nonzero port {port}\")")
                    };
                    let port = port.to_string();
                    let template = if self.has_calls {
                        RETURN_ARM
                    } else {
                        RETURN_ARM_NO_CALLS
                    };
                    splice(
                        out,
                        "        ",
                        template,
                        &[
                            ("STATE", &self.state(state.id).const_name),
                            ("PORT", &port),
                            ("TOP_LEVEL", &top_level),
                        ],
                    );
                }
            }
        }
        out.push_str(DISPATCH_CLOSE);
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

    fn match_arm(&self, out: &mut String, state: &StatePlan, plan: &MatchPlan) {
        let info = self.state(state.id);

        if plan.can_fail_before_flow() {
            let _ = writeln!(out, "        {} => 'state: {{", info.const_name);
        } else {
            let _ = writeln!(out, "        {} => {{", info.const_name);
        }

        if plan.navigates() {
            self.navigate_or_backtrack(out, plan.nav);
        }

        self.candidate_search(out, state, plan, CandidateFailure::StateBacktrack);
        self.retry_checkpoint(out, state, plan, "            ");

        if plan.retry.is_some() {
            let _ = writeln!(out, "            finish_{}(eng, m)?", info.fn_stem);
        } else {
            self.effects_and_flow(out, &plan.effects, &plan.flow, "            ");
        }
        out.push_str("        }\n");
    }

    fn candidate_search(
        &self,
        out: &mut String,
        state: &StatePlan,
        plan: &MatchPlan,
        failure: CandidateFailure,
    ) {
        if !plan.has_candidate_checks() {
            return;
        }
        let call = self.cand_call(state, plan);
        let fail = failure.code();
        let indent = "            ";
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
        let policy = policy_expr(policy);
        let state = &self.state(state.id).const_name;
        let compact_len = format!(
            "{indent}    eng.push_checkpoint(m, rt::Checkpoint::match_retry(eng.checkpoint_state(), {state}))?;"
        )
        .len()
            + 4;
        let template = if compact_len <= 100 {
            RETRY_CHECKPOINT_COMPACT
        } else {
            RETRY_CHECKPOINT_EXPANDED
        };
        splice(
            out,
            indent,
            template,
            &[("POLICY", &policy), ("STATE", state)],
        );
    }

    fn navigate_or_backtrack(&self, out: &mut String, nav: Nav) {
        let nav = nav_expr(nav);
        let chain = format!("eng.cursor_mut().navigate(m, {nav})?.is_none()");
        let template = if chain.len() <= 60 {
            NAVIGATE_OR_BACKTRACK
        } else {
            NAVIGATE_OR_BACKTRACK_EXPANDED
        };
        splice(out, "            ", template, &[("NAV", &nav)]);
    }

    fn cand_call(&self, state: &StatePlan, plan: &MatchPlan) -> String {
        let stem = &self.state(state.id).fn_stem;
        if plan.has_predicate() {
            format!("cand_{stem}(eng, m, source)?")
        } else {
            format!("cand_{stem}(eng, m)?")
        }
    }

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
            FlowPlan::Fork { next, successors } => {
                let successor_names = successors
                    .iter()
                    .map(|successor| self.state(*successor).const_name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = writeln!(
                    out,
                    "{indent}eng.push_successors(m, &[{successor_names}])?;"
                );
                let _ = writeln!(out, "{indent}Flow::Jump({})", self.state(*next).const_name);
            }
        }
    }

    fn effect_stmt(&self, out: &mut String, effect: &EffectPlan, indent: &str) {
        let unit = |out: &mut String, variant: &str| {
            let _ = writeln!(
                out,
                "{indent}eng.emit(m, |_| rt::JournalEvent::{variant})?;"
            );
        };
        match effect.kind {
            EffectKind::Node => {
                let _ = writeln!(
                    out,
                    "{indent}eng.emit(m, |c| rt::JournalEvent::Node(c.node()))?;"
                );
            }
            EffectKind::ListOpen => unit(out, "ListOpen"),
            EffectKind::ArrayPush => unit(out, "ArrayPush"),
            EffectKind::ListClose => unit(out, "ListClose"),
            EffectKind::RecordOpen => unit(out, "RecordOpen"),
            EffectKind::RecordClose => unit(out, "RecordClose"),
            EffectKind::VariantClose => unit(out, "VariantClose"),
            EffectKind::Absent => unit(out, "Absent"),
            EffectKind::RecordSet | EffectKind::VariantOpen => {
                let variant = if effect.kind == EffectKind::RecordSet {
                    "RecordSet"
                } else {
                    "VariantOpen"
                };
                let _ = writeln!(
                    out,
                    "{indent}eng.emit(m, |_| rt::JournalEvent::{variant}({}))?; // {}",
                    effect.payload, effect.display
                );
            }
            EffectKind::SuppressBegin => {
                let _ = writeln!(out, "{indent}eng.suppress_begin();");
            }
            EffectKind::SuppressEnd => {
                let _ = writeln!(out, "{indent}eng.suppress_end();");
            }
            EffectKind::ScalarOpen => {
                let _ = writeln!(out, "{indent}eng.scalar_open(m)?;");
            }
            EffectKind::ScalarMark => {
                let _ = writeln!(out, "{indent}eng.scalar_mark(m)?;");
            }
            EffectKind::TextClose => {
                let _ = writeln!(out, "{indent}eng.scalar_close_text(m)?;");
            }
            EffectKind::BoolClose => {
                let value = effect.payload != 0;
                let _ = writeln!(out, "{indent}eng.scalar_close_bool(m, {value})?;");
            }
            EffectKind::NodeText => {
                let _ = writeln!(out, "{indent}eng.node_text(m)?;");
            }
            EffectKind::NodeBool => {
                let _ = writeln!(out, "{indent}eng.node_bool(m)?;");
            }
            EffectKind::BoolValue => {
                let value = effect.payload != 0;
                let _ = writeln!(out, "{indent}eng.bool_value(m, {value})?;");
            }
            EffectKind::SpanStartAt | EffectKind::SpanStart | EffectKind::SpanEnd => {
                unreachable!("inspection spans rejected before generation")
            }
        }
    }

    fn call_arm(&self, out: &mut String, state_plan: &StatePlan, plan: &CallPlan) {
        if plan.caller_owned() {
            self.caller_owned_call_arm(out, state_plan, plan);
        } else {
            self.callee_owned_call_arm(out, state_plan, plan);
        }
    }

    fn caller_owned_call_arm(&self, out: &mut String, state_plan: &StatePlan, plan: &CallPlan) {
        let state = &self.state(state_plan.id).const_name;
        let target = &self.state(plan.target).const_name;
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
            self.navigate_or_backtrack(out, plan.nav);
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
                    ("CALL_SITE", state),
                    ("FIELD", &field),
                    ("POLICY", &policy_expr(policy)),
                ],
            );
        }

        let _ = writeln!(out, "            eng.enter_frame(m, {state})?;");
        let _ = writeln!(out, "            Flow::Jump({target})");
        out.push_str("        }\n");
    }

    fn callee_owned_call_arm(&self, out: &mut String, state_plan: &StatePlan, plan: &CallPlan) {
        let state = &self.state(state_plan.id).const_name;
        let target = &self.state(plan.target).const_name;
        let _ = writeln!(out, "        {state} => {{");
        let _ = writeln!(out, "            eng.enter_frame(m, {state})?;");
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

    fn cand_fn(&self, out: &mut String, state: &StatePlan, plan: &MatchPlan) {
        let info = self.state(state.id);
        out.push('\n');
        let _ = writeln!(
            out,
            "/// `{}` candidate: `{}`.",
            info.const_name, plan.candidate_pattern
        );
        out.push_str("#[inline]\n");
        let metered = plan.has_predicate();
        if metered {
            let _ = writeln!(
                out,
                "fn cand_{}(\n    eng: &mut rt::Engine<'_>,\n    m: &mut rt::Meter<'_>,\n    source: &str,\n) -> Result<bool, rt::LimitExceeded> {{",
                info.fn_stem
            );
        } else {
            let _ = writeln!(
                out,
                "fn cand_{}(eng: &mut rt::Engine<'_>, m: &mut rt::Meter<'_>) -> Result<bool, rt::LimitExceeded> {{",
                info.fn_stem
            );
        }
        out.push_str("    m.charge(1)?;\n");
        if plan.needs_node_binding() {
            out.push_str("    let node = eng.node();\n");
        }
        let (reject, accept) = ("return Ok(false);", "    Ok(true)\n}\n");
        for check in self.candidate_checks(&plan.checks) {
            splice(
                out,
                "    ",
                CHECK,
                &[
                    ("COMMENT", &check.comment),
                    ("FAIL", &check.fail),
                    ("REJECT", reject),
                ],
            );
        }
        out.push_str(accept);
    }

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
                        "!eng.cursor_mut().has_field(m, {})?",
                        self.field_const(field.id)
                    ),
                ),
                CheckPlan::NegField(field) => CandidateCheck::new(
                    format!("-{}", field.name),
                    format!(
                        "!eng.cursor_mut().field_absent(m, {})?",
                        self.field_const(field.id)
                    ),
                ),
                CheckPlan::Predicate(predicate) => self.predicate_check(predicate),
            })
            .collect()
    }

    fn predicate_check(&self, predicate: &PredicatePlan) -> CandidateCheck {
        let text = "rt::predicate_text(m, source, &node)?";
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
            let (eng, meter) = if self.flow_meters(&plan.effects, &plan.flow) {
                ("eng", "m")
            } else if plan.effects.is_empty() {
                ("_eng", "_m")
            } else {
                ("eng", "_m")
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
                    ("METER", meter),
                ],
            );
            out.push_str("    let flow = {\n");
            self.effects_and_flow(out, &plan.effects, &plan.flow, "        ");
            out.push_str("    };\n    Ok(flow)\n");
            out.push_str("}\n");
        }
    }

    fn backtrack_fn(&self, out: &mut String) {
        let source_arg = if self.plan.matcher().any_retry_predicate() {
            "source"
        } else {
            "_source"
        };
        let call_resume = if self.has_calls {
            BACKTRACK_CALL_RESUME
        } else {
            BACKTRACK_CALL_RESUME_UNREACHABLE
        };
        out.push('\n');
        splice(
            out,
            "",
            BACKTRACK_SKELETON,
            &[("SOURCE", source_arg), ("CALL_RESUME", call_resume)],
        );
    }

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

        let source_param = if self.plan.matcher().any_retry_predicate() {
            "source"
        } else {
            "_source"
        };
        out.push('\n');
        if retryable.is_empty() {
            splice(out, "", MATCH_RETRY_EMPTY, &[("SOURCE", source_param)]);
            return;
        }
        splice(
            out,
            "",
            MATCH_RETRY_OPEN,
            &[("ENG", "eng"), ("SOURCE", source_param)],
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
            self.candidate_search(out, state, plan, CandidateFailure::RetryExhausted);
            self.retry_checkpoint(out, state, plan, "            ");
            let _ = writeln!(out, "            finish_{}(eng, m).map(Some)", info.fn_stem);
            out.push_str("        }\n");
        }
        out.push_str(MATCH_RETRY_CLOSE);
    }
}

impl Generator<'_> {
    /// Whether any dispatch arm charges the meter. Unused parameters are
    /// named with a leading underscore to keep generated code warning-free.
    fn dispatch_meters(&self) -> bool {
        self.plan
            .matcher()
            .states()
            .iter()
            .any(|state| match &state.kind {
                StatePlanKind::Epsilon { effects, flow } => self.flow_meters(effects, flow),
                StatePlanKind::Match(plan) => {
                    plan.navigates()
                        || plan.has_predicate()
                        || plan.retry.is_some()
                        || plan.has_candidate_checks()
                        || self.flow_meters(&plan.effects, &plan.flow)
                }
                StatePlanKind::Call(_) => true,
                StatePlanKind::Return(_) => false,
            })
    }

    fn flow_meters(&self, effects: &[EffectPlan], flow: &FlowPlan) -> bool {
        matches!(flow, FlowPlan::Fork { .. })
            || effects.iter().any(|effect| {
                !matches!(
                    effect.kind,
                    EffectKind::SuppressBegin | EffectKind::SuppressEnd
                )
            })
    }
}

/// The matcher-module type implementing `rt::Entry` for one entrypoint.
fn entry_type(item_ident: &str) -> String {
    format!("{item_ident}Entry")
}

pub(super) fn nav_expr(nav: Nav) -> String {
    format!("rt::Nav::{nav:?}")
}

fn policy_expr(policy: SkipPolicy) -> String {
    format!("rt::SkipPolicy::{policy:?}")
}

fn render_const_slice(
    out: &mut String,
    declaration: &str,
    entries: impl Iterator<Item = String>,
    inline_entry_limit: usize,
) {
    let entries = entries.collect::<Vec<_>>();
    if entries.is_empty() {
        let _ = writeln!(out, "{declaration} = &[];");
        return;
    }

    let joined = entries.join(", ");
    let compact = format!("{declaration} = &[{joined}];");
    if compact.len() + 4 <= 100 {
        let _ = writeln!(out, "{compact}");
        return;
    }
    if entries.len() <= inline_entry_limit && joined.len() + 12 <= 100 {
        let _ = writeln!(out, "{declaration} =");
        let _ = writeln!(out, "    &[{joined}];");
        return;
    }

    let _ = writeln!(out, "{declaration} = &[");
    for entry in entries {
        let _ = writeln!(out, "    {entry},");
    }
    out.push_str("];\n");
}

pub(super) fn limit_expr(limit: Limit) -> String {
    format!("rt::Limit::{limit:?}")
}

const HEADER: &str = r#"
// Generated Plotnik query module: typed result types, supplied-node execution
// points, per-type result decoders, and the compiled matcher (`mod matcher`).
// Matcher states mirror the NFA dump's labels 1:1 (`S{label}_{DEF}`), and every
// dispatch arm carries its instruction in the dump format.

use @RT@ as rt;

pub const REQUIRED_RUNTIME_ABI: u32 = @ABI@;
const _: () = assert!(
    rt::RUNTIME_ABI == REQUIRED_RUNTIME_ABI,
    "Plotnik runtime ABI mismatch"
);
"#;

const MOD_HEADER: &str = r#"
use @RT@ as rt;

/// The limit policy compiled into the safe entrypoints, resolved against
/// each input's node count. Chosen at generation time, never at the call
/// site: the query is trusted, the input is not.
const LIMITS: rt::RuntimeLimitSpec = rt::RuntimeLimitSpec {
    fuel_limit: @FUEL@,
    memory: @MEMORY@,
};

"#;

const VERIFY_LANGUAGE: &str = r#"
/// A parser built from any other grammar version could renumber the baked
/// kind/field ids and silently mis-match, so mismatches panic: version skew
/// between the generation-time grammar and the runtime parser is a build
/// mistake, not a runtime condition to recover from.
pub(super) fn verify_language(node: rt::Node<'_>) {
    let language = node.language();
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
pub(super) fn verify_language(node: rt::Node<'_>) {
    let language = node.language();
    for &(id, name, named) in EXPECTED_KINDS {
        let found = language.node_kind_for_id(id);
        if found != Some(name) || language.node_kind_is_named(id) != named {
            panic!(
                "grammar version skew: this query module was generated against {} \
                 ({}, grammar.json SHA-256 {}) where node kind {id} is {name:?}, \
                 but the tree's language says {found:?} — regenerate against the \
                 grammar.json belonging to the parser that produced the tree",
                GRAMMAR_NAME, GRAMMAR_SOURCE, GRAMMAR_SHA256,
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
                GRAMMAR_NAME, GRAMMAR_SOURCE, GRAMMAR_SHA256,
            );
        }
    }
}
"#;

const ENTRY_TYPE: &str = r#"
pub(super) struct @TYPE@;

impl rt::Entry for @TYPE@ {
    const LIMITS: rt::RuntimeLimitSpec = LIMITS;

    fn verify_language(node: rt::Node<'_>) {
        verify_language(node);
    }

    fn run<'t>(
        eng: &mut rt::Engine<'t>,
        m: &mut rt::Meter<'_>,
        source: &str,
    ) -> Result<bool, rt::LimitExceeded> {
        @CALL@
    }
}
"#;

const DRIVER_SKELETON: &str = r#"
#[derive(Clone, Copy)]
struct EntryBoundary {
    record: bool,
    node: bool,
}

impl EntryBoundary {
    const fn new(record: bool, node: bool) -> Self {
        Self { record, node }
    }

    fn begin(self, eng: &mut rt::Engine<'_>, m: &mut rt::Meter<'_>) -> Result<(), rt::LimitExceeded> {
        if self.record {
            eng.emit(m, |_| rt::JournalEvent::RecordOpen)?;
        }
        Ok(())
    }

    fn finish(self, eng: &mut rt::Engine<'_>, m: &mut rt::Meter<'_>) -> Result<(), rt::LimitExceeded> {
        if self.node {
            eng.emit(m, |cursor| rt::JournalEvent::Node(cursor.node()))?;
            return Ok(());
        }
        if self.record {
            eng.emit(m, |_| rt::JournalEvent::RecordClose)?;
        }
        Ok(())
    }
}

/// What a dispatched state hands back to the driver loop.
enum Flow {
    /// Continue at this state.
    Jump(u16),
    /// The entrypoint accepted; the match journal is committed.
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

fn run<'t>(
    eng: &mut rt::Engine<'t>,
    m: &mut rt::Meter<'_>,
    source: &str,
    entry: u16,
    boundary: EntryBoundary,
) -> Result<bool, rt::LimitExceeded> {
    boundary.begin(eng, m)?;
    let mut ip = entry;
    loop {
        m.charge(1)?;
        match dispatch(eng, m, source, ip)? {
            Flow::Jump(next) => ip = next,
            Flow::Accept => {
                boundary.finish(eng, m)?;
                eng.accepted_tape();
                return Ok(true);
            }
            Flow::Backtrack => match backtrack(eng, m, source)? {
                Unwound::Resumed(next) => ip = next,
                Unwound::Accepted => {
                    boundary.finish(eng, m)?;
                    eng.accepted_tape();
                    return Ok(true);
                }
                Unwound::NoMatch => return Ok(false),
            },
        }
    }
}
"#;

const DISPATCH_OPEN: &str = r#"
fn dispatch<'t>(eng: &mut rt::Engine<'t>, @METER@: &mut rt::Meter<'_>, @SOURCE@: &str, ip: u16) -> Result<Flow, rt::LimitExceeded> {
    Ok(match ip {
"#;

const DISPATCH_CLOSE: &str = "        _ => unreachable!(\"ip {ip} is not a generated state\"),
    })
}
";

const NAVIGATE_OR_BACKTRACK: &str = r#"
if eng.cursor_mut().navigate(m, @NAV@)?.is_none() {
    break 'state Flow::Backtrack;
}
"#;

const NAVIGATE_OR_BACKTRACK_EXPANDED: &str = r#"
if eng
    .cursor_mut()
    .navigate(m, @NAV@)?
    .is_none()
{
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
    if !eng.cursor_mut().continue_search(m, @POLICY@)? {
        @FAIL@
    }
}
"#;

const RETRY_CHECKPOINT_COMPACT: &str = r#"
if @POLICY@.admits(&eng.node()) {
    eng.push_checkpoint(m, rt::Checkpoint::match_retry(eng.checkpoint_state(), @STATE@))?;
}
"#;

const RETRY_CHECKPOINT_EXPANDED: &str = r#"
if @POLICY@.admits(&eng.node()) {
    eng.push_checkpoint(m, rt::Checkpoint::match_retry(
        eng.checkpoint_state(),
        @STATE@,
    ))?;
}
"#;

const CHECK: &str = r#"
// @COMMENT@
if @FAIL@ {
    @REJECT@
}
"#;

const CALL_FIELD_CHECK: &str = r#"
if !eng.cursor_mut().has_field(m, @FIELD@)? {
    break 'state Flow::Backtrack;
}
"#;

const CALL_FIELD_SCAN: &str = r#"
loop {
    if eng.cursor_mut().has_field(m, @FIELD@)? {
        break;
    }
    if !eng.cursor_mut().continue_search(m, @POLICY@)? {
        break 'state Flow::Backtrack;
    }
}
"#;

const CALL_RETRY_PUSH: &str = r#"
eng.push_checkpoint(m, rt::Checkpoint::call_retry(
    eng.checkpoint_state(),
    @STATE@,
    rt::CallResume {
        target: @TARGET@,
        call_site: @CALL_SITE@,
        field: @FIELD@,
        policy: @POLICY@,
    },
))?;
"#;

const RETURN_ARM: &str = r#"
@STATE@ => {
    if eng.frames_empty() {
        @TOP_LEVEL@
    } else {
        let call_site = eng.exit_frame();
        Flow::Jump(call_return(call_site, rt::PortId::from_raw(@PORT@)))
    }
}
"#;

const RETURN_ARM_NO_CALLS: &str = r#"
@STATE@ => @TOP_LEVEL@,
"#;

const FINISH_FN_OPEN: &str = r#"
/// `@STATE@` post-acceptance: effects, then successor dispatch. Shared by the dispatch
/// path and the match-retry resume, so a retried candidate emits exactly
/// what the original acceptance would have emitted.
#[inline]
fn finish_@STEM@(@ENG@: &mut rt::Engine<'_>, @METER@: &mut rt::Meter<'_>) -> Result<Flow, rt::LimitExceeded> {
"#;

const BACKTRACK_SKELETON: &str = r#"
/// Unwind the checkpoint stack: successor checkpoints resume dispatch, Call and
/// Match checkpoints advance their sibling search and re-enter. Loops, never
/// recurses — a run of exhausted retries unwinds in one call.
fn backtrack<'t>(eng: &mut rt::Engine<'t>, m: &mut rt::Meter<'_>, @SOURCE@: &str) -> Result<Unwound, rt::LimitExceeded> {
    'unwind: loop {
        m.charge(1)?;
        let Some((cp, snapshot)) = eng.pop_checkpoint() else {
            return Ok(Unwound::NoMatch);
        };
        eng.restore_checkpoint_state(m, cp.state, snapshot)?;

        match cp.resume {
            rt::Resume::Successor => return Ok(Unwound::Resumed(cp.ip)),

@CALL_RESUME@

            // Match retry: advance past the accepted-but-failed candidate and
            // re-run that state's sibling search from there.
            rt::Resume::Match => match match_retry(eng, m, @SOURCE@, cp.ip)? {
                Some(Flow::Jump(next)) => return Ok(Unwound::Resumed(next)),
                Some(Flow::Accept) => return Ok(Unwound::Accepted),
                Some(Flow::Backtrack) => unreachable!("finish never backtracks"),
                None => continue 'unwind,
            },
        }
    }
}
"#;

const BACKTRACK_CALL_RESUME: &str = r#"            // Call retry: advance to the next candidate satisfying the field
            // constraint, then re-enter the callee. Exhausted siblings keep
            // unwinding to an earlier checkpoint.
            rt::Resume::Call(resume) => {
                if !eng.cursor_mut().continue_search(m, resume.policy)? {
                    continue 'unwind;
                }
                if let Some(field_id) = resume.field {
                    loop {
                        if eng.cursor_mut().has_field(m, field_id)? {
                            break;
                        }
                        if !eng.cursor_mut().continue_search(m, resume.policy)? {
                            continue 'unwind;
                        }
                    }
                }
                eng.push_checkpoint(m, rt::Checkpoint::call_retry(
                    eng.checkpoint_state(),
                    cp.ip,
                    resume,
                ))?;
                eng.enter_frame(m, resume.call_site)?;
                return Ok(Unwound::Resumed(resume.target));
            }"#;

const BACKTRACK_CALL_RESUME_UNREACHABLE: &str =
    r#"            rt::Resume::Call(_) => unreachable!("matcher has no call checkpoints"),"#;

const MATCH_RETRY_OPEN: &str = r#"
fn match_retry<'t>(@ENG@: &mut rt::Engine<'t>, m: &mut rt::Meter<'_>, @SOURCE@: &str, ip: u16) -> Result<Option<Flow>, rt::LimitExceeded> {
    match ip {
"#;

const MATCH_RETRY_CLOSE: &str = "        _ => unreachable!(\"match-retry checkpoint ip {ip} must address a sibling-search Match\"),
    }
}
";

const MATCH_RETRY_EMPTY: &str = r#"
fn match_retry(_eng: &mut rt::Engine<'_>, _m: &mut rt::Meter<'_>, @SOURCE@: &str, ip: u16) -> Result<Option<Flow>, rt::LimitExceeded> {
    unreachable!("match-retry checkpoint ip {ip} must address a sibling-search Match")
}
"#;

const RETRY_ADVANCE: &str = r#"
if !eng.cursor_mut().continue_search(m, @POLICY@)? {
    return Ok(None);
}
"#;
