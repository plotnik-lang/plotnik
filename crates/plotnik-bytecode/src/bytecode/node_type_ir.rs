//! Node type constraint for Match instructions.
//!
//! Extracted from ir.rs for use by runtime instruction decoding.

use std::num::NonZeroU16;

/// Node type constraint for Match instructions.
///
/// Distinguishes between named nodes (`(identifier)`), anonymous nodes (`"text"`),
/// and wildcards (`_`, `(_)`). Encoded in bytecode header byte bits 5-4.
///
/// | `node_kind` | Value | Meaning      | `node_type=0`       | `node_type>0`     |
/// | ----------- | ----- | ------------ | ------------------- | ----------------- |
/// | `00`        | Any   | `_` pattern  | No check            | (invalid)         |
/// | `01`        | Named | `(_)`/`(t)`  | Check `is_named()`  | Check `kind_id()` |
/// | `10`        | Anon  | `"text"`     | Check `!is_named()` | Check `kind_id()` |
/// | `11`        | -     | Reserved     | Error               | Error             |
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum NodeTypeIR {
    /// Any node (`_` pattern) - no type check performed.
    #[default]
    Any,
    /// Named node constraint (`(_)` or `(identifier)`).
    /// - `None` = any named node (check `is_named()`)
    /// - `Some(id)` = specific named type (check `kind_id()`)
    Named(Option<NonZeroU16>),
    /// Anonymous node constraint (`"text"` literals).
    /// - `None` = any anonymous node (check `!is_named()`)
    /// - `Some(id)` = specific anonymous type (check `kind_id()`)
    Anonymous(Option<NonZeroU16>),
}

impl NodeTypeIR {
    /// Encode to bytecode: returns (node_kind bits, node_type value).
    ///
    /// `node_kind` is 2 bits for header byte bits 5-4.
    /// `node_type` is u16 for bytes 2-3.
    pub fn to_bytes(self) -> (u8, u16) {
        match self {
            Self::Any => (0b00, 0),
            Self::Named(opt) => (0b01, opt.map(|n| n.get()).unwrap_or(0)),
            Self::Anonymous(opt) => (0b10, opt.map(|n| n.get()).unwrap_or(0)),
        }
    }

    /// Decode from bytecode: node_kind bits (2 bits) and node_type value (u16).
    pub fn from_bytes(node_kind: u8, node_type: u16) -> Self {
        match node_kind {
            0b00 => Self::Any,
            0b01 => Self::Named(NonZeroU16::new(node_type)),
            0b10 => Self::Anonymous(NonZeroU16::new(node_type)),
            _ => panic!("invalid node_kind: {node_kind}"),
        }
    }

    /// Check if this represents a specific type ID (not a wildcard).
    pub fn type_id(&self) -> Option<NonZeroU16> {
        match self {
            Self::Any => None,
            Self::Named(opt) | Self::Anonymous(opt) => *opt,
        }
    }

    /// Check if this is the Any wildcard.
    pub fn is_any(&self) -> bool {
        matches!(self, Self::Any)
    }

    /// Check if this is a Named constraint (wildcard or specific).
    pub fn is_named(&self) -> bool {
        matches!(self, Self::Named(_))
    }

    /// Check if this is an Anonymous constraint (wildcard or specific).
    pub fn is_anonymous(&self) -> bool {
        matches!(self, Self::Anonymous(_))
    }
}
