use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;

use crate::bytecode::{EffectKind, EntryBoundary, PredicateOp};
use crate::compiler::diagnostics::SourceId;
use crate::compiler::emit::plan::{
    BlockKind, CallPlan, CheckPlan, CodegenPlan, DefinitionPlan, EffectPlan, FlowPlan, KindClass,
    MatchPlan, PredicatePlan, PredicateValuePlan, RegexId, StateId, StatePlan, StatePlanKind,
};
use crate::compiler::emit::sink::Sink;
use crate::compiler::emit::targets::rust::decode::DecoderGen;
use crate::compiler::emit::targets::rust::ident::shouty_ident;
use crate::compiler::emit::targets::rust::literal::{rust_byte_string, rust_string};
use crate::compiler::emit::targets::rust::{Config, TypeModel};
use crate::compiler::ids::DefId;
use crate::compiler::regex::compile_native_dfa;
use crate::core::NodeFieldId;
use plotnik_runtime::{Limit, Nav, SkipPolicy};

use super::entry_names::apply_fn_name;

pub(crate) fn generate(plan: &CodegenPlan<'_>, config: &Config) -> Result<String, String> {
    let rust = RustRepresentation::from_plan(plan.matcher())?;
    Generator::new(plan, config, &rust).render(TypeModel::new(*plan.result()))
}

