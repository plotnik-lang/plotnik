//! Compile-time inspection span table.

use std::collections::{HashMap, HashSet};

use rowan::TextRange;

use crate::bytecode::{MAX_SPANS, SpanKind};
use crate::compiler::analyze::types::BuiltInCaptureType;
use crate::compiler::analyze::types::type_shape::{TYPE_BOOL, TYPE_TEXT};
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
        SpanKind::Alternative
        | SpanKind::Quantifier
        | SpanKind::Sequence
        | SpanKind::UnlabeledAlternation
        | SpanKind::LabeledAlternation => 3,
        SpanKind::Field | SpanKind::CaptureType => 4,
        SpanKind::NegField | SpanKind::Predicate => 5,
    }
}

pub(crate) struct SpanAssignment {
    pub(crate) table: SpanTable,
    pub(crate) dropped_tiers: Vec<u8>,
    pub(crate) first_dropped: Option<(SourceId, TextRange)>,
}

pub(crate) fn assign_spans(input: &LowerInput<'_>) -> SpanAssignment {
    let reachable_defs = input.analysis.dependency_analysis.reachable_from(
        input
            .analysis
            .type_analysis
            .iter_entry_point_outputs()
            .map(|(def_id, _)| def_id),
    );
    let mut candidates = Vec::new();
    for name in input.symbol_table.names() {
        let def_id = input
            .analysis
            .dependency_analysis
            .def_id_for_name(input.analysis.interner, name)
            .expect("definition name must have a DefId");
        if !reachable_defs.contains(def_id) {
            continue;
        }
        let (source, body) = input
            .symbol_table
            .definition(name)
            .expect("symbol-table name must have a definition");
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
        collect_pattern(
            input,
            source,
            body,
            OutputBindingVisibility::Visible,
            &mut candidates,
        );
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

#[derive(Clone, Copy)]
enum OutputBindingVisibility {
    Visible,
    Suppressed,
}

impl OutputBindingVisibility {
    fn bind(self, binding: SpanBindingIR) -> Option<SpanBindingIR> {
        match self {
            Self::Visible => Some(binding),
            Self::Suppressed => None,
        }
    }

    fn suppress(self) -> Self {
        Self::Suppressed
    }
}

fn collect_pattern(
    input: &LowerInput<'_>,
    source: SourceId,
    pattern: &Pattern,
    visibility: OutputBindingVisibility,
    out: &mut Vec<Candidate>,
) {
    match pattern {
        Pattern::NodePattern(node) => {
            push_pattern(source, SpanKind::Pattern, node.syntax(), out);
            for child in node.children() {
                collect_pattern(input, source, &child, visibility, out);
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
                binding: visibility.bind(SpanBindingIR::Type(
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
                collect_pattern(input, source, child, visibility, out);
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

            if let Some(capture_type) = capture.capture_type() {
                let capture_pattern = Pattern::CapturedPattern(capture.clone());
                let fact = input
                    .analysis
                    .type_analysis
                    .expect_capture_fact(&capture_pattern);
                let binding = fact
                    .built_in_plan()
                    .map(|(capture_type, _)| {
                        let primitive = match capture_type {
                            BuiltInCaptureType::Str => TYPE_TEXT,
                            BuiltInCaptureType::Bool => TYPE_BOOL,
                        };
                        SpanBindingIR::Type(primitive)
                    })
                    .or_else(|| {
                        capture_type.name().and_then(|name| {
                            input
                                .analysis
                                .type_analysis
                                .iter_type_names()
                                .find(|(_, sym)| {
                                    input.analysis.interner.resolve(*sym) == name.text()
                                })
                                .map(|(type_id, _)| SpanBindingIR::Type(type_id))
                        })
                    })
                    .and_then(|binding| visibility.bind(binding));
                out.push(Candidate {
                    node: capture_type.syntax().clone(),
                    source,
                    range: capture_type.text_range(),
                    kind: SpanKind::CaptureType,
                    binding,
                });
            }

            if let Some(inner) = capture.inner() {
                let capture_pattern = Pattern::CapturedPattern(capture.clone());
                let suppresses_output = capture.is_suppressive()
                    || input
                        .analysis
                        .type_analysis
                        .expect_capture_fact(&capture_pattern)
                        .built_in_plan()
                        .is_some_and(|(_, plan)| plan.suppresses_semantic_data());
                let inner_visibility = if suppresses_output {
                    visibility.suppress()
                } else {
                    visibility
                };
                collect_pattern(input, source, &inner, inner_visibility, out);
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
                collect_pattern(input, source, &inner, visibility, out);
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
                collect_pattern(input, source, &value, visibility, out);
            }
        }
        Pattern::Alternation(alternation) => {
            let kind = match alternation.labeling() {
                ast::Labeling::Labeled => SpanKind::LabeledAlternation,
                ast::Labeling::Unlabeled | ast::Labeling::Mixed => SpanKind::UnlabeledAlternation,
            };
            push_pattern(source, kind, alternation.syntax(), out);
            for alternative in alternation.alternatives() {
                push_alternative(source, &alternative, out);
                if let Some(body) = alternative.body() {
                    collect_pattern(input, source, &body, visibility, out);
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

fn push_alternative(source: SourceId, alternative: &ast::Alternative, out: &mut Vec<Candidate>) {
    out.push(Candidate {
        node: alternative.syntax().clone(),
        source,
        range: alternative.text_range(),
        kind: SpanKind::Alternative,
        binding: None,
    });
}
