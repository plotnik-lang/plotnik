//! Instruction IR with symbolic labels.
//!
//! Pre-layout instructions use `Label` for symbolic references.
//! After layout, labels are resolved to `StepId` for serialization.

use std::collections::BTreeMap;
use std::num::NonZeroU16;

use super::effects::EffectOp;
use super::ids::StepId;
use super::instructions::{Call, Match, Return, select_match_opcode};
use super::nav::Nav;

/// Symbolic reference, resolved to StepId at layout time.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Label(pub u32);

impl Label {
    /// Sentinel for terminal (accept) state.
    pub const ACCEPT: Label = Label(u32::MAX);

    #[inline]
    pub fn is_accept(self) -> bool {
        self.0 == u32::MAX
    }

    /// Resolve this label to a StepId using the layout mapping.
    #[inline]
    pub fn resolve(self, map: &BTreeMap<Label, StepId>) -> StepId {
        if self.is_accept() {
            return StepId::ACCEPT;
        }
        *map.get(&self).unwrap_or(&StepId::ACCEPT)
    }
}

/// Pre-layout instruction with symbolic references.
#[derive(Clone, Debug)]
pub enum Instruction {
    Match(MatchIR),
    Call(CallIR),
    Return(ReturnIR),
}

impl Instruction {
    /// Get the label where this instruction lives.
    #[inline]
    pub fn label(&self) -> Label {
        match self {
            Self::Match(m) => m.label,
            Self::Call(c) => c.label,
            Self::Return(r) => r.label,
        }
    }

    /// Compute instruction size in bytes (8, 16, 24, 32, 48, or 64).
    pub fn size(&self) -> usize {
        match self {
            Self::Match(m) => m.size(),
            Self::Call(_) | Self::Return(_) => 8,
        }
    }

    /// Get all successor labels (for graph building).
    pub fn successors(&self) -> Vec<Label> {
        match self {
            Self::Match(m) => m.successors.clone(),
            Self::Call(c) => vec![c.next],
            Self::Return(_) => vec![],
        }
    }

    /// Resolve labels and serialize to bytecode bytes.
    pub fn resolve(&self, map: &BTreeMap<Label, StepId>) -> Vec<u8> {
        match self {
            Self::Match(m) => m.resolve(map),
            Self::Call(c) => c.resolve(map).to_vec(),
            Self::Return(r) => r.resolve().to_vec(),
        }
    }
}

/// Match instruction IR with symbolic successors.
#[derive(Clone, Debug)]
pub struct MatchIR {
    /// Where this instruction lives.
    pub label: Label,
    /// Navigation command.
    pub nav: Nav,
    /// Node type constraint (None = wildcard).
    pub node_type: Option<NonZeroU16>,
    /// Field constraint (None = wildcard).
    pub node_field: Option<NonZeroU16>,
    /// Effects to execute before match attempt.
    pub pre_effects: Vec<EffectOp>,
    /// Fields that must NOT be present on the node.
    pub neg_fields: Vec<u16>,
    /// Effects to execute after successful match.
    pub post_effects: Vec<EffectOp>,
    /// Successor labels (empty = accept, 1 = linear, 2+ = branch).
    pub successors: Vec<Label>,
}

impl MatchIR {
    /// Compute instruction size in bytes.
    pub fn size(&self) -> usize {
        // Match8 can be used if: no effects, no neg_fields, and at most 1 successor
        let can_use_match8 = self.pre_effects.is_empty()
            && self.neg_fields.is_empty()
            && self.post_effects.is_empty()
            && self.successors.len() <= 1;

        if can_use_match8 {
            return 8;
        }

        // Extended match: count all payload slots
        let slots = self.pre_effects.len()
            + self.neg_fields.len()
            + self.post_effects.len()
            + self.successors.len();

        select_match_opcode(slots)
            .map(|op| op.size())
            .unwrap_or(64)
    }

    /// Resolve labels and serialize to bytecode bytes.
    pub fn resolve(&self, map: &BTreeMap<Label, StepId>) -> Vec<u8> {
        let successors: Vec<StepId> = self.successors.iter().map(|&l| l.resolve(map)).collect();

        let m = Match {
            segment: 0,
            nav: self.nav,
            node_type: self.node_type,
            node_field: self.node_field,
            pre_effects: self.pre_effects.clone(),
            neg_fields: self.neg_fields.clone(),
            post_effects: self.post_effects.clone(),
            successors,
        };

        m.to_bytes().expect("instruction too large")
    }

