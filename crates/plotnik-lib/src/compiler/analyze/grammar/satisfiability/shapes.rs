//! Short, complete child-sequence descriptions for diagnostics. Stop rather than
//! display a partial list when a frontier is recursive or too large to read.

use std::collections::HashSet;

use crate::core::grammar::{Grammar, StepProjection, VarId};
use crate::core::{NodeFieldId, NodeKindId};

use crate::core::grammar::NodeRealizer;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Child {
    pub(super) kind: NodeKindId,
    pub(super) field: Option<NodeFieldId>,
}

pub(super) fn children(grammar: &Grammar, realizers: &[NodeRealizer]) -> Option<Vec<Vec<Child>>> {
    if realizers.is_empty() {
        return None;
    }

    let mut reader = Reader {
        grammar,
        active: HashSet::new(),
        remaining: 256,
    };
    let mut result = Vec::new();
    for &realizer in realizers {
        let sequences = match realizer {
            NodeRealizer::Leaf => vec![Vec::new()],
            NodeRealizer::Var(body) => reader.variable(body, None)?,
        };
        for sequence in sequences {
            append(&mut result, sequence)?;
        }
    }
    (!result.is_empty()).then_some(result)
}

struct Reader<'a> {
    grammar: &'a Grammar,
    active: HashSet<VarId>,
    remaining: usize,
}

impl Reader<'_> {
    fn variable(&mut self, body: VarId, inherited: Option<NodeFieldId>) -> Option<Vec<Vec<Child>>> {
        if self.active.len() >= 16 || !self.active.insert(body) {
            return None;
        }

        let mut result = Vec::new();
        for production in &self
            .grammar
            .structure()
            .variable(body)
            .expect("retained variable exists")
            .productions
        {
            self.remaining = self.remaining.checked_sub(1)?;
            let mut prefixes = vec![Vec::new()];
            for step in production {
                self.remaining = self.remaining.checked_sub(1)?;
                let suffixes = match step.projection(self.grammar) {
                    StepProjection::HiddenLeaf => continue,
                    StepProjection::Visible { kind, field, .. } => vec![vec![Child {
                        kind,
                        field: field.or(inherited),
                    }]],
                    StepProjection::Transparent { body, field } => {
                        self.variable(body, field.or(inherited))?
                    }
                };
                let mut combined = Vec::new();
                for prefix in &prefixes {
                    for suffix in &suffixes {
                        let mut sequence = prefix.clone();
                        sequence.extend(suffix);
                        append(&mut combined, sequence)?;
                    }
                }
                prefixes = combined;
            }
            for sequence in prefixes {
                append(&mut result, sequence)?;
            }
        }
        self.active.remove(&body);
        Some(result)
    }
}

fn append(sequences: &mut Vec<Vec<Child>>, sequence: Vec<Child>) -> Option<()> {
    if sequence.len() > 8 {
        return None;
    }
    if sequences.contains(&sequence) {
        return Some(());
    }
    if sequences.len() == 6 {
        return None;
    }
    sequences.push(sequence);
    Some(())
}
