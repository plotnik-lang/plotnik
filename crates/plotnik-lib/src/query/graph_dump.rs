//! Dump helpers for graph inspection and testing.

use std::collections::HashSet;
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

    fn format(&self, w: &mut String) -> std::fmt::Result {
        for (name, entry) in self.graph.definitions() {
            writeln!(w, "{} = N{}", name, entry)?;
        }
        if self.graph.definitions().next().is_some() {
            writeln!(w)?;
        }

        for (id, node) in self.graph.iter() {
            let is_dead = self.dead_nodes.map(|d| d.contains(&id)).unwrap_or(false);

            if is_dead && !self.show_dead {
                continue;
            }

            if is_dead {
                write!(w, "N{}: ✗ ", id)?;
            } else {
                write!(w, "N{}: ", id)?;
            }

            if !node.nav.is_stay() {
                write!(w, "[{}] ", format_nav(&node.nav))?;
            }

            self.format_matcher(w, &node.matcher)?;

            match &node.ref_marker {
                RefMarker::None => {}
                RefMarker::Enter { ref_id } => {
                    let name = node.ref_name.unwrap_or("?");
                    write!(w, " +Enter({}, {})", ref_id, name)?;
                }
                RefMarker::Exit { ref_id } => {
                    write!(w, " +Exit({})", ref_id)?;
                }
            }

            for effect in &node.effects {
                write!(w, " [{}]", format_effect(effect))?;
            }

            self.format_successors(w, &node.successors)?;

            writeln!(w)?;
        }

        Ok(())
    }

    fn format_matcher(&self, w: &mut String, matcher: &BuildMatcher<'src>) -> std::fmt::Result {
        match matcher {
            BuildMatcher::Epsilon => write!(w, "ε"),
            BuildMatcher::Node {
                kind,
                field,
                negated_fields,
            } => {
                write!(w, "({})", kind)?;
                if let Some(f) = field {
                    write!(w, " @{}", f)?;
                }
                for neg in negated_fields {
                    write!(w, " !{}", neg)?;
                }
                Ok(())
            }
            BuildMatcher::Anonymous { literal, field } => {
                write!(w, "\"{}\"", literal)?;
                if let Some(f) = field {
                    write!(w, " @{}", f)?;
                }
                Ok(())
            }
            BuildMatcher::Wildcard { field } => {
                write!(w, "_")?;
                if let Some(f) = field {
                    write!(w, " @{}", f)?;
                }
                Ok(())
            }
        }
    }

    fn format_successors(&self, w: &mut String, successors: &[NodeId]) -> std::fmt::Result {
        let live_succs: Vec<_> = successors
            .iter()
            .filter(|s| self.dead_nodes.map(|d| !d.contains(s)).unwrap_or(true))
            .collect();

        if live_succs.is_empty() {
            write!(w, " → ∅")
        } else {
            write!(w, " → ")?;
            let succs: Vec<_> = live_succs.iter().map(|s| format!("N{}", s)).collect();
            write!(w, "{}", succs.join(", "))
        }
    }
}

fn format_nav(nav: &Nav) -> String {
    match nav.kind {
        NavKind::Stay => "Stay".to_string(),
        NavKind::Next => "Next".to_string(),
        NavKind::NextSkipTrivia => "Next.".to_string(),
        NavKind::NextExact => "Next!".to_string(),
        NavKind::Down => "Down".to_string(),
        NavKind::DownSkipTrivia => "Down.".to_string(),
        NavKind::DownExact => "Down!".to_string(),
        NavKind::Up => format!("Up({})", nav.level),
        NavKind::UpSkipTrivia => format!("Up.({})", nav.level),
        NavKind::UpExact => format!("Up!({})", nav.level),
    }
}

fn format_effect(effect: &BuildEffect) -> String {
    match effect {
        BuildEffect::CaptureNode => "Capture".to_string(),
        BuildEffect::ClearCurrent => "Clear".to_string(),
        BuildEffect::StartArray { .. } => "StartArray".to_string(),
        BuildEffect::PushElement => "Push".to_string(),
        BuildEffect::EndArray => "EndArray".to_string(),
        BuildEffect::StartObject => "StartObj".to_string(),
        BuildEffect::EndObject => "EndObj".to_string(),
        BuildEffect::Field { name, .. } => format!("Field({})", name),
        BuildEffect::StartVariant(v) => format!("Variant({})", v),
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
