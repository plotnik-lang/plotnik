use std::collections::BTreeMap;

use crate::core::{Colors, NodeFieldId, NodeKindId};

use super::format::{format_effect, pattern_constraints, return_content, width_for_count};
use super::module::Module;
use super::node_kind_constraint::NodeKindConstraint;
use super::{
    CallOwnership, CodeAddr, EffectKind, Instruction, LineBuilder, Match, Nav, Symbol, nav_symbol,
};
#[cfg(feature = "vm")]
use plotnik_runtime::JournalEvent;

pub(crate) struct ModuleRenderContext {
    node_kind_names: BTreeMap<NodeKindId, String>,
    node_field_names: BTreeMap<NodeFieldId, String>,
    member_names: Vec<String>,
    all_strings: Vec<String>,
    regex_patterns: Vec<String>,
    entrypoint_by_ip: BTreeMap<u16, String>,
    pub(crate) builder: LineBuilder,
    pub(crate) addr_width: usize,
    colors: Colors,
}

impl ModuleRenderContext {
    pub(crate) fn new(module: &Module, colors: Colors) -> Self {
        let header = module.header();
        let strings = module.strings();
        let regexes = module.regexes();
        let types = module.types();
        let node_kinds = module.node_kinds();
        let node_fields = module.node_fields();
        let entrypoints = module.entrypoints();

        let mut node_kind_names = BTreeMap::new();
        for t in node_kinds.iter() {
            node_kind_names.insert(t.symbol(), strings.get(t.name()).to_string());
        }

        let mut node_field_names = BTreeMap::new();
        for f in node_fields.iter() {
            node_field_names.insert(f.symbol(), strings.get(f.name()).to_string());
        }

        let member_names: Vec<String> = types
            .members()
            .map(|member| strings.get(member.name_id).to_string())
            .collect();

        let all_strings: Vec<String> = (0..header.str_table_count as usize)
            .map(|i| strings.at(i).to_string())
            .collect();

        let mut regex_patterns = vec![String::new()];
        for i in 1..header.regex_table_count as usize {
            let string_id = regexes.pattern_string_id(i);
            regex_patterns.push(strings.get(string_id).to_string());
        }

        let mut entrypoint_by_ip = BTreeMap::new();
        for e in entrypoints.iter() {
            let name = match e.identity() {
                super::EntrypointIdentity::Named(name) => strings.get(name),
                super::EntrypointIdentity::Expression => "<expression>",
            };
            entrypoint_by_ip.insert(u16::from(e.target()), name.to_string());
        }

        let addr_width = width_for_count(header.instruction_word_count as usize);
        Self {
            node_kind_names,
            node_field_names,
            member_names,
            all_strings,
            regex_patterns,
            entrypoint_by_ip,
            builder: LineBuilder::new(addr_width),
            addr_width,
            colors,
        }
    }

    pub(crate) fn node_kind_name(&self, id: NodeKindId) -> Option<&str> {
        self.node_kind_names.get(&id).map(|s| s.as_str())
    }

    pub(crate) fn node_field_name(&self, id: NodeFieldId) -> Option<&str> {
        self.node_field_names.get(&id).map(|s| s.as_str())
    }

    pub(crate) fn member_name(&self, idx: u16) -> Option<&str> {
        self.member_names.get(idx as usize).map(|s| s.as_str())
    }

    pub(crate) fn string(&self, idx: usize) -> &str {
        &self.all_strings[idx]
    }

    pub(crate) fn regex_pattern(&self, idx: usize) -> &str {
        &self.regex_patterns[idx]
    }

    pub(crate) fn entrypoint_name(&self, ip: u16) -> Option<&str> {
        self.entrypoint_by_ip.get(&ip).map(|s| s.as_str())
    }

    pub(crate) fn dump_node_field_name(&self, id: NodeFieldId) -> String {
        self.node_field_name(id)
            .map(String::from)
            .unwrap_or_else(|| format!("field#{id}"))
    }

