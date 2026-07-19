//! Human-readable dump of the fork-point NFA (the optimized, pre-pack IR).
//!
//! Mirrors the bytecode dump's visual grammar (`docs/binary-format/08-dump-format.md`)
//! in label space, keeping the resolution the wire format erases: symbolic labels
//! instead of packed code addresses, definition-name section headers from label
//! provenance (`Name (consuming):` for the guarded-recursion body variant,
//! real member names on `RecordSet`/`VariantOpen`,
//! callee names on calls (`(Name+)` marks a consuming-body callee), and inline
//! predicate text — the IR has no string table to index into.

use std::fmt::Write as _;

use crate::bytecode::{EffectKind, LineBuilder, Nav, Symbol, nav_symbol, width_for_count};
use crate::compiler::analyze::AnalysisArtifacts;
use crate::compiler::analyze::result::CaptureLayout;
use crate::compiler::ids::DefId;
use crate::compiler::lower::ir::{
    CallEntry, CallIR, DefOutputOrigin, EffectArg, EffectIR, InstructionIR, Label, LabelOrigin,
    MatchIR, NfaGraph, NodeKindConstraint, PredicateValueIR, SemanticNfa, SourceMode,
};
use crate::core::{Colors, NodeFieldId, NodeKindId};

/// Render the optimized NFA in the bytecode dump format, label space.
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
    dumper.dump_entry_points(&mut out);
    dumper.dump_transitions(&mut out);
    out
}

/// Renders instructions in the dump format. Besides the full [`dump_nfa`]
/// output, codegen borrows this per-instruction: each generated state arm
/// carries its instruction as a comment in this exact format, so generated
/// code lines up with the NFA dump 1:1.
pub(crate) struct NfaDumper<'a> {
    graph: &'a NfaGraph,
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
            artifacts,
            layout,
            colors: Colors::new(false),
            label_width: width_for_count(max_label as usize + 1),
        }
    }

    /// One instruction in dump format (label, nav glyph, content, successors).
    pub(crate) fn render_instruction(&self, instr: &InstructionIR) -> String {
        match instr {
            InstructionIR::Match(m) => self.format_match(m),
            InstructionIR::Call(call) => self.format_call(call),
            InstructionIR::Return(r) => self.format_return(r),
        }
    }

    pub(crate) fn label_width(&self) -> usize {
        self.label_width
    }

    pub(crate) fn def_name_of(&self, label: Label) -> &str {
        match self.origin_of(label) {
            LabelOrigin::Def(id) => self.def_name(id),
            LabelOrigin::DefSpecialization { def_id, .. } => self.def_name(def_id),
        }
    }
}

