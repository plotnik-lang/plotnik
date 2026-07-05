//! Compile-time inspection span table.

use std::collections::{HashMap, HashSet};

use rowan::TextRange;

use crate::bytecode::{MAX_SPANS, SpanKind};
use crate::compiler::diagnostics::SourceId;
use crate::compiler::ids::TypeId;
use crate::compiler::lower::LowerInput;
use crate::compiler::lower::ir::MemberRef;
use crate::compiler::parse::ast::{self, Pattern};
use crate::compiler::parse::cst::SyntaxNode;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct SpanId(pub(crate) u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SpanBindingIR {
    Type(TypeId),
    #[allow(dead_code)]
    Member(MemberRef),
}

#[derive(Clone, Debug)]
pub(crate) struct SpanEntryIR {
    pub(crate) source: SourceId,
    pub(crate) range: TextRange,
    pub(crate) kind: SpanKind,
    pub(crate) binding: Option<SpanBindingIR>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SpanTable {
    pub(crate) entries: Vec<SpanEntryIR>,
    index: HashMap<(SyntaxNode, SpanKind), SpanId>,
}

impl SpanTable {
    pub(crate) fn lookup(&self, node: &SyntaxNode, kind: SpanKind) -> Option<SpanId> {
        self.index.get(&(node.clone(), kind)).copied()
    }

    pub(crate) fn bind(&mut self, id: SpanId, binding: SpanBindingIR) {
        let entry = self
            .entries
            .get_mut(id.0 as usize)
            .expect("span id must address an assigned entry");
        if let Some(existing) = entry.binding {
            assert_eq!(existing, binding, "span binding changed after assignment");
            return;
        }
        entry.binding = Some(binding);
    }
}

pub(crate) fn tier(kind: SpanKind) -> u8 {
    match kind {
        SpanKind::Def => 0,
        SpanKind::Capture => 1,
        SpanKind::Pattern | SpanKind::Ref => 2,
        SpanKind::Branch
        | SpanKind::Quantifier
        | SpanKind::Sequence
        | SpanKind::Union
        | SpanKind::Enum => 3,
        SpanKind::Field | SpanKind::Annotation => 4,
        SpanKind::NegField | SpanKind::Predicate => 5,
    }
}

pub(crate) struct SpanAssignment {
    pub(crate) table: SpanTable,
    pub(crate) dropped_tiers: Vec<u8>,
    pub(crate) first_dropped: Option<(SourceId, TextRange)>,
}

pub(crate) fn assign_spans(input: &LowerInput<'_>) -> SpanAssignment {
    let mut candidates = Vec::new();
    for name in input.symbol_table.names() {
        let (source, body) = input
            .symbol_table
            .definition(name)
            .expect("symbol-table name must have a definition");
        let def_id = input
            .analysis
            .dependency_analysis
            .def_id_for_name(input.analysis.interner, name)
            .expect("definition name must have a DefId");
        let def = body
            .syntax()
            .parent()
            .and_then(ast::Def::cast)
            .expect("definition body must live inside a Def node");
        candidates.push(Candidate {
            node: def.syntax().clone(),
            source,
            range: def.text_range(),
            kind: SpanKind::Def,
            binding: Some(SpanBindingIR::Type(
                input.analysis.type_analysis.expect_def_output(def_id),
            )),
        });
        collect_pattern(input, source, body, &mut candidates);
    }

    let mut counts = [0usize; 6];
    for candidate in &candidates {
        counts[tier(candidate.kind) as usize] += 1;
    }

    let mut admitted = HashSet::new();
    let mut total = 0usize;
    let mut dropped_tiers = Vec::new();
    for tier in 0..=4 {
        let next = total + counts[tier as usize];
        if next <= MAX_SPANS {
            admitted.insert(tier);
            total = next;
        } else if counts[tier as usize] > 0 {
            dropped_tiers.push(tier);
        }
    }

    let first_dropped = dropped_tiers.first().and_then(|first| {
        candidates
            .iter()
            .find(|candidate| tier(candidate.kind) == *first)
            .map(|candidate| (candidate.source, candidate.range))
    });

    let mut table = SpanTable::default();
    for candidate in candidates {
        if !admitted.contains(&tier(candidate.kind)) {
            continue;
        }
        let id =
            SpanId(u16::try_from(table.entries.len()).expect("admitted span count fits in u16"));
        table.index.insert((candidate.node, candidate.kind), id);
        table.entries.push(SpanEntryIR {
            source: candidate.source,
            range: candidate.range,
            kind: candidate.kind,
            binding: candidate.binding,
        });
    }

    SpanAssignment {
        table,
        dropped_tiers,
        first_dropped,
    }
}

#[derive(Clone)]
struct Candidate {
    node: SyntaxNode,
    source: SourceId,
    range: TextRange,
    kind: SpanKind,
    binding: Option<SpanBindingIR>,
}

fn collect_pattern(
    input: &LowerInput<'_>,
    source: SourceId,
    pattern: &Pattern,
    out: &mut Vec<Candidate>,
) {
    match pattern {
        Pattern::NodePattern(node) => {
            push_pattern(source, SpanKind::Pattern, node.syntax(), out);
            for child in node.children() {
                collect_pattern(input, source, &child, out);
            }
        }
        Pattern::TokenPattern(token) => {
            push_pattern(source, SpanKind::Pattern, token.syntax(), out);
        }
        Pattern::DefRef(reference) => {
            let name = reference
                .name()
                .expect("resolved reference must have a name");
            let target = input
                .analysis
                .dependency_analysis
                .def_id_for_name(input.analysis.interner, name.text())
                .expect("reference target must resolve");
            out.push(Candidate {
                node: reference.syntax().clone(),
                source,
                range: reference.text_range(),
                kind: SpanKind::Ref,
                binding: Some(SpanBindingIR::Type(
                    input.analysis.type_analysis.expect_def_output(target),
                )),
            });
        }
        Pattern::SeqPattern(seq) => {
            push_pattern(source, SpanKind::Sequence, seq.syntax(), out);
            for item in seq.items() {
                let Some(child) = item.as_pattern() else {
                    continue;
                };
                collect_pattern(input, source, child, out);
            }
        }
        Pattern::CapturedPattern(capture) => {
            if !capture.is_suppressive() {
                let name = capture.name().expect("capture must have a name token");
                out.push(Candidate {
                    node: capture.syntax().clone(),
                    source,
                    range: name.text_range(),
                    kind: SpanKind::Capture,
                    binding: None,
                });
            }

            if let Some(annotation) = capture.type_annotation() {
                let binding = annotation.name().and_then(|name| {
                    input
                        .analysis
                        .type_analysis
                        .iter_type_names()
                        .find(|(_, sym)| input.analysis.interner.resolve(*sym) == name.text())
                        .map(|(type_id, _)| SpanBindingIR::Type(type_id))
                });
                out.push(Candidate {
                    node: annotation.syntax().clone(),
                    source,
                    range: annotation.text_range(),
                    kind: SpanKind::Annotation,
                    binding,
                });
            }

            if let Some(inner) = capture.inner() {
                collect_pattern(input, source, &inner, out);
            }
        }
        Pattern::QuantifiedPattern(quantifier) => {
            if let Some(operator) = quantifier.operator() {
                out.push(Candidate {
                    node: quantifier.syntax().clone(),
                    source,
                    range: operator.text_range(),
                    kind: SpanKind::Quantifier,
                    binding: None,
                });
            }
            if let Some(inner) = quantifier.inner() {
                collect_pattern(input, source, &inner, out);
            }
        }
        Pattern::FieldPattern(field) => {
            let name = field.name().expect("field pattern must have a name token");
            out.push(Candidate {
                node: field.syntax().clone(),
                source,
                range: name.text_range(),
                kind: SpanKind::Field,
                binding: None,
            });
            if let Some(value) = field.value() {
                collect_pattern(input, source, &value, out);
            }
        }
        Pattern::Union(union) => {
            push_pattern(source, SpanKind::Union, union.syntax(), out);
            for branch in union.branches() {
                push_branch(source, &branch, out);
                if let Some(body) = branch.body() {
                    collect_pattern(input, source, &body, out);
                }
            }
        }
        Pattern::Enum(enumeration) => {
            push_pattern(source, SpanKind::Enum, enumeration.syntax(), out);
            for branch in enumeration.branches() {
                push_branch(source, &branch, out);
                if let Some(body) = branch.body() {
                    collect_pattern(input, source, &body, out);
                }
            }
        }
    }
}

fn push_pattern(source: SourceId, kind: SpanKind, node: &SyntaxNode, out: &mut Vec<Candidate>) {
    out.push(Candidate {
        node: node.clone(),
        source,
        range: node.text_range(),
        kind,
        binding: None,
    });
}

fn push_branch(source: SourceId, branch: &ast::Branch, out: &mut Vec<Candidate>) {
    out.push(Candidate {
        node: branch.syntax().clone(),
        source,
        range: branch.text_range(),
        kind: SpanKind::Branch,
        binding: None,
    });
}