    pub(crate) fn format_addr(&self, addr: CodeAddr) -> String {
        if let Some(name) = self.entrypoint_name(addr.get()) {
            return format!("▶({}{}{})", self.colors.blue, name, self.colors.reset);
        }

        format!("{:0w$}", addr, w = self.addr_width)
    }

    pub(crate) fn def_ref_name(&self, ip: CodeAddr) -> String {
        self.entrypoint_name(ip.get())
            .map(str::to_string)
            .unwrap_or_else(|| format!("@{:0w$}", ip, w = self.addr_width))
    }

    fn successors(&self, instruction: &Instruction<'_>) -> String {
        match instruction {
            Instruction::Match(m) if m.is_terminal() => "◼".to_string(),
            Instruction::Match(m) => m
                .successors()
                .map(|addr| self.format_addr(CodeAddr::from(u16::from(addr))))
                .collect::<Vec<_>>()
                .join(", "),
            Instruction::Call(call) => {
                let returns = call
                    .returns()
                    .map(|addr| format!("{:0w$}", u16::from(addr), w = self.addr_width))
                    .collect::<Vec<_>>()
                    .join(" / ");
                format!("{:0w$} : {returns}", call.target, w = self.addr_width)
            }
            Instruction::Switch { .. } | Instruction::Return(_) => String::new(),
        }
    }

    pub(crate) fn instruction_line(&self, addr: CodeAddr, instruction: &Instruction<'_>) -> String {
        let (symbol, content, effects) = match instruction {
            Instruction::Match(m) => (
                nav_symbol(m.nav),
                self.format_match_content(m),
                m.effects()
                    .map(|effect| self.format_effect(effect.kind, effect.payload))
                    .collect::<Vec<_>>(),
            ),
            Instruction::Call(call) => {
                let symbol = match call.ownership {
                    CallOwnership::Caller => nav_symbol(call.nav),
                    CallOwnership::Callee => Symbol::EMPTY,
                };
                let field = call
                    .node_field
                    .map(|id| format!("{}: ", self.dump_node_field_name(id)))
                    .unwrap_or_default();
                let name = self.def_ref_name(call.target);
                (
                    symbol,
                    format!("{field}({}{}{})", self.colors.blue, name, self.colors.reset),
                    Vec::new(),
                )
            }
            Instruction::Return(return_) => {
                (Symbol::EMPTY, return_content(return_.port), Vec::new())
            }
            Instruction::Switch { .. } => {
                (nav_symbol(Nav::StayExact), "switch".to_string(), Vec::new())
            }
        };
        let successors = self.successors(instruction);
        self.builder
            .instruction(addr.get(), symbol, &content, &successors, &effects)
    }

    fn format_match_content(&self, m: &Match<'_>) -> String {
        if m.is_epsilon() {
            return String::new();
        }

        let mut constraints = m
            .neg_fields()
            .map(|id| format!("-{}", self.dump_node_field_name(id)))
            .collect::<Vec<_>>();

        if let Some(predicate) = m.predicate() {
            let op = predicate.op;
            let value = if op.is_regex_op() {
                let pattern = self.regex_pattern(predicate.value_ref as usize);
                format!("/{}/", pattern)
            } else {
                let s = self.string(predicate.value_ref as usize);
                format!("{:?}", s)
            };
            constraints.push(format!("{} {}", op.as_str(), value));
        }

        let mut result = String::new();
        if let Some(field_id) = m.node_field {
            result.push_str(&self.dump_node_field_name(field_id));
            result.push_str(": ");
        }

        let pattern = if m.missing() {
            self.format_missing(m.node_kind)
        } else {
            self.dump_node_kind(m.node_kind)
        };
        result.push_str(&pattern_constraints(pattern, constraints));
        result
    }

