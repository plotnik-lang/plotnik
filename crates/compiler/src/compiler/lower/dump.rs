use std::collections::HashMap;
use std::fmt::Write as _;

use crate::bytecode::{
    EffectKind, LineBuilder, Nav, Symbol, format_effect, nav_symbol, pattern_constraints,
    return_content, width_for_count,
};
use crate::compiler::analyze::AnalysisArtifacts;
use crate::compiler::analyze::result::CaptureLayout;
use crate::compiler::ids::DefId;
use crate::compiler::lower::ir::{
    CallEntry, CallIR, DefOutputOrigin, EffectArg, EffectIR, InstructionIR, Label, LabelOrigin,
    MatchIR, NfaGraph, NodeKindConstraint, PredicateValueIR, SemanticNfa, SourceMode, SwitchIR,
};
use crate::compiler::lower::switch::SwitchEntry;
use crate::core::{Colors, NodeFieldId, NodeKind, NodeKindId};

pub(crate) fn dump_nfa(
    nfa: &SemanticNfa,
    artifacts: AnalysisArtifacts<'_>,
    layout: &CaptureLayout,
    colors: Colors,
) -> String {
    let graph = nfa.raw();
    let mut dumper = NfaDumper::new(graph, artifacts, layout);
    dumper.colors = colors;

    let mut out = String::new();
    dumper.dump_entrypoints(&mut out);
    dumper.dump_transitions(&mut out);
    out
}

pub(crate) struct NfaDumper<'a> {
    graph: &'a NfaGraph,
    instructions: HashMap<Label, &'a InstructionIR>,
    artifacts: AnalysisArtifacts<'a>,
    layout: &'a CaptureLayout,
    colors: Colors,
    label_width: usize,
}

impl<'a> NfaDumper<'a> {
    pub(crate) fn new(
        graph: &'a NfaGraph,
        artifacts: AnalysisArtifacts<'a>,
        layout: &'a CaptureLayout,
    ) -> Self {
        let max_label = graph
            .instructions()
            .iter()
            .map(|i| i.label().0)
            .max()
            .unwrap_or(0);
        Self {
            graph,
            instructions: graph
                .instructions()
                .iter()
                .map(|instruction| (instruction.label(), instruction))
                .collect(),
            artifacts,
            layout,
            colors: Colors::new(false),
            label_width: width_for_count(max_label as usize + 1),
        }
    }

    pub(crate) fn render_instruction(&self, instr: &InstructionIR) -> String {
        match instr {
            InstructionIR::Switch(s) => self.format_switch(s),
            InstructionIR::Match(m) => self.format_match(m),
            InstructionIR::Call(call) => self.format_call(call),
            InstructionIR::Return(r) => self.format_return(r),
        }
    }
}

impl<'a> NfaDumper<'a> {
    fn dump_entrypoints(&self, out: &mut String) {
        let c = &self.colors;
        writeln!(out, "{}[entrypoints]{}", c.blue, c.reset)
            .expect("writing to a String is infallible");

        let mut entries: Vec<(&str, Label)> = self
            .graph
            .entrypoints()
            .iter()
            .map(|(&key, entry)| {
                (
                    match key {
                        crate::compiler::lower::ir::EntrypointKey::Named(def_id) => {
                            self.def_name(def_id)
                        }
                        crate::compiler::lower::ir::EntrypointKey::Expression => "<expression>",
                    },
                    entry.target,
                )
            })
            .collect();
        entries.sort_by_key(|(name, _)| *name);

        let max_len = entries.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
        for (name, label) in entries {
            writeln!(
                out,
                "{}{name:max_len$}{} = {:0w$}",
                c.blue,
                c.reset,
                label.0,
                w = self.label_width
            )
            .expect("writing to a String is infallible");
        }
        out.push('\n');
    }

    fn dump_transitions(&self, out: &mut String) {
        let c = &self.colors;
        writeln!(out, "{}[transitions]{}", c.blue, c.reset)
            .expect("writing to a String is infallible");

        let mut sorted: Vec<&InstructionIR> = self.graph.instructions().iter().collect();
        sorted.sort_by_key(|i| i.label());

        let mut current: Option<LabelOrigin> = None;
        for instr in sorted {
            let origin = self.origin_of(instr.label());
            if current != Some(origin) {
                if current.is_some() {
                    out.push('\n');
                }
                writeln!(out, "{}{}{}", c.blue, self.origin_header(origin), c.reset)
                    .expect("writing to a String is infallible");
                current = Some(origin);
            }

            let line = self.render_instruction(instr);
            out.push_str(&line);
            out.push('\n');
        }
    }

