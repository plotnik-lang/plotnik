//! Short, complete child-sequence descriptions for diagnostics. Stop rather than
//! display a partial list when a frontier is recursive or too large to read.

use std::collections::HashSet;

use crate::core::grammar::{Grammar, StepProjection, VarId};
use crate::core::{NodeFieldId, NodeKindId};

use super::AnalysisLimit;
use super::budget::Budget;
use crate::core::grammar::NodeRealizer;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Child {
    pub(super) kind: NodeKindId,
    pub(super) field: Option<NodeFieldId>,
}

pub(super) fn children(
    grammar: &Grammar,
    realizers: &[NodeRealizer],
    budget: &mut Budget,
) -> Result<Option<Vec<Vec<Child>>>, AnalysisLimit> {
    if realizers.is_empty() {
        return Ok(None);
    }

    let mut reader = Reader {
        grammar,
        budget,
        active: HashSet::new(),
        remaining: 256,
    };
    let mut result = Vec::new();
    for &realizer in realizers {
        reader.budget.charge()?;
        let sequences = match realizer {
            NodeRealizer::Leaf => vec![Vec::new()],
            NodeRealizer::Var(body) => {
                let Some(sequences) = reader.variable(body, None)? else {
                    return Ok(None);
                };
                sequences
            }
        };
        for sequence in sequences {
            if !append(&mut result, sequence, reader.budget)? {
                return Ok(None);
            }
        }
    }
    Ok((!result.is_empty()).then_some(result))
}

struct Reader<'g, 'a> {
    grammar: &'g Grammar,
    budget: &'a mut Budget,
    active: HashSet<VarId>,
    remaining: usize,
}

impl Reader<'_, '_> {
    fn variable(
        &mut self,
        body: VarId,
        inherited: Option<NodeFieldId>,
    ) -> Result<Option<Vec<Vec<Child>>>, AnalysisLimit> {
        self.budget.charge()?;
        if self.active.len() >= 16 || !self.active.insert(body) {
            return Ok(None);
        }

        let mut result = Vec::new();
        for production in &self
            .grammar
            .structure()
            .variable(body)
            .expect("retained variable exists")
            .productions
        {
            if !self.visit()? {
                return Ok(None);
            }

            let mut prefixes = vec![Vec::new()];
            for step in production {
                if !self.visit()? {
                    return Ok(None);
                }

                let suffixes = match step.projection(self.grammar) {
                    StepProjection::HiddenLeaf => continue,
                    StepProjection::Visible { kind, field, .. } => vec![vec![Child {
                        kind,
                        field: field.or(inherited),
                    }]],
                    StepProjection::Transparent { body, field } => {
                        let Some(sequences) = self.variable(body, field.or(inherited))? else {
                            return Ok(None);
                        };
                        sequences
                    }
                };
                let mut combined = Vec::new();
                for prefix in &prefixes {
                    for suffix in &suffixes {
                        self.budget
                            .spend((prefix.len() + suffix.len() + 1) as u64)?;
                        let mut sequence = prefix.clone();
                        sequence.extend(suffix);
                        if !append(&mut combined, sequence, self.budget)? {
                            return Ok(None);
                        }
                    }
                }
                prefixes = combined;
            }
            for sequence in prefixes {
                if !append(&mut result, sequence, self.budget)? {
                    return Ok(None);
                }
            }
        }
        self.active.remove(&body);
        Ok(Some(result))
    }

    fn visit(&mut self) -> Result<bool, AnalysisLimit> {
        if self.remaining == 0 {
            return Ok(false);
        }

        self.budget.charge()?;
        self.remaining -= 1;
        Ok(true)
    }
}

fn append(
    sequences: &mut Vec<Vec<Child>>,
    sequence: Vec<Child>,
    budget: &mut Budget,
) -> Result<bool, AnalysisLimit> {
    if sequence.len() > 8 {
        return Ok(false);
    }

    budget.spend((sequences.len() * (sequence.len() + 1) + 1) as u64)?;
    if sequences.contains(&sequence) {
        return Ok(true);
    }

    if sequences.len() == 6 {
        return Ok(false);
    }

    sequences.push(sequence);
    Ok(true)
}
