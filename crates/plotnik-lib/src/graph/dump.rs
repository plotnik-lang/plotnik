//! Dump helpers for graph inspection and testing.
//!
//! Provides formatted output for `BuildGraph` and `TypeInferenceResult`
//! suitable for snapshot testing and debugging.

use super::{BuildEffect, BuildGraph, BuildMatcher, NodeId, RefMarker, TypeInferenceResult};
use crate::ir::{Nav, NavKind, TYPE_NODE, TYPE_STR, TYPE_VOID, TypeId};
use std::collections::HashSet;
use std::fmt::Write;

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

    /// Mark nodes as dead (from optimization pass).
    pub fn with_dead_nodes(mut self, dead: &'a HashSet<NodeId>) -> Self {
        self.dead_nodes = Some(dead);
        self
    }

    /// Show dead nodes (struck through or marked).
    pub fn show_dead(mut self, show: bool) -> Self {
        self.show_dead = show;
        self
    }

    /// Filter dead nodes from successor lists.
    pub fn filter_dead_successors(self) -> Self {
        // This is controlled by dead_nodes being set
        self
    }

    pub fn dump(&self) -> String {
        let mut out = String::new();
        self.format(&mut out).expect("String write never fails");
        out
    }

    fn format(&self, w: &mut String) -> std::fmt::Result {
        // Definitions header
        for (name, entry) in self.graph.definitions() {
            writeln!(w, "{} = N{}", name, entry)?;
        }
        if self.graph.definitions().next().is_some() {
            writeln!(w)?;
        }

        // Nodes
        for (id, node) in self.graph.iter() {
            let is_dead = self.dead_nodes.map(|d| d.contains(&id)).unwrap_or(false);

            if is_dead && !self.show_dead {
                continue;
            }

            // Node header
            if is_dead {
                write!(w, "N{}: ✗ ", id)?;
            } else {
                write!(w, "N{}: ", id)?;
            }

            // Navigation (skip Stay)
            if !node.nav.is_stay() {
                write!(w, "[{}] ", format_nav(&node.nav))?;
            }

            // Matcher
            self.format_matcher(w, &node.matcher)?;

            // Ref marker
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

            // Effects
            for effect in &node.effects {
                write!(w, " [{}]", format_effect(effect))?;
            }

            // Successors (filter dead nodes from list)
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
        // Filter out dead nodes from successor list
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
        BuildEffect::StartArray => "StartArray".to_string(),
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

// ─────────────────────────────────────────────────────────────────────────────
// BuildGraph dump methods
// ─────────────────────────────────────────────────────────────────────────────

impl<'src> BuildGraph<'src> {
    /// Create a printer for this graph.
    pub fn printer(&self) -> GraphPrinter<'_, 'src> {
        GraphPrinter::new(self)
    }

    /// Dump graph in default format.
    pub fn dump(&self) -> String {
        self.printer().dump()
    }

    /// Dump graph showing dead nodes from optimization.
    pub fn dump_with_dead(&self, dead_nodes: &HashSet<NodeId>) -> String {
        self.printer()
            .with_dead_nodes(dead_nodes)
            .show_dead(true)
            .dump()
    }

    /// Dump only live nodes (dead nodes filtered out completely).
    pub fn dump_live(&self, dead_nodes: &HashSet<NodeId>) -> String {
        self.printer().with_dead_nodes(dead_nodes).dump()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// TypeInferenceResult dump
// ─────────────────────────────────────────────────────────────────────────────

impl TypeInferenceResult<'_> {
    /// Dump inferred types for debugging/testing.
    pub fn dump(&self) -> String {
        let mut out = String::new();

        out.push_str("=== Entrypoints ===\n");
        for (name, type_id) in &self.entrypoint_types {
            out.push_str(&format!("{} → {}\n", name, format_type_id(*type_id)));
        }

        if !self.type_defs.is_empty() {
            out.push_str("\n=== Types ===\n");
            for (idx, def) in self.type_defs.iter().enumerate() {
                let type_id = idx as TypeId + 3;
                let name = def.name.unwrap_or("<anon>");
                out.push_str(&format!("T{}: {:?} {}", type_id, def.kind, name));

                if let Some(inner) = def.inner_type {
                    out.push_str(&format!(" → {}", format_type_id(inner)));
                }

                if !def.members.is_empty() {
                    out.push_str(" {\n");
                    for member in &def.members {
                        out.push_str(&format!(
                            "    {}: {}\n",
                            member.name,
                            format_type_id(member.ty)
                        ));
                    }
                    out.push('}');
                }
                out.push('\n');
            }
        }

        if !self.errors.is_empty() {
            out.push_str("\n=== Errors ===\n");
            for err in &self.errors {
                out.push_str(&format!(
                    "field `{}` in `{}`: incompatible types [{}]\n",
                    err.field,
                    err.definition,
                    err.types_found
                        .iter()
                        .map(|t| t.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }

        out
    }

    /// Render diagnostics for display (used in tests and CLI).
    pub fn dump_diagnostics(&self, source: &str) -> String {
        self.diagnostics.render_filtered(source)
    }

    /// Check if inference produced any errors.
    pub fn has_errors(&self) -> bool {
        self.diagnostics.has_errors()
    }
}

fn format_type_id(id: TypeId) -> String {
    if id == TYPE_VOID {
        "Void".to_string()
    } else if id == TYPE_NODE {
        "Node".to_string()
    } else if id == TYPE_STR {
        "String".to_string()
    } else {
        format!("T{}", id)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test-only dump helpers
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod test_helpers {
    use super::*;

    impl<'src> BuildGraph<'src> {
        /// Dump graph for snapshot tests.
        pub fn dump_graph(&self) -> String {
            self.dump()
        }

        /// Dump graph with optimization info.
        pub fn dump_optimized(&self, dead_nodes: &HashSet<NodeId>) -> String {
            self.printer().with_dead_nodes(dead_nodes).dump()
        }
    }

    impl TypeInferenceResult<'_> {
        /// Dump types for snapshot tests.
        pub fn dump_types(&self) -> String {
            self.dump()
        }
    }
}