    /// Check if this is an epsilon transition (no node interaction).
    #[inline]
    pub fn is_epsilon(&self) -> bool {
        self.nav == Nav::Stay && self.node_type.is_none() && self.node_field.is_none()
    }
}

/// Call instruction IR with symbolic target.
#[derive(Clone, Debug)]
pub struct CallIR {
    /// Where this instruction lives.
    pub label: Label,
    /// Return address (where to continue after callee returns).
    pub next: Label,
    /// Callee entry point.
    pub target: Label,
    /// Definition identifier for stack validation.
    pub ref_id: u16,
}

impl CallIR {
    /// Resolve labels and serialize to bytecode bytes.
    pub fn resolve(&self, map: &BTreeMap<Label, StepId>) -> [u8; 8] {
        let c = Call {
            segment: 0,
            next: self.next.resolve(map),
            target: self.target.resolve(map),
            ref_id: self.ref_id,
        };
        c.to_bytes()
    }
}

/// Return instruction IR.
#[derive(Clone, Debug)]
pub struct ReturnIR {
    /// Where this instruction lives.
    pub label: Label,
    /// Definition identifier for stack validation.
    pub ref_id: u16,
}

impl ReturnIR {
    /// Serialize to bytecode bytes (no labels to resolve).
    pub fn resolve(&self) -> [u8; 8] {
        let r = Return {
            segment: 0,
            ref_id: self.ref_id,
        };

        r.to_bytes()
    }
}

/// Result of layout: maps labels to step IDs.
#[derive(Clone, Debug)]
pub struct LayoutResult {
    /// Mapping from symbolic labels to concrete step IDs.
    pub label_to_step: BTreeMap<Label, StepId>,
    /// Total number of steps (for header).
    pub total_steps: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_accept_sentinel() {
        assert!(Label::ACCEPT.is_accept());
        assert!(!Label(0).is_accept());
        assert!(!Label(100).is_accept());
    }

    #[test]
    fn match_ir_size_match8() {
        let m = MatchIR {
            label: Label(0),
            nav: Nav::Down,
            node_type: NonZeroU16::new(10),
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: vec![],
            successors: vec![Label(1)],
        };

        assert_eq!(m.size(), 8);
    }

    #[test]
    fn match_ir_size_extended() {
        let m = MatchIR {
            label: Label(0),
            nav: Nav::Down,
            node_type: NonZeroU16::new(10),
            node_field: None,
            pre_effects: vec![EffectOp {
                opcode: super::super::effects::EffectOpcode::S,
                payload: 0,
            }],
            neg_fields: vec![],
            post_effects: vec![EffectOp {
                opcode: super::super::effects::EffectOpcode::Node,
                payload: 0,
            }],
            successors: vec![Label(1)],
        };

        // 3 slots needed (1 pre + 1 post + 1 succ), fits in Match16 (4 slots)
        assert_eq!(m.size(), 16);
    }

    #[test]
    fn instruction_successors() {
        let m = Instruction::Match(MatchIR {
            label: Label(0),
            nav: Nav::Stay,
            node_type: None,
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: vec![],
            successors: vec![Label(1), Label(2)],
        });

        assert_eq!(m.successors(), vec![Label(1), Label(2)]);

        let c = Instruction::Call(CallIR {
            label: Label(3),
            next: Label(4),
            target: Label(5),
            ref_id: 0,
        });

        assert_eq!(c.successors(), vec![Label(4)]);

        let r = Instruction::Return(ReturnIR {
            label: Label(6),
            ref_id: 0,
        });

        assert!(r.successors().is_empty());
    }

    #[test]
    fn resolve_match_with_accept() {
        let m = MatchIR {
            label: Label(0),
            nav: Nav::Stay,
            node_type: None,
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: vec![],
            successors: vec![Label::ACCEPT],
        };

        let mut map = BTreeMap::new();
        map.insert(Label(0), StepId(1));

        let bytes = m.resolve(&map);
        assert_eq!(bytes.len(), 8);

        // Verify opcode is Match8 (0x0)
        assert_eq!(bytes[0] & 0xF, 0);
        // Verify next is ACCEPT (0)
        assert_eq!(u16::from_le_bytes([bytes[6], bytes[7]]), 0);
    }
}
