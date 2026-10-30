use indexmap::{IndexMap, IndexSet};

use super::DefinitionGraph;
use crate::compiler::diagnostics::report::{DiagnosticKind, Diagnostics, Span};
use crate::compiler::diagnostics::source::SourceId;
use crate::compiler::ids::DefId;
use crate::compiler::parse::ast::DefRef;
use crate::core::Interner;

#[derive(Clone, Copy)]
struct SourceEdge {
    kind: SourceEdgeKind,
    span: Span,
}

#[derive(Clone, Copy)]
enum SourceEdgeKind {
    Reference { owner: DefId, target: DefId },
    TypeOwnership,
}

type SourceEdges = IndexMap<SourceId, IndexMap<SourceId, SourceEdge>>;

pub(in crate::compiler) fn validate_source_cycles(
    definitions: &DefinitionGraph,
    interner: &Interner,
    ownership_uses: &[(Span, SourceId)],
    diag: &mut Diagnostics,
) -> bool {
    let mut outgoing = reference_edges(definitions);
    for &(span, target) in ownership_uses {
        if span.source == target {
            continue;
        }
        outgoing.entry(target).or_default();
        outgoing
            .entry(span.source)
            .or_default()
            .entry(target)
            .or_insert(SourceEdge {
                kind: SourceEdgeKind::TypeOwnership,
                span,
            });
    }
    validate_cycles(&outgoing, definitions, interner, diag)
}

fn reference_edges(definitions: &DefinitionGraph) -> SourceEdges {
    let mut outgoing = SourceEdges::new();
    for &owner in definitions.ids_in_declaration_order() {
        let definition = definitions.definition(owner);
        let source = definition.source();
        outgoing.entry(source).or_default();
        for reference in definition
            .body()
            .syntax()
            .descendants()
            .filter_map(DefRef::cast)
        {
            let target = definitions.expect_reference_target(&reference);
            let target_source = definitions.definition(target).source();
            if source == target_source {
                continue;
            }
            let name = reference.name().expect("resolved reference has a name");
            outgoing
                .entry(source)
                .or_default()
                .entry(target_source)
                .or_insert(SourceEdge {
                    kind: SourceEdgeKind::Reference { owner, target },
                    span: Span::new(source, name.text_range()),
                });
        }
    }
    outgoing
}

fn validate_cycles(
    outgoing: &SourceEdges,
    definitions: &DefinitionGraph,
    interner: &Interner,
    diag: &mut Diagnostics,
) -> bool {
    let mut finished = IndexSet::new();
    let mut valid = true;
    for &root in outgoing.keys() {
        if finished.contains(&root) {
            continue;
        }

        // Keep the DFS on the heap: a file chain need not be bounded by the
        // definition recursion limit (each file can use different definitions).
        let mut stack = vec![(root, 0usize)];
        let mut active = IndexMap::from([(root, 0usize)]);
        let mut path = Vec::<SourceEdge>::new();
        while let Some((source, next)) = stack.last_mut() {
            let neighbors = outgoing
                .get(source)
                .expect("every source has an adjacency list");
            let Some((&target_source, &edge)) = neighbors.get_index(*next) else {
                finished.insert(*source);
                active.swap_remove(source);
                stack.pop();
                if !stack.is_empty() {
                    path.pop();
                }
                continue;
            };
            *next += 1;
            if let Some(&start) = active.get(&target_source) {
                let cycle = path[start..]
                    .iter()
                    .copied()
                    .chain([edge])
                    .collect::<Vec<_>>();
                report_cycle(&cycle, definitions, interner, diag);
                valid = false;
                continue;
            }
            if finished.contains(&target_source) {
                continue;
            }
            active.insert(target_source, stack.len());
            stack.push((target_source, 0));
            path.push(edge);
        }
    }
    valid
}

fn report_cycle(
    cycle: &[SourceEdge],
    definitions: &DefinitionGraph,
    interner: &Interner,
    diag: &mut Diagnostics,
) {
    let primary = cycle.first().expect("a source cycle has an edge").span;
    let mut report = diag.report(DiagnosticKind::SourceDependencyCycle, primary);
    for (index, edge) in cycle.iter().enumerate() {
        let suffix = if index + 1 == cycle.len() {
            " (completing file cycle)"
        } else {
            ""
        };
        let message = match edge.kind {
            SourceEdgeKind::Reference { owner, target } => {
                let owner = interner.resolve(definitions.definition(owner).name());
                let target = interner.resolve(definitions.definition(target).name());
                format!("{owner} references {target} in another source file{suffix}")
            }
            SourceEdgeKind::TypeOwnership => {
                format!("result type is owned by another source file{suffix}")
            }
        };
        report = report.related_to(edge.span, message);
    }
    let hint = if cycle
        .iter()
        .any(|edge| matches!(edge.kind, SourceEdgeKind::TypeOwnership))
    {
        "move mutually dependent definitions and result types into the same source file, or remove a cross-file dependency"
    } else {
        "move mutually dependent definitions into the same source file, or remove a cross-file reference"
    };
    report.hint(hint).emit();
}
