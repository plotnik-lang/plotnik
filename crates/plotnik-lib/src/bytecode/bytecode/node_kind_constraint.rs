//! Node kind constraint for Match instructions.
//!
//! Extracted from ir.rs for use by runtime instruction decoding.

use std::num::NonZeroU16;

/// Node kind constraint for Match instructions.
///
/// Distinguishes between named nodes (`(identifier)`), anonymous nodes (`"text"`),
/// and wildcards (`_`, `(_)`). Encoded in bytecode header byte bits 5-4.
///
/// | `node_class(2)` | Value | Meaning      | `node_val=0`        | `node_val>0`      |
/// | --------------- | ----- | ------------ | ------------------- | ----------------- |
/// | `00`            | Any   | `_` pattern  | No check            | (invalid)         |
/// | `01`            | Named | `(_)`/`(t)`  | Check `is_named()`  | Check `is_named()` + `kind_id()` |
/// | `10`            | Anon  | `"text"`     | Check `!is_named()` | Check `!is_named()` + `kind_id()` |
/// | `11`            | -     | Reserved     | Error               | Error             |
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum NodeKindConstraint {
    /// Any node (`_` pattern) - no kind check performed.
    #[default]
    Any,
    /// Named node constraint (`(_)` or `(identifier)`).
    /// - `None` = any named node (check `is_named()`)
    /// - `Some(id)` = specific named kind (check `is_named()` and `kind_id()`)
    Named(Option<NonZeroU16>),
    /// Anonymous node constraint (`"text"` literals).
    /// - `None` = any anonymous node (check `!is_named()`)
    /// - `Some(id)` = specific anonymous kind (check `!is_named()` and `kind_id()`)
    Anonymous(Option<NonZeroU16>),
}

impl NodeKindConstraint {
    /// Encode to bytecode: returns (node_class bits, node_val).
    ///
    /// `node_class` is 2 bits for header byte bits 5-4.
    /// `node_val` is u16 for bytes 2-3.
    pub fn to_bytes(self) -> (u8, u16) {
        match self {
            Self::Any => (0b00, 0),
            Self::Named(opt) => (0b01, opt.map(|n| n.get()).unwrap_or(0)),
            Self::Anonymous(opt) => (0b10, opt.map(|n| n.get()).unwrap_or(0)),
        }
    }

    /// Decode from bytecode: node_class bits (2 bits) and node_val (u16).
    pub fn from_bytes(node_class: u8, node_val: u16) -> Self {
        Self::try_from_bytes(node_class, node_val)
            .unwrap_or_else(|| panic!("invalid node_class: {node_class}"))
    }

    /// Non-panicking decode, for validating an untrusted instruction stream at
    /// load time. `node_class` `0b11` is reserved and has no valid decoding.
    pub fn try_from_bytes(node_class: u8, node_val: u16) -> Option<Self> {
        match node_class {
            0b00 => Some(Self::Any),
            0b01 => Some(Self::Named(NonZeroU16::new(node_val))),
            0b10 => Some(Self::Anonymous(NonZeroU16::new(node_val))),
            _ => None,
        }
    }

    pub fn kind_id(&self) -> Option<NonZeroU16> {
        match self {
            Self::Any => None,
            Self::Named(opt) | Self::Anonymous(opt) => *opt,
        }
    }

    pub fn is_any(&self) -> bool {
        matches!(self, Self::Any)
    }

    pub fn is_named(&self) -> bool {
        matches!(self, Self::Named(_))
    }

    pub fn is_anonymous(&self) -> bool {
        matches!(self, Self::Anonymous(_))
    }
}
