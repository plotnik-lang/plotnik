//! Instruction IR with symbolic labels.
//!
//! Pre-layout instructions use `Label` for symbolic references.
//! After layout, labels are resolved to `StepId` for serialization.
//! Member indices use deferred resolution via `MemberRef`.

use std::collections::BTreeMap;
use std::num::NonZeroU16;

use super::effects::{EffectOp, EffectOpcode};
use super::ids::StepId;
use super::instructions::{Call, Match, Return, select_match_opcode};
use super::nav::Nav;
use crate::query::type_check::TypeId;

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

/// Symbolic reference to a struct field or enum variant.
/// Resolved to absolute member index during bytecode emission.
#[derive(Clone, Copy, Debug)]
pub enum MemberRef {
    /// Already resolved to absolute index (for cases where it's known).
    Absolute(u16),
    /// Deferred resolution: (struct/enum type, relative field/variant index).
    Deferred { type_id: TypeId, relative_index: u16 },
}

impl MemberRef {
    /// Create an absolute reference.
    pub fn absolute(index: u16) -> Self {
        Self::Absolute(index)
    }

    /// Create a deferred reference.
    pub fn deferred(type_id: TypeId, relative_index: u16) -> Self {
        Self::Deferred { type_id, relative_index }
    }

    /// Resolve this reference using a member base lookup function.
    pub fn resolve<F>(self, get_member_base: F) -> u16
    where
        F: Fn(TypeId) -> Option<u16>,
    {
        match self {
            Self::Absolute(n) => n,
            Self::Deferred { type_id, relative_index } => {
                get_member_base(type_id).unwrap_or(0) + relative_index
            }
        }
    }
}

/// Effect operation with symbolic member references.
/// Used during compilation; resolved to EffectOp during emission.
#[derive(Clone, Debug)]
pub struct EffectIR {
    pub opcode: EffectOpcode,
    /// Payload for effects that don't use member indices.
    pub payload: usize,
    /// Member reference for Set/E effects (None for other effects).
    pub member_ref: Option<MemberRef>,
}

impl EffectIR {
    /// Create a simple effect without member reference.
    pub fn simple(opcode: EffectOpcode, payload: usize) -> Self {
        Self { opcode, payload, member_ref: None }
    }

    /// Create an effect with a member reference.
    pub fn with_member(opcode: EffectOpcode, member_ref: MemberRef) -> Self {
        Self { opcode, payload: 0, member_ref: Some(member_ref) }
    }

    /// Resolve this IR effect to a concrete EffectOp.
    pub fn resolve<F>(&self, get_member_base: F) -> EffectOp
    where
        F: Fn(TypeId) -> Option<u16>,
    {
        let payload = if let Some(member_ref) = self.member_ref {
            member_ref.resolve(&get_member_base) as usize
        } else {
            self.payload
        };
        EffectOp { opcode: self.opcode, payload }
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
    pub fn resolve<F>(&self, map: &BTreeMap<Label, StepId>, get_member_base: F) -> Vec<u8>
    where
        F: Fn(TypeId) -> Option<u16>,
    {
        match self {
            Self::Match(m) => m.resolve(map, get_member_base),
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
    pub pre_effects: Vec<EffectIR>,
    /// Fields that must NOT be present on the node.
    pub neg_fields: Vec<u16>,
    /// Effects to execute after successful match.
    pub post_effects: Vec<EffectIR>,
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
    pub fn resolve<F>(&self, map: &BTreeMap<Label, StepId>, get_member_base: F) -> Vec<u8>
    where
        F: Fn(TypeId) -> Option<u16>,
    {
        let successors: Vec<StepId> = self.successors.iter().map(|&l| l.resolve(map)).collect();

        // Resolve effect member references to absolute indices
        let pre_effects: Vec<EffectOp> = self
            .pre_effects
            .iter()
            .map(|e| e.resolve(&get_member_base))
            .collect();
        let post_effects: Vec<EffectOp> = self
            .post_effects
            .iter()
            .map(|e| e.resolve(&get_member_base))
            .collect();

        let m = Match {
            segment: 0,
            nav: self.nav,
            node_type: self.node_type,
            node_field: self.node_field,
            pre_effects,
            neg_fields: self.neg_fields.clone(),
            post_effects,
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
    /// Navigation to apply before jumping to target.
    pub nav: Nav,
    /// Field constraint (None = no constraint).
    pub node_field: Option<NonZeroU16>,
    /// Return address (where to continue after callee returns).
    pub next: Label,
    /// Callee entry point.
    pub target: Label,
}

impl CallIR {
    /// Resolve labels and serialize to bytecode bytes.
    pub fn resolve(&self, map: &BTreeMap<Label, StepId>) -> [u8; 8] {
        let c = Call {
            segment: 0,
            nav: self.nav,
            node_field: self.node_field,
            next: self.next.resolve(map),
            target: self.target.resolve(map),
        };
        c.to_bytes()
    }
}

/// Return instruction IR.
#[derive(Clone, Debug)]
pub struct ReturnIR {
    /// Where this instruction lives.
    pub label: Label,
}

impl ReturnIR {
    /// Serialize to bytecode bytes (no labels to resolve).
    pub fn resolve(&self) -> [u8; 8] {
        let r = Return { segment: 0 };
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
            pre_effects: vec![EffectIR::simple(EffectOpcode::Obj, 0)],
            neg_fields: vec![],
            post_effects: vec![EffectIR::simple(EffectOpcode::Node, 0)],
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
            nav: Nav::Down,
            node_field: None,
            next: Label(4),
            target: Label(5),
        });

        assert_eq!(c.successors(), vec![Label(4)]);

        let r = Instruction::Return(ReturnIR {
            label: Label(6),
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

        let bytes = m.resolve(&map, |_| None);
        assert_eq!(bytes.len(), 8);

        // Verify opcode is Match8 (0x0)
        assert_eq!(bytes[0] & 0xF, 0);
        // Verify next is ACCEPT (0)
        assert_eq!(u16::from_le_bytes([bytes[6], bytes[7]]), 0);
    }

    #[test]
    fn member_ref_resolution() {
        // Test absolute reference
        let abs = MemberRef::absolute(42);
        assert_eq!(abs.resolve(|_| None), 42);

        // Test deferred reference with base lookup
        let deferred = MemberRef::deferred(TypeId(10), 2);
        assert_eq!(deferred.resolve(|id| if id.0 == 10 { Some(100) } else { None }), 102);

        // Test deferred reference with no base (defaults to 0)
        assert_eq!(deferred.resolve(|_| None), 2);
    }

    #[test]
    fn effect_ir_resolution() {
        // Simple effect without member ref
        let simple = EffectIR::simple(EffectOpcode::Node, 5);
        let resolved = simple.resolve(|_| None);
        assert_eq!(resolved.opcode, EffectOpcode::Node);
        assert_eq!(resolved.payload, 5);

        // Effect with deferred member ref
        let set_effect = EffectIR::with_member(
            EffectOpcode::Set,
            MemberRef::deferred(TypeId(10), 1),
        );
        let resolved = set_effect.resolve(|id| if id.0 == 10 { Some(50) } else { None });
        assert_eq!(resolved.opcode, EffectOpcode::Set);
        assert_eq!(resolved.payload, 51); // base 50 + relative 1
    }
}