pub(crate) fn generate_sources(
    plan: &CodegenPlan<'_>,
    config: &Config,
    modules: &BTreeMap<SourceId, String>,
) -> Result<Vec<(SourceId, String)>, String> {
    let rust = RustRepresentation::from_plan(plan.matcher())?;
    let schema = plan.result();
    let types = TypeModel::new(*schema);
    let mut errors = Vec::new();
    for item in types.items() {
        let Err(sites) = schema.types.type_source(item.name) else {
            continue;
        };
        let locations = sites
            .iter()
            .map(|span| {
                let module = modules
                    .get(&span.source)
                    .expect("every participating source has a Rust module");
                format!(
                    "{}.ptk at bytes {:?}",
                    module.trim_start_matches("r#"),
                    span.range
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        errors.push(format!(
            "ambiguous source ownership for Rust type `{}`: {locations}. Define the type in one query file or use distinct type names.",
            schema.interner.resolve(item.name),
        ));
    }
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }

    modules
        .keys()
        .map(|&source| {
            let mut generator = Generator::new(plan, config, &rust);
            generator.source = Some(source);
            generator.modules = Some(modules);
            Ok((
                source,
                generator.render(types.clone().in_module(source, modules))?,
            ))
        })
        .collect()
}

struct RustRepresentation {
    fields: BTreeMap<NodeFieldId, String>,
    regexes: BTreeMap<RegexId, Vec<u8>>,
}

impl RustRepresentation {
    fn from_plan(plan: &crate::compiler::emit::plan::MatcherPlan) -> Result<Self, String> {
        let regexes = plan
            .regexes()
            .iter()
            .map(|regex| {
                let bytes = compile_native_dfa(&regex.normalized).map_err(|error| {
                    format!("cannot compile regex {:?}: {error}", regex.pattern)
                })?;
                Ok((regex.id, bytes))
            })
            .collect::<Result<_, String>>()?;
        let mut fields = BTreeMap::new();
        for field in plan.fields() {
            let mut name = format!("F_{}", shouty_ident(&field.name));
            while fields.values().any(|taken| *taken == name) {
                let _ = write!(name, "_{}", field.id);
            }
            fields.insert(field.id, name);
        }
        Ok(Self { fields, regexes })
    }
}

struct Generator<'a> {
    config: &'a Config,
    plan: &'a CodegenPlan<'a>,
    rust: &'a RustRepresentation,
    source: Option<SourceId>,
    modules: Option<&'a BTreeMap<SourceId, String>>,
}

impl<'a> Generator<'a> {
    fn new(plan: &'a CodegenPlan<'a>, config: &'a Config, rust: &'a RustRepresentation) -> Self {
        Self {
            config,
            plan,
            rust,
            source: None,
            modules: None,
        }
    }

    fn owns(&self, definition: DefId) -> bool {
        self.source.is_none_or(|source| {
            self.plan
                .result()
                .definitions
                .definition(definition)
                .source()
                == source
        })
    }

    fn has_entries(&self) -> bool {
        self.plan
            .matcher()
            .entrypoints()
            .iter()
            .any(|entry| self.owns(entry.definition))
    }

    fn field_const(&self, field: NodeFieldId) -> &str {
        self.rust
            .fields
            .get(&field)
            .expect("rendered field has a Rust identifier")
    }

    fn runner_path(&self, definition: DefId) -> String {
        let runner = format!("run_{}", definition.index());
        if self.owns(definition) {
            return runner;
        }
        let source = self
            .plan
            .result()
            .definitions
            .definition(definition)
            .source();
        let module = self
            .modules
            .expect("split output has module names")
            .get(&source)
            .expect("callee owner has a Rust module");
        format!("super::super::{module}::matcher::{runner}")
    }

    fn render(&self, types: TypeModel<'_>) -> Result<String, String> {
        let mut entry_names = HashMap::new();
        for entry in self
            .plan
            .matcher()
            .entrypoints()
            .iter()
            .filter(|entry| self.owns(entry.definition))
        {
            if let Some(previous) = entry_names.insert(apply_fn_name(&entry.name), &entry.name) {
                return Err(format!(
                    "definitions `{previous}` and `{}` have the same snake_case name. Rename one.",
                    entry.name
                ));
            }
        }
        let decoders = DecoderGen::new(self.plan.result(), &types, self.plan.decode());
        let mut out = String::new();
        self.header(&mut out);
        out.push_str(&super::emit_model(&types, &self.config.rust_types));
        out.push_str(
            &decoders.execution_api(
                self.plan
                    .matcher()
                    .entrypoints()
                    .iter()
                    .filter(|entry| self.owns(entry.definition))
                    .map(|entry| entry.definition),
            ),
        );
        out.push_str(&decoders.decoders());
        if !self.has_entries()
            && !self
                .plan
                .matcher()
                .definitions()
                .iter()
                .any(|definition| self.owns(definition.owner))
        {
            return Ok(out);
        }

        let mut machinery = String::new();
        let _ = writeln!(machinery, "use {} as rt;", self.config.rt_crate_path());
        machinery.push_str("use rt::Flow;\n");
        if self.has_entries() {
            self.grammar(&mut machinery);
            if decoders.uses_decode_depth() {
                let _ = writeln!(
                    machinery,
                    "pub(super) const MAX_DECODE_DEPTH: Option<u64> = {};",
                    depth_expr(self.config.decode_depth, decoders.max_decoder_frame_bytes())
                );
            }
        }
        self.shared_data(&mut machinery);
        self.entry_fns(&mut machinery);
        for definition in self
            .plan
            .matcher()
            .definitions()
            .iter()
            .filter(|definition| self.owns(definition.owner))
        {
            self.definition(&mut machinery, definition);
        }
        out.push_str(if self.source.is_some() {
            "\npub(super) mod matcher {\n"
        } else {
            "\nmod matcher {\n"
        });
        let mut nested = Sink::new();
        nested.indented(|sink| sink.lines(&machinery));
        out.push_str(nested.plain());
        out.push_str("}\n");
        Ok(out)
    }

    fn header(&self, out: &mut String) {
        if let Some(identity) = &self.config.grammar_identity {
            let _ = writeln!(out, "// Grammar: {:?}", identity.name);
            let _ = writeln!(out, "// SHA-256: {}", identity.sha256);
            let _ = writeln!(out, "// Source: {:?}", identity.source);
        }
        let _ = writeln!(out, "use {} as rt;\n", self.config.rt_crate_path());
        let _ = writeln!(
            out,
            "const _: () = assert!(rt::RUNTIME_ABI == {}, \"runtime interface mismatch\");\n",
            plotnik_runtime::RUNTIME_ABI
        );
    }

    fn grammar(&self, out: &mut String) {
        out.push_str("\npub(super) static GRAMMAR: rt::Grammar = rt::Grammar {\n    kinds: &[\n");
        for expected in self.plan.matcher().expected_kinds() {
            let _ = writeln!(
                out,
                "        ({}, {}, {}),",
                expected.id,
                rust_string(&expected.name),
                expected.named
            );
        }
        out.push_str("    ],\n    fields: &[\n");
        let mut fields = self.plan.matcher().fields().iter().collect::<Vec<_>>();
        fields.sort_unstable_by_key(|field| field.id);
        for field in fields {
            let _ = writeln!(out, "        ({}, {}),", field.id, rust_string(&field.name));
        }
        out.push_str("    ],\n    identity: ");
        if let Some(identity) = &self.config.grammar_identity {
            let _ = writeln!(
                out,
                "Some(({}, {}, {})),",
                rust_string(&identity.name),
                rust_string(&identity.source),
                rust_string(&identity.sha256)
            );
        } else {
            out.push_str("None,\n");
        }
        out.push_str("};\n");
    }

    fn shared_data(&self, out: &mut String) {
        let mut fields = BTreeSet::new();
        let mut regexes = BTreeSet::new();
        for state in self
            .plan
            .matcher()
            .states()
            .iter()
            .filter(|state| self.owns(state.owner))
        {
            let checks = match &state.kind {
                StatePlanKind::Switch(cases) => cases
                    .iter()
                    .flat_map(|case| &case.action.checks)
                    .collect::<Vec<_>>(),
                StatePlanKind::Match(plan) => plan.action.checks.iter().collect(),
                StatePlanKind::Call(plan) => {
                    fields.extend(plan.field);
                    Vec::new()
                }
                _ => Vec::new(),
            };
            for check in checks {
                match check {
                    CheckPlan::Field(field) | CheckPlan::NegField(field) => {
                        fields.insert(field.id);
                    }
                    CheckPlan::Predicate(PredicatePlan {
                        value: PredicateValuePlan::Regex { id, .. },
                        ..
                    }) => {
                        regexes.insert(*id);
                    }
                    _ => {}
                }
            }
        }
        for field in fields {
            let _ = writeln!(
                out,
                "const {}: rt::NodeFieldId = rt::NodeFieldId::from_raw({field});",
                self.field_const(field)
            );
        }
        for id in regexes {
            let bytes = &self.rust.regexes[&id];
            let _ = writeln!(
                out,
                "static RE_{}: rt::StaticDfa = rt::StaticDfa::new({});",
                id.index(),
                rust_byte_string(bytes)
            );
        }
    }

    fn entry_fns(&self, out: &mut String) {
        for entry in self
            .plan
            .matcher()
            .entrypoints()
            .iter()
            .filter(|entry| self.owns(entry.definition))
        {
            let boundary = match entry.boundary {
                EntryBoundary::Passthrough => "rt::EntryBoundary::new(false, false)",
                EntryBoundary::Node => "rt::EntryBoundary::new(false, true)",
                EntryBoundary::Record => "rt::EntryBoundary::new(true, false)",
            };
            let _ = writeln!(
                out,
                "\npub(super) fn {}<'a, 't>(eng: &'a mut rt::MatcherEngine<'t>, source: &str) -> Result<Option<&'a rt::MatchJournal<'t>>, rt::LimitExceeded> {{",
                apply_fn_name(&entry.name)
            );
            let _ = writeln!(
                out,
                "    rt::Matcher {{ runner: {} }}.run(eng, source, {}, {boundary})",
                self.runner_path(entry.definition),
                self.plan.matcher().block(entry.entry)
            );
            out.push_str("}\n");
        }
    }

    fn definition(&self, out: &mut String, definition: &DefinitionPlan) {
        let name = self.plan.result().interner.resolve(
            self.plan
                .result()
                .definitions
                .definition(definition.owner)
                .name(),
        );
        let _ = writeln!(out, "\n// {name}\n");
        for state in self
            .plan
            .matcher()
            .states()
            .iter()
            .filter(|state| state.owner == definition.owner)
        {
            if let StatePlanKind::Call(call) = &state.kind {
                let callee = self.plan.matcher().state(call.target).owner;
                let returns = call
                    .returns
                    .iter()
                    .map(|&target| self.plan.matcher().block(target).to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = writeln!(
                    out,
                    "static CALL_{}: rt::Call = rt::Call {{",
                    state.id.raw()
                );
                let _ = writeln!(out, "    runner: {},", self.runner_path(callee));
                let _ = writeln!(
                    out,
                    "    entry: {},",
                    self.plan.matcher().block(call.target)
                );
                let _ = writeln!(
                    out,
                    "    returns: &[{returns}],\n    site: {},\n}};",
                    state.id.raw()
                );
            }
        }
        let loops = definition.blocks.iter().any(|block| {
            let terminal = match &block.kind {
                BlockKind::States(states) => *states.last().expect("local blocks contain states"),
                BlockKind::Retry(state) => *state,
            };
            self.plan
                .matcher()
                .state(terminal)
                .flows()
                .iter()
                .any(|flow| !matches!(flow, FlowPlan::Accept))
        });
        let block_parameter = if loops { "mut block" } else { "block" };
        let _ = writeln!(
            out,
            "pub(crate) fn run_{}(eng: &mut rt::MatcherEngine<'_>, source: &str, {block_parameter}: u32) -> Flow {{",
            definition.owner.index()
        );
        out.push_str("    let _ = (&eng, &source);\n");
        if loops {
            out.push_str("    'run: loop {\n");
            out.push_str("        return match block {\n");
        } else {
            out.push_str("        match block {\n");
        }
        for block in &definition.blocks {
            let _ = writeln!(out, "            {} => {{", block.id);
            match &block.kind {
                BlockKind::States(states) => {
                    for (index, &id) in states.iter().enumerate() {
                        self.state_body(
                            out,
                            self.plan.matcher().state(id),
                            states.get(index + 1).copied(),
                        );
                    }
                }
                BlockKind::Retry(id) => self.retry(out, self.plan.matcher().state(*id)),
            }
            out.push_str("            }\n");
        }
        out.push_str(
            "            _ => unreachable!(\"invalid local runner block {block}\"),\n        }",
        );
        if loops {
            out.push_str(";\n    }\n");
        } else {
            out.push('\n');
        }
        out.push_str("}\n");
    }

    fn state_body(&self, out: &mut String, state: &StatePlan, next: Option<StateId>) {
        for line in state.provenance.lines() {
            let _ = writeln!(out, "                // {}", line.trim_end());
        }
        match &state.kind {
            StatePlanKind::Epsilon { effects, flow } => {
                self.effects_and_flow(out, effects, flow, next, "                ")
            }
            StatePlanKind::Match(plan) => {
                if plan.navigates() {
                    self.navigate(out, plan.nav);
                }
                self.candidate_search(out, plan);
                self.retry_checkpoint(out, state);
                self.effects_and_flow(
                    out,
                    &plan.action.effects,
                    &plan.action.flow,
                    next,
                    "                ",
                );
            }
            StatePlanKind::Call(plan) => {
                if plan.caller_owned() {
                    if !plan.stays_on_current_node() {
                        self.navigate(out, plan.nav);
                    }
                    self.call_field(out, plan);
                    self.retry_checkpoint(out, state);
                }
                let _ = writeln!(out, "                Flow::Call(&CALL_{})", state.id.raw());
            }
            StatePlanKind::Return(port) => {
                let _ = writeln!(
                    out,
                    "                Flow::Return(rt::PortId::from_raw({}))",
                    port.to_byte()
                );
            }
            StatePlanKind::Switch(cases) => {
                out.push_str("                let node = eng.node();\n                match node.kind_id() {\n");
                for case in cases {
                    let _ = writeln!(out, "                    {} => {{", case.kind);
                    for effects in &case.prefix {
                        for effect in effects {
                            self.effect_stmt(out, effect, "                        ");
                        }
                    }
                    for check in &case.action.checks {
                        let fail = self.check_failure(check);
                        let _ = writeln!(
                            out,
                            "                        if {fail} {{ return Flow::Backtrack; }}"
                        );
                    }
                    self.effects_and_flow(
                        out,
                        &case.action.effects,
                        &case.action.flow,
                        None,
                        "                        ",
                    );
                    out.push_str("                    }\n");
                }
                out.push_str("                    _ => Flow::Backtrack,\n                }\n");
            }
        }
    }

    fn navigate(&self, out: &mut String, nav: Nav) {
        let _ = writeln!(
            out,
            "                if eng.cursor_mut().navigate(rt::Nav::{nav:?}).is_none() {{ return Flow::Backtrack; }}"
        );
    }

    fn candidate_search(&self, out: &mut String, plan: &MatchPlan) {
        if !plan.has_candidate_checks() {
            return;
        }
        let mut failures = Vec::new();
        if let Some(kind) = &plan.kind {
            failures.push(format!("node.kind_id() != {}", kind.id));
        }
        failures.extend(
            plan.action
                .checks
                .iter()
                .map(|check| self.check_failure(check)),
        );
        let failure = failures.join(" || ");
        if plan.search == SkipPolicy::Exact {
            if plan.needs_node_binding() {
                out.push_str("                let node = eng.node();\n");
            }
            let _ = writeln!(
                out,
                "                if {failure} {{ return Flow::Backtrack; }}"
            );
            return;
        }
        out.push_str("                loop {\n");
        if plan.needs_node_binding() {
            out.push_str("                    let node = eng.node();\n");
        }
        let _ = writeln!(out, "                    if !({failure}) {{ break; }}");
        let _ = writeln!(
            out,
            "                    if !eng.cursor_mut().continue_search(rt::SkipPolicy::{:?}) {{ return Flow::Backtrack; }}",
            plan.search
        );
        out.push_str("                }\n");
    }

    fn call_field(&self, out: &mut String, plan: &CallPlan) {
        let Some(field) = plan.field else {
            return;
        };
        let field = self.field_const(field);
        if plan.stays_on_current_node() {
            let _ = writeln!(
                out,
                "                if eng.cursor().field_id() != Some({field}) {{ return Flow::Backtrack; }}"
            );
            return;
        }
        let _ = writeln!(
            out,
            "                while eng.cursor().field_id() != Some({field}) {{"
        );
        let _ = writeln!(
            out,
            "                    if !eng.cursor_mut().continue_search(rt::SkipPolicy::{:?}) {{ return Flow::Backtrack; }}",
            plan.search
        );
        out.push_str("                }\n");
    }

    fn retry_checkpoint(&self, out: &mut String, state: &StatePlan) {
        let policy = match &state.kind {
            StatePlanKind::Match(plan) => plan.retry,
            StatePlanKind::Call(plan) => plan.retry,
            _ => unreachable!("only matches and calls have retry checkpoints"),
        };
        let Some(policy) = policy else {
            return;
        };
        let block = self.plan.matcher().retry_block(state.id);
        let push = format!(
            "eng.push_checkpoint(rt::Checkpoint::new(eng.checkpoint_state(), {block}u32));"
        );
        if matches!(state.kind, StatePlanKind::Match(_)) {
            let _ = writeln!(
                out,
                "                if rt::SkipPolicy::{policy:?}.admits(|| eng.node()) {{\n                    {push}\n                }}"
            );
            return;
        }

        let _ = writeln!(out, "                {push}");
    }

    fn retry(&self, out: &mut String, state: &StatePlan) {
        let policy = match &state.kind {
            StatePlanKind::Match(plan) => plan
                .retry
                .expect("retry block belongs to a retryable match"),
            StatePlanKind::Call(plan) => {
                plan.retry.expect("retry block belongs to a retryable call")
            }
            _ => unreachable!("only matches and calls have retry blocks"),
        };
        let _ = writeln!(
            out,
            "                if !eng.cursor_mut().continue_search(rt::SkipPolicy::{policy:?}) {{ return Flow::Backtrack; }}"
        );
        match &state.kind {
            StatePlanKind::Match(plan) => {
                self.candidate_search(out, plan);
                self.retry_checkpoint(out, state);
                self.effects_and_flow(
                    out,
                    &plan.action.effects,
                    &plan.action.flow,
                    None,
                    "                ",
                );
            }
            StatePlanKind::Call(plan) => {
                self.call_field(out, plan);
                self.retry_checkpoint(out, state);
                let _ = writeln!(out, "                Flow::Call(&CALL_{})", state.id.raw());
            }
            _ => unreachable!("only matches and calls have retry blocks"),
        }
    }

    fn effects_and_flow(
        &self,
        out: &mut String,
        effects: &[EffectPlan],
        flow: &FlowPlan,
        next: Option<StateId>,
        indent: &str,
    ) {
        for effect in effects {
            self.effect_stmt(out, effect, indent);
        }
        if let Some(next) = next {
            assert!(
                matches!(flow, FlowPlan::Jump(target) if *target == next),
                "fused block falls through to its only successor"
            );
            return;
        }
        match flow {
            FlowPlan::Accept => {
                let _ = writeln!(out, "{indent}Flow::Accept");
            }
            FlowPlan::Jump(next) => self.jump(out, *next, indent),
            FlowPlan::Fork { next, successors } => {
                let successors = successors
                    .iter()
                    .map(|&target| format!("{}u32", self.plan.matcher().block(target)))
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = writeln!(out, "{indent}eng.push_successors(&[{successors}]);");
                self.jump(out, *next, indent);
            }
        }
    }

    fn jump(&self, out: &mut String, target: StateId, indent: &str) {
        let _ = writeln!(
            out,
            "{indent}block = {};\n{indent}continue 'run;",
            self.plan.matcher().block(target)
        );
    }

    fn check_failure(&self, check: &CheckPlan) -> String {
        match check {
            CheckPlan::Class(KindClass::Named) => "!node.is_named()".to_owned(),
            CheckPlan::Class(KindClass::Anonymous) => "node.is_named()".to_owned(),
            CheckPlan::Missing => "!node.is_missing()".to_owned(),
            CheckPlan::Field(field) => format!(
                "eng.cursor().field_id() != Some({})",
                self.field_const(field.id)
            ),
            CheckPlan::NegField(field) => format!(
                "node.child_by_field_id(u16::from({})).is_some()",
                self.field_const(field.id)
            ),
            CheckPlan::Predicate(predicate) => self.predicate_failure(predicate),
        }
    }

    fn predicate_failure(&self, predicate: &PredicatePlan) -> String {
        let text = "rt::node_text(source, &node)";
        match &predicate.value {
            PredicateValuePlan::String(value) => {
                let value = rust_string(value);
                match predicate.op {
                    PredicateOp::Eq => format!("{text} != {value}"),
                    PredicateOp::Ne => format!("{text} == {value}"),
                    PredicateOp::StartsWith => format!("!{text}.starts_with({value})"),
                    PredicateOp::EndsWith => format!("!{text}.ends_with({value})"),
                    PredicateOp::Contains => format!("!{text}.contains({value})"),
                    _ => unreachable!("string predicate has a string operator"),
                }
            }
            PredicateValuePlan::Regex { id, .. } => {
                let regex = format!("RE_{}", id.index());
                match predicate.op {
                    PredicateOp::RegexMatch => format!("!{regex}.is_match({text})"),
                    PredicateOp::RegexNoMatch => format!("{regex}.is_match({text})"),
                    _ => unreachable!("regex predicate has a regex operator"),
                }
            }
        }
    }

    fn effect_stmt(&self, out: &mut String, effect: &EffectPlan, indent: &str) {
        let event = match effect.kind {
            EffectKind::Node => "Node(c.node())".to_owned(),
            EffectKind::ListOpen => "ListOpen".to_owned(),
            EffectKind::ArrayPush => "ArrayPush".to_owned(),
            EffectKind::ListClose => "ListClose".to_owned(),
            EffectKind::RecordOpen => "RecordOpen".to_owned(),
            EffectKind::RecordClose => "RecordClose".to_owned(),
            EffectKind::VariantClose => "VariantClose".to_owned(),
            EffectKind::Absent => "Absent".to_owned(),
            EffectKind::RecordSet => format!("RecordSet({})", effect.payload),
            EffectKind::VariantOpen => format!("VariantOpen({})", effect.payload),
            kind => {
                let call = match kind {
                    EffectKind::SuppressBegin => "suppress_begin()".to_owned(),
                    EffectKind::SuppressEnd => "suppress_end()".to_owned(),
                    EffectKind::ScalarOpen => "scalar_open()".to_owned(),
                    EffectKind::ScalarMark => "scalar_mark()".to_owned(),
                    EffectKind::TextClose => "scalar_close_text()".to_owned(),
                    EffectKind::BoolClose => format!("scalar_close_bool({})", effect.payload != 0),
                    EffectKind::NodeText => "node_text()".to_owned(),
                    EffectKind::NodeBool => "node_bool()".to_owned(),
                    EffectKind::BoolValue => format!("bool_value({})", effect.payload != 0),
                    EffectKind::SpanStartAt | EffectKind::SpanStart | EffectKind::SpanEnd => {
                        unreachable!("inspection spans rejected before generation")
                    }
                    _ => unreachable!("event effects are emitted directly"),
                };
                let _ = writeln!(out, "{indent}eng.{call};");
                return;
            }
        };
        let cursor = if effect.kind == EffectKind::Node {
            "c"
        } else {
            "_"
        };
        let _ = writeln!(
            out,
            "{indent}eng.emit_output_event(|{cursor}| rt::JournalEvent::{event});"
        );
    }
}

pub(super) fn depth_expr(limit: Limit, max_decoder_frame_bytes: u64) -> String {
    match limit {
        Limit::Auto => format!("Some(rt::decode_depth_auto({max_decoder_frame_bytes}))"),
        Limit::Of(n) => format!("Some({n})"),
        Limit::Unbounded => "None".to_owned(),
    }
}