    fn origin_of(&self, label: Label) -> LabelOrigin {
        self.graph
            .origin(label)
            .expect("every pre-pack label carries an origin")
    }

    fn origin_header(&self, origin: LabelOrigin) -> String {
        match origin {
            LabelOrigin::Expression => "<expression>:".to_owned(),
            LabelOrigin::Def(id) => format!("{}:", self.def_name(id)),
            origin @ LabelOrigin::DefSpecialization { .. } => {
                format!("{}:", self.specialization_name(origin))
            }
        }
    }

    fn def_name(&self, def_id: DefId) -> &str {
        let sym = self.artifacts.definitions.definition(def_id).name();
        self.artifacts.interner.resolve(sym)
    }

    fn specialization_name(&self, origin: LabelOrigin) -> String {
        let LabelOrigin::DefSpecialization {
            def_id,
            output,
            source,
            route,
        } = origin
        else {
            unreachable!("specialization names require specialization provenance")
        };
        let mut modes = Vec::new();
        if let DefOutputOrigin::CaptureType(output) = output {
            modes.push(format!("output#{}", output.0));
        }
        if output == DefOutputOrigin::Suppressed {
            modes.push("suppressed".to_string());
        }
        if source == SourceMode::Mark {
            modes.push("marked".to_string());
        }
        if route.requires_consumption() {
            modes.push("consuming".to_string());
        }
        if route.splits() {
            modes.push("split".to_string());
        }
        let entry_nav = route.body_nav();
        if entry_nav != Nav::StayExact {
            modes.push(format!("routed {entry_nav:?}"));
        }
        format!("{} ({})", self.def_name(def_id), modes.join(", "))
    }

    pub(crate) fn switch_cases(&self, switch: &SwitchIR) -> Vec<(SwitchEntry<'a>, String)> {
        let mut cases = switch
            .entries(&self.instructions)
            .into_iter()
            .map(|entry| {
                let anonymous = self
                    .artifacts
                    .grammar
                    .kind_entries()
                    .any(|(node_kind, id)| {
                        id == entry.kind && matches!(node_kind, NodeKind::Anonymous(_))
                    });
                let constraint = if anonymous {
                    NodeKindConstraint::Anonymous(Some(entry.kind))
                } else {
                    NodeKindConstraint::Named(Some(entry.kind))
                };
                (entry, self.node_kind_pattern(constraint))
            })
            .collect::<Vec<_>>();
        cases.sort_by_key(|(entry, _)| entry.target);
        cases
    }

    fn format_switch(&self, switch: &SwitchIR) -> String {
        let builder = LineBuilder::new(self.label_width);
        let mut result = builder.instruction(
            switch.label.0,
            nav_symbol(Nav::StayExact),
            "switch",
            "",
            &[],
        );

        for (entry, kind) in self.switch_cases(switch) {
            let target = format!("{:0w$}", entry.target.0, w = self.label_width);
            let row = builder.switch_case(&kind, &target);
            result.push('\n');
            result.push_str(&row);
        }

        result
    }

    fn format_match(&self, m: &MatchIR) -> String {
        let content = self.match_content(m);
        let successors = if m.successors.is_empty() {
            "◼".to_string()
        } else {
            m.successors
                .iter()
                .map(|s| format!("{:0w$}", s.0, w = self.label_width))
                .collect::<Vec<_>>()
                .join(", ")
        };

        let effects = m
            .effects
            .iter()
            .map(|effect| self.effect(effect))
            .collect::<Vec<_>>();
        LineBuilder::new(self.label_width).instruction(
            m.label.0,
            nav_symbol(m.nav),
            &content,
            &successors,
            &effects,
        )
    }

    fn match_content(&self, m: &MatchIR) -> String {
        if m.is_epsilon() {
            return String::new();
        }

        self.node_pattern(m)
    }

    pub(crate) fn node_pattern(&self, m: &MatchIR) -> String {
        let mut constraints = m
            .neg_fields
            .iter()
            .map(|id| format!("-{}", self.field_name(*id)))
            .collect::<Vec<_>>();
        if let Some(predicate) = &m.predicate {
            let value = match &predicate.value {
                PredicateValueIR::String(s) => format!("{s:?}"),
                PredicateValueIR::Regex(s) => format!("/{s}/"),
            };
            constraints.push(format!("{} {}", predicate.op.as_str(), value));
        }

        let mut result = String::new();

        if let Some(field_id) = m.node_field {
            result.push_str(&self.field_name(field_id));
            result.push_str(": ");
        }

        let pattern = if m.missing {
            self.missing_pattern(m.node_kind)
        } else {
            self.node_kind_pattern(m.node_kind)
        };
        result.push_str(&pattern_constraints(pattern, constraints));
        result
    }

