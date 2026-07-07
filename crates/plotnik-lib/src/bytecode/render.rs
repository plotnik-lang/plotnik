use std::collections::BTreeMap;

use crate::core::{NodeFieldId, NodeKindId};

use super::format::format_effect;
use super::instructions::Match;
use super::module::Module;
use super::node_kind_constraint::NodeKindConstraint;
use super::predicate_op::PredicateOp;

pub(crate) struct ModuleRenderContext {
    node_kind_names: BTreeMap<NodeKindId, String>,
    node_field_names: BTreeMap<NodeFieldId, String>,
    member_names: Vec<String>,
    all_strings: Vec<String>,
    regex_patterns: Vec<String>,
    entrypoint_by_ip: BTreeMap<u16, String>,
}

impl ModuleRenderContext {
    pub(crate) fn new(module: &Module) -> Self {
        let header = module.header();
        let strings = module.strings();
        let regexes = module.regexes();
        let types = module.types();
        let node_kinds = module.node_kinds();
        let node_fields = module.node_fields();
        let entrypoints = module.entrypoints();

        let mut node_kind_names = BTreeMap::new();
        for t in node_kinds.iter() {
            let id = NodeKindId::try_from(t.symbol).expect("node kind id must be non-zero");
            node_kind_names.insert(id, strings.get(t.name).to_string());
        }

        let mut node_field_names = BTreeMap::new();
        for f in node_fields.iter() {
            let id = NodeFieldId::try_from(f.symbol).expect("node field id must be non-zero");
            node_field_names.insert(id, strings.get(f.name).to_string());
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
            entrypoint_by_ip.insert(u16::from(e.target()), strings.get(e.name()).to_string());
        }

        Self {
            node_kind_names,
            node_field_names,
            member_names,
            all_strings,
            regex_patterns,
            entrypoint_by_ip,
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
            .unwrap_or_else(|| MissingSymbolPolicy::Dump.format("field", id))
    }

    pub(crate) fn dump_match_content(&self, m: &Match<'_>) -> String {
        MatchRenderer::new(self, MissingSymbolPolicy::Dump).format_match_content(m)
    }

    pub(crate) fn trace_match_content(&self, m: &Match<'_>) -> String {
        MatchRenderer::new(self, MissingSymbolPolicy::Trace).format_match_content(m)
    }
}

struct MatchRenderer<'a> {
    context: &'a ModuleRenderContext,
    missing_symbols: MissingSymbolPolicy,
}

impl<'a> MatchRenderer<'a> {
    fn new(context: &'a ModuleRenderContext, missing_symbols: MissingSymbolPolicy) -> Self {
        Self {
            context,
            missing_symbols,
        }
    }

    fn format_match_content(&self, m: &Match<'_>) -> String {
        let mut parts = Vec::new();

        if !m.is_epsilon() {
            for field_id in m.neg_fields() {
                let name = self.format_node_field_name(field_id);
                parts.push(format!("-{name}"));
            }

            let node_part = self.format_node_pattern(m);
            if !node_part.is_empty() {
                parts.push(node_part);
            }
        }

        let effects: Vec<_> = m.effects().map(|e| format_effect(&e)).collect();
        if !effects.is_empty() {
            parts.push(format!("[{}]", effects.join(" ")));
        }

        if !m.is_epsilon()
            && let Some(predicate) = m.predicate()
        {
            let op = PredicateOp::from_byte(predicate.op);
            let value = if predicate.is_regex {
                let pattern = self.context.regex_pattern(predicate.value_ref as usize);
                format!("/{}/", pattern)
            } else {
                let s = self.context.string(predicate.value_ref as usize);
                format!("{:?}", s)
            };
            parts.push(format!("{} {}", op.as_str(), value));
        }

        parts.join(" ")
    }

    fn format_node_pattern(&self, m: &Match<'_>) -> String {
        let mut result = String::new();

        if let Some(field_id) = m.node_field {
            result.push_str(&self.format_node_field_name(field_id));
            result.push_str(": ");
        }

        match m.node_kind {
            NodeKindConstraint::Any => {
                result.push('_');
            }
            NodeKindConstraint::Named(None) => {
                result.push_str("(_)");
            }
            NodeKindConstraint::Named(Some(id)) => {
                let name = self.format_node_kind_name(id, "node");
                result.push('(');
                result.push_str(&name);
                result.push(')');
            }
            NodeKindConstraint::Anonymous(None) => {
                result.push_str("\"_\"");
            }
            NodeKindConstraint::Anonymous(Some(id)) => {
                let name = self.format_node_kind_name(id, "anon");
                result.push('"');
                result.push_str(&name);
                result.push('"');
            }
        }

        result
    }

    fn format_node_kind_name(&self, id: NodeKindId, dump_prefix: &str) -> String {
        // The builtin error symbol has no grammar entry; render `(ERROR)` as written.
        if id == NodeKindId::ERROR {
            return "ERROR".to_string();
        }
        match self.context.node_kind_name(id) {
            Some(name) => name.to_string(),
            None => self.missing_symbols.format(dump_prefix, id),
        }
    }

    fn format_node_field_name(&self, id: NodeFieldId) -> String {
        match self.context.node_field_name(id) {
            Some(name) => name.to_string(),
            None => self.missing_symbols.format("field", id),
        }
    }
}

#[derive(Clone, Copy)]
enum MissingSymbolPolicy {
    Dump,
    Trace,
}

impl MissingSymbolPolicy {
    fn format(self, prefix: &str, id: impl std::fmt::Display) -> String {
        match self {
            Self::Dump => format!("{prefix}#{id}"),
            Self::Trace => "?".to_string(),
        }
    }
}
