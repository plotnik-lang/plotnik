//! Types for representing query results.

use super::effect_stream::CapturedNode;
use crate::ir::{DataFieldId, VariantTagId};
use serde::Serialize;
use std::collections::BTreeMap;

/// A structured value produced by a query.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Value<'tree> {
    /// Represents a missing optional value. Serializes to `null`.
    Null,
    /// An AST node capture.
    Node(CapturedNode<'tree>),
    /// A string, typically from a `:: string` conversion.
    String(String),
    /// A list of values, from a `*` or `+` capture.
    Array(Vec<Value<'tree>>),
    /// A map of field names to values, from a `{...}` capture.
    Object(BTreeMap<DataFieldId, Value<'tree>>),
    /// A tagged union, from a `[...]` capture with labels.
    Variant {
        tag: VariantTagId,
        value: Box<Value<'tree>>,
    },
}
