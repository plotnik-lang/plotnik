//! Dump helpers for graph inspection and testing.

use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use crate::ir::{Nav, NavKind};

use super::graph::{BuildEffect, BuildGraph, BuildMatcher, NodeId, RefMarker};

/// Printer for `BuildGraph` with configurable output options.
pub struct GraphPrinter<'a, 'src> {
    graph: &'a BuildGraph<'src>,
    dead_nodes: Option<&'a HashSet<NodeId>>,
    show_dead: bool,
}

impl<'a, 'src> GraphPrinter<'a, 'src> {
    pub fn new(graph: &'a BuildGraph<'src>) -> Self {
        Self {
            graph,
            dead_nodes: None,
            show_dead: false,
        }
    }

    pub fn with_dead_nodes(mut self, dead: &'a HashSet<NodeId>) -> Self {
        self.dead_nodes = Some(dead);
        self
    }

    pub fn show_dead(mut self, show: bool) -> Self {
        self.show_dead = show;
        self
    }

    pub fn dump(&self) -> String {
        let mut out = String::new();
        self.format(&mut out).expect("String write never fails");
        out
    }

    fn node_width(&self) -> usize {
        let max_id = self.graph.iter().map(|(id, _)| id).max().unwrap_or(0);
        if max_id == 0 {
            1
        } else {
            ((max_id as f64).log10().floor() as usize) + 1
        }
    }

    fn format_node_id(&self, id: NodeId, width: usize) -> String {
        format!("({:0width$})", id, width = width)
    }

    fn format(&self, w: &mut String) -> std::fmt::Result {
        let width = self.node_width();

        // Build ref_id → name lookup from Enter nodes
        let ref_names: HashMap<u32, &str> = self
            .graph
            .iter()
            .filter_map(|(_, node)| {
                if let RefMarker::Enter { ref_id } = &node.ref_marker {
                    Some((*ref_id, node.ref_name.unwrap_or("?")))
                } else {
                    None
                }
            })
            .collect();

        for (name, entry) in self.graph.definitions() {
            writeln!(w, "{} = {}", name, self.format_node_id(entry, width))?;
        }
        if self.graph.definitions().next().is_some() {
            writeln!(w)?;
        }

        for (id, node) in self.graph.iter() {
            let is_dead = self.dead_nodes.map(|d| d.contains(&id)).unwrap_or(false);

            if is_dead && !self.show_dead {
                continue;
            }

            // Source node
            write!(w, "{}", self.format_node_id(id, width))?;

            // Dead node short-circuit
            if is_dead {
                writeln!(w, " → (⨯)")?;
                continue;
            }

            write!(w, " —")?;

            // Navigation (omit for Stay)
            if !node.nav.is_stay() {
                write!(w, "{}—", format_nav(&node.nav))?;
            }

            // Enter ref marker (before matcher)
            if let RefMarker::Enter { .. } = &node.ref_marker {
                let name = node.ref_name.unwrap_or("?");
                write!(w, "<{}>—", name)?;
            }

            // Matcher
            self.format_matcher(w, &node.matcher)?;

            // Exit ref marker (after matcher)
            if let RefMarker::Exit { ref_id } = &node.ref_marker {
                let name = ref_names.get(ref_id).copied().unwrap_or("?");
                write!(w, "—<{}>", name)?;
            }

            // Effects
            if !node.effects.is_empty() {
                write!(w, "—[")?;
                for (i, effect) in node.effects.iter().enumerate() {
                    if i > 0 {
                        write!(w, ", ")?;
                    }
                    write!(w, "{}", format_effect(effect))?;
                }
                write!(w, "]")?;
            }

            // Successors
            self.format_successors(w, &node.successors, width)?;

            writeln!(w)?;
        }

        Ok(())
    }