impl NfaDumper<'_> {
    fn dump_entry_points(&self, out: &mut String) {
        let c = &self.colors;
        writeln!(out, "{}[entry_points]{}", c.blue, c.reset)
            .expect("writing to a String is infallible");

        let mut entries: Vec<(&str, Label)> = self
            .graph
            .entry_points()
            .iter()
            .map(|(&def_id, entry)| (self.def_name(def_id), entry.target))
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

            let line = match instr {
                InstructionIR::Match(m) => self.format_match(m),
                InstructionIR::Call(call) => self.format_call(call),
                InstructionIR::Return(r) => self.format_return(r),
            };
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

    fn prefix(&self, label: Label, symbol: Symbol) -> String {
        format!(
            "  {:0w$} {} ",
            label.0,
            symbol.format(),
            w = self.label_width
        )
    }

    fn format_match(&self, m: &MatchIR) -> String {
        let prefix = self.prefix(m.label, nav_symbol(m.nav));
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

        LineBuilder::new(self.label_width).pad_successors(format!("{prefix}{content}"), &successors)
    }

    fn match_content(&self, m: &MatchIR) -> String {
        let mut parts = Vec::new();

        if !m.is_epsilon() {
            for field_id in &m.neg_fields {
                parts.push(format!("-{}", self.field_name(*field_id)));
            }

            let node_part = self.node_pattern(m);
            if !node_part.is_empty() {
                parts.push(node_part);
            }
        }

        let effects: Vec<_> = m.effects.iter().map(|e| self.effect(e)).collect();
        if !effects.is_empty() {
            parts.push(format!("[{}]", effects.join(" ")));
        }

        if !m.is_epsilon()
            && let Some(predicate) = &m.predicate
        {
            let value = match &predicate.value {
                PredicateValueIR::String(s) => format!("{s:?}"),
                PredicateValueIR::Regex(s) => format!("/{s}/"),
            };
            parts.push(format!("{} {}", predicate.op.as_str(), value));
        }

        parts.join(" ")
    }

    pub(crate) fn node_pattern(&self, m: &MatchIR) -> String {
        let mut result = String::new();

        if let Some(field_id) = m.node_field {
            result.push_str(&self.field_name(field_id));
            result.push_str(": ");
        }

        if m.missing {
            result.push_str(&self.missing_pattern(m.node_kind));
            return result;
        }

        match m.node_kind {
            NodeKindConstraint::Any => result.push('_'),
            NodeKindConstraint::Named(None) => result.push_str("(_)"),
            NodeKindConstraint::Named(Some(id)) => {
                result.push('(');
                result.push_str(&self.kind_name(id));
                result.push(')');
            }
            NodeKindConstraint::Anonymous(None) => result.push_str("\"_\""),
            NodeKindConstraint::Anonymous(Some(id)) => {
                result.push('"');
                result.push_str(&self.kind_name(id));
                result.push('"');
            }
        }

        result
    }

    pub(crate) fn effect(&self, e: &EffectIR) -> String {
        match e.kind() {
            EffectKind::Node => "Node".to_string(),
            EffectKind::ListOpen => "ListOpen".to_string(),
            EffectKind::ArrayPush => "ArrayPush".to_string(),
            EffectKind::ListClose => "ListClose".to_string(),
            EffectKind::RecordOpen => "RecordOpen".to_string(),
            EffectKind::RecordClose => "RecordClose".to_string(),
            EffectKind::VariantClose => "VariantClose".to_string(),
            EffectKind::Absent => "Absent".to_string(),
            EffectKind::SuppressBegin => "SuppressBegin".to_string(),
            EffectKind::SuppressEnd => "SuppressEnd".to_string(),
            EffectKind::RecordSet => format!("RecordSet({})", self.member_name(e.payload())),
            EffectKind::VariantOpen => format!("VariantOpen({})", self.member_name(e.payload())),
            EffectKind::SpanStartAt => format!("SpanStartAt#{}", literal(e.payload())),
            EffectKind::SpanStart => format!("SpanStart#{}", literal(e.payload())),
            EffectKind::SpanEnd => format!("SpanEnd#{}", literal(e.payload())),
            EffectKind::ScalarOpen => "ScalarOpen".to_string(),
            EffectKind::ScalarMark => "ScalarMark".to_string(),
            EffectKind::TextClose => "TextClose".to_string(),
            EffectKind::BoolClose => format!("BoolClose({})", literal(e.payload())),
            EffectKind::NodeText => "NodeText".to_string(),
            EffectKind::NodeBool => "NodeBool".to_string(),
            EffectKind::BoolValue => format!("BoolValue({})", literal(e.payload())),
        }
    }

    fn member_name(&self, payload: &EffectArg) -> String {
        let EffectArg::Member(member) = payload else {
            unreachable!("RecordSet/VariantOpen effects are built with member refs");
        };

        let descriptor = self.layout.expect_member(*member);
        self.artifacts.interner.resolve(descriptor.name).to_string()
    }

    /// Render a `(MISSING …)` constraint. `Any` is bare `(MISSING)`; a named or
    /// anonymous kind names the specific missing token.
    fn missing_pattern(&self, kind: NodeKindConstraint) -> String {
        match kind {
            NodeKindConstraint::Any => "(MISSING)".to_string(),
            NodeKindConstraint::Named(Some(id)) => format!("(MISSING {})", self.kind_name(id)),
            NodeKindConstraint::Anonymous(Some(id)) => {
                format!("(MISSING \"{}\")", self.kind_name(id))
            }
            NodeKindConstraint::Named(None) | NodeKindConstraint::Anonymous(None) => {
                unreachable!("MISSING resolves to a concrete kind or Any")
            }
        }
    }

    fn kind_name(&self, id: NodeKindId) -> String {
        // The builtin error symbol has no grammar entry; render `(ERROR)` as written.
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
        let prefix = self.prefix(call.label, symbol);

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

        LineBuilder::new(self.label_width).pad_successors(format!("{prefix}{content}"), &successors)
    }

    /// Callee display name, resolved through the target label's origin: calls
    /// enter at a definition body (or its consuming specialization), so the window that
    /// allocated the target label names the callee.
    fn callee_name(&self, target: Label) -> String {
        match self.origin_of(target) {
            LabelOrigin::Def(id) => self.def_name(id).to_string(),
            origin @ LabelOrigin::DefSpecialization { .. } => self.specialization_name(origin),
        }
    }

    fn format_return(&self, return_: &crate::compiler::lower::ir::ReturnIR) -> String {
        let prefix = self.prefix(return_.label, Symbol::EMPTY);
        let port = return_.port.to_byte();
        let successor = if port == 0 {
            "▶".to_string()
        } else {
            format!("▶ p{port}")
        };
        LineBuilder::new(self.label_width).pad_successors(prefix, &successor)
    }
}

fn literal(payload: &EffectArg) -> usize {
    let EffectArg::Literal(value) = payload else {
        unreachable!("span effects carry literal span ids");
    };
    *value
}