    pub(crate) fn dump_node_kind(&self, kind: NodeKindConstraint) -> String {
        match kind {
            NodeKindConstraint::Any => "_".to_string(),
            NodeKindConstraint::Named(None) => "(_)".to_string(),
            NodeKindConstraint::Named(Some(id)) => {
                let name = self.format_node_kind_name(id);
                format!("({name})")
            }
            NodeKindConstraint::Anonymous(None) => "\"_\"".to_string(),
            NodeKindConstraint::Anonymous(Some(id)) => {
                let name = self.format_node_kind_name(id);
                format!("{name:?}")
            }
        }
    }

    fn format_missing(&self, kind: NodeKindConstraint) -> String {
        match kind {
            NodeKindConstraint::Any => "(MISSING)".to_string(),
            NodeKindConstraint::Named(Some(id)) => {
                format!("(MISSING {})", self.format_node_kind_name(id))
            }
            NodeKindConstraint::Anonymous(Some(id)) => {
                format!("(MISSING {:?})", self.format_node_kind_name(id))
            }
            NodeKindConstraint::Named(None) | NodeKindConstraint::Anonymous(None) => {
                unreachable!("MISSING resolves to a concrete kind or Any")
            }
        }
    }

    fn format_node_kind_name(&self, id: NodeKindId) -> String {
        if id == NodeKindId::ERROR {
            return "ERROR".to_string();
        }
        match self.node_kind_name(id) {
            Some(name) => name.to_string(),
            None => format!("node#{id}"),
        }
    }

    pub(crate) fn format_effect(&self, kind: EffectKind, payload: usize) -> String {
        let operand = match kind {
            EffectKind::RecordSet | EffectKind::VariantOpen => format!(
                "M{payload} {}",
                self.member_name(payload as u16)
                    .expect("effect member index names a type member")
            ),
            EffectKind::SpanStartAt | EffectKind::SpanStart | EffectKind::SpanEnd => {
                format!("P{payload}")
            }
            _ => payload.to_string(),
        };
        format_effect(kind, operand)
    }

    #[cfg(feature = "vm")]
    pub(crate) fn format_journal_event(&self, event: &JournalEvent<'_>) -> String {
        let (kind, payload) = match event {
            JournalEvent::Node(_) => (EffectKind::Node, 0),
            JournalEvent::ListOpen => (EffectKind::ListOpen, 0),
            JournalEvent::ArrayPush => (EffectKind::ArrayPush, 0),
            JournalEvent::ListClose => (EffectKind::ListClose, 0),
            JournalEvent::RecordOpen => (EffectKind::RecordOpen, 0),
            JournalEvent::RecordClose => (EffectKind::RecordClose, 0),
            JournalEvent::RecordSet(id) => (EffectKind::RecordSet, *id as usize),
            JournalEvent::VariantOpen(id) => (EffectKind::VariantOpen, *id as usize),
            JournalEvent::VariantClose => (EffectKind::VariantClose, 0),
            JournalEvent::Absent => (EffectKind::Absent, 0),
            JournalEvent::SpanStart { id, node: Some(_) } => {
                (EffectKind::SpanStartAt, *id as usize)
            }
            JournalEvent::SpanStart { id, node: None } => (EffectKind::SpanStart, *id as usize),
            JournalEvent::SpanEnd(id) => (EffectKind::SpanEnd, *id as usize),
            JournalEvent::ScalarOpen => (EffectKind::ScalarOpen, 0),
            JournalEvent::ScalarMark(_) => (EffectKind::ScalarMark, 0),
            JournalEvent::TextClose => (EffectKind::TextClose, 0),
            JournalEvent::BoolClose(value) => (EffectKind::BoolClose, usize::from(*value)),
            JournalEvent::NodeText(_) => (EffectKind::NodeText, 0),
            JournalEvent::NodeBool(_) => (EffectKind::NodeBool, 0),
            JournalEvent::BoolValue(value) => (EffectKind::BoolValue, usize::from(*value)),
        };
        self.format_effect(kind, payload)
    }
}