    fn format_matcher(&self, w: &mut String, matcher: &BuildMatcher<'src>) -> std::fmt::Result {
        match matcher {
            BuildMatcher::Epsilon => write!(w, "𝜀"),
            BuildMatcher::Node {
                kind,
                field,
                negated_fields,
            } => {
                write!(w, "({})", kind)?;
                if let Some(f) = field {
                    write!(w, "@{}", f)?;
                }
                for neg in negated_fields {
                    write!(w, "!{}", neg)?;
                }
                Ok(())
            }
            BuildMatcher::Anonymous { literal, field } => {
                write!(w, "\"{}\"", literal)?;
                if let Some(f) = field {
                    write!(w, "@{}", f)?;
                }
                Ok(())
            }
            BuildMatcher::Wildcard { field } => {
                write!(w, "(🞵)")?;
                if let Some(f) = field {
                    write!(w, "@{}", f)?;
                }
                Ok(())
            }
        }
    }

    fn format_successors(
        &self,
        w: &mut String,
        successors: &[NodeId],
        width: usize,
    ) -> std::fmt::Result {
        let live_succs: Vec<_> = successors
            .iter()
            .filter(|s| self.dead_nodes.map(|d| !d.contains(s)).unwrap_or(true))
            .collect();

        if live_succs.is_empty() {
            write!(w, "→ (✓)")
        } else {
            write!(w, "→ ")?;
            for (i, s) in live_succs.iter().enumerate() {
                if i > 0 {
                    write!(w, ", ")?;
                }
                write!(w, "{}", self.format_node_id(**s, width))?;
            }
            Ok(())
        }
    }
}

fn format_nav(nav: &Nav) -> String {
    match nav.kind {
        NavKind::Stay => "{˟}".to_string(),
        NavKind::Next => "{→}".to_string(),
        NavKind::NextSkipTrivia => "{→·}".to_string(),
        NavKind::NextExact => "{→!}".to_string(),
        NavKind::Down => "{↘}".to_string(),
        NavKind::DownSkipTrivia => "{↘.}".to_string(),
        NavKind::DownExact => "{↘!}".to_string(),
        NavKind::Up => format!("{{↗{}}}", to_superscript(nav.level)),
        NavKind::UpSkipTrivia => format!("{{↗·{}}}", to_superscript(nav.level)),
        NavKind::UpExact => format!("{{↗!{}}}", to_superscript(nav.level)),
    }
}

fn to_superscript(n: u8) -> String {
    const SUPERSCRIPTS: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
    if n == 0 {
        return "⁰".to_string();
    }
    let mut result = String::new();
    let mut num = n;
    while num > 0 {
        result.insert(0, SUPERSCRIPTS[(num % 10) as usize]);
        num /= 10;
    }
    result
}

fn format_effect(effect: &BuildEffect) -> String {
    match effect {
        BuildEffect::CaptureNode => "CaptureNode".to_string(),
        BuildEffect::ClearCurrent => "ClearCurrent".to_string(),
        BuildEffect::StartArray { .. } => "StartArray".to_string(),
        BuildEffect::PushElement => "PushElement".to_string(),
        BuildEffect::EndArray => "EndArray".to_string(),
        BuildEffect::StartObject { .. } => "StartObject".to_string(),
        BuildEffect::EndObject => "EndObject".to_string(),
        BuildEffect::Field { name, .. } => format!("Field({})", name),
        BuildEffect::StartVariant(v) => format!("StartVariant({})", v),
        BuildEffect::EndVariant => "EndVariant".to_string(),
        BuildEffect::ToString => "ToString".to_string(),
    }
}

impl<'src> BuildGraph<'src> {
    pub fn printer(&self) -> GraphPrinter<'_, 'src> {
        GraphPrinter::new(self)
    }

    pub fn dump(&self) -> String {
        self.printer().dump()
    }

    pub fn dump_with_dead(&self, dead_nodes: &HashSet<NodeId>) -> String {
        self.printer()
            .with_dead_nodes(dead_nodes)
            .show_dead(true)
            .dump()
    }

    pub fn dump_live(&self, dead_nodes: &HashSet<NodeId>) -> String {
        self.printer().with_dead_nodes(dead_nodes).dump()
    }
}