    fn node_kind_pattern(&self, kind: NodeKindConstraint) -> String {
        match kind {
            NodeKindConstraint::Any => "_".to_string(),
            NodeKindConstraint::Named(None) => "(_)".to_string(),
            NodeKindConstraint::Named(Some(id)) => format!("({})", self.kind_name(id)),
            NodeKindConstraint::Anonymous(None) => "\"_\"".to_string(),
            NodeKindConstraint::Anonymous(Some(id)) => format!("{:?}", self.kind_name(id)),
        }
    }

    pub(crate) fn effect(&self, e: &EffectIR) -> String {
        let operand = match e.kind() {
            EffectKind::RecordSet | EffectKind::VariantOpen => self.member_name(e.payload()),
            EffectKind::SpanStartAt | EffectKind::SpanStart | EffectKind::SpanEnd => {
                format!("P{}", literal(e.payload()))
            }
            _ => literal(e.payload()).to_string(),
        };
        format_effect(e.kind(), operand)
    }

    fn member_name(&self, payload: &EffectArg) -> String {
        let EffectArg::Member(member) = payload else {
            unreachable!("RecordSet/VariantOpen effects are built with member refs");
        };

        let descriptor = self.layout.expect_member(*member);
        self.artifacts.interner.resolve(descriptor.name).to_string()
    }

    fn missing_pattern(&self, kind: NodeKindConstraint) -> String {
        match kind {
            NodeKindConstraint::Any => "(MISSING)".to_string(),
            NodeKindConstraint::Named(Some(id)) => format!("(MISSING {})", self.kind_name(id)),
            NodeKindConstraint::Anonymous(Some(id)) => {
                format!("(MISSING {:?})", self.kind_name(id))
            }
            NodeKindConstraint::Named(None) | NodeKindConstraint::Anonymous(None) => {
                unreachable!("MISSING resolves to a concrete kind or Any")
            }
        }
    }

    fn kind_name(&self, id: NodeKindId) -> String {
        if id == NodeKindId::ERROR {
            return "ERROR".to_string();
        }
        self.artifacts
            .grammar
            .kind_name(id, self.artifacts.interner)
            .expect("grammar-bound query binds every referenced node kind")
    }

    fn field_name(&self, id: NodeFieldId) -> String {
        self.artifacts
            .grammar
            .field_name(id, self.artifacts.interner)
            .expect("grammar-bound query binds every referenced field")
    }

    fn format_call(&self, call: &CallIR) -> String {
        let c = &self.colors;
        let symbol = match call.entry {
            CallEntry::CallerOwned { nav, .. } => nav_symbol(nav),
            CallEntry::CalleeOwned { .. } => Symbol::EMPTY,
        };
        let field_part = match call.field() {
            Some(field_id) => format!("{}: ", self.field_name(field_id)),
            None => String::new(),
        };
        let content = format!(
            "{field_part}({}{}{})",
            c.blue,
            self.callee_name(call.target),
            c.reset
        );
        let returns = call
            .return_labels()
            .iter()
            .map(|label| format!("{:0w$}", label.0, w = self.label_width))
            .collect::<Vec<_>>()
            .join(" / ");
        let successors = format!("{:0w$} : {returns}", call.target.0, w = self.label_width);

        LineBuilder::new(self.label_width).instruction(
            call.label.0,
            symbol,
            &content,
            &successors,
            &[],
        )
    }

    fn callee_name(&self, target: Label) -> String {
        match self.origin_of(target) {
            LabelOrigin::Expression => unreachable!("expressions are not call targets"),
            LabelOrigin::Def(id) => self.def_name(id).to_string(),
            origin @ LabelOrigin::DefSpecialization { .. } => self.specialization_name(origin),
        }
    }

    fn format_return(&self, return_: &crate::compiler::lower::ir::ReturnIR) -> String {
        LineBuilder::new(self.label_width).instruction(
            return_.label.0,
            Symbol::EMPTY,
            &return_content(return_.port),
            "",
            &[],
        )
    }
}

fn literal(payload: &EffectArg) -> usize {
    let EffectArg::Literal(value) = payload else {
        unreachable!("span effects carry literal span ids");
    };
    *value
}
