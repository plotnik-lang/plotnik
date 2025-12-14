//! Types for representing query results.

use super::effect_stream::{CapturedNode, VerboseNode};
use crate::ir::{CompiledQuery, DataFieldId, VariantTagId};
use serde::Serialize;
use serde::ser::{SerializeMap, SerializeSeq, SerializeStruct};
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

/// Wrapper for verbose serialization of a Value.
/// Nodes include full positional information (bytes + line/column).
pub struct VerboseValue<'a, 'tree>(pub &'a Value<'tree>);

impl Serialize for VerboseValue<'_, '_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self.0 {
            Value::Null => serializer.serialize_none(),
            Value::Node(node) => VerboseNode(node).serialize(serializer),
            Value::String(s) => serializer.serialize_str(s),
            Value::Array(arr) => {
                let mut seq = serializer.serialize_seq(Some(arr.len()))?;
                for item in arr {
                    seq.serialize_element(&VerboseValue(item))?;
                }
                seq.end()
            }
            Value::Object(obj) => {
                let mut map = serializer.serialize_map(Some(obj.len()))?;
                for (k, v) in obj {
                    map.serialize_entry(&k, &VerboseValue(v))?;
                }
                map.end()
            }
            Value::Variant { tag, value } => {
                let mut state = serializer.serialize_struct("Variant", 2)?;
                state.serialize_field("$tag", tag)?;
                state.serialize_field("$data", &VerboseValue(value))?;
                state.end()
            }
        }
    }
}

/// Wrapper for serialization that resolves string IDs to actual strings.
/// Object field names and variant tags are resolved via CompiledQuery.
pub struct ResolvedValue<'a, 'tree>(pub &'a Value<'tree>, pub &'a CompiledQuery);

impl Serialize for ResolvedValue<'_, '_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let ResolvedValue(value, query) = self;
        match value {
            Value::Null => serializer.serialize_none(),
            Value::Node(node) => node.serialize(serializer),
            Value::String(s) => serializer.serialize_str(s),
            Value::Array(arr) => {
                let mut seq = serializer.serialize_seq(Some(arr.len()))?;
                for item in arr {
                    seq.serialize_element(&ResolvedValue(item, query))?;
                }
                seq.end()
            }
            Value::Object(obj) => {
                let mut map = serializer.serialize_map(Some(obj.len()))?;
                for (k, v) in obj {
                    let key = query.string(*k);
                    map.serialize_entry(key, &ResolvedValue(v, query))?;
                }
                map.end()
            }
            Value::Variant { tag, value } => {
                let mut state = serializer.serialize_struct("Variant", 2)?;
                let tag_str = query.string(*tag);
                state.serialize_field("$tag", tag_str)?;
                state.serialize_field("$data", &ResolvedValue(value, query))?;
                state.end()
            }
        }
    }
}

/// Wrapper for verbose serialization with resolved string IDs.
/// Combines VerboseValue (full node positions) with ResolvedValue (string resolution).
pub struct VerboseResolvedValue<'a, 'tree>(pub &'a Value<'tree>, pub &'a CompiledQuery);

impl Serialize for VerboseResolvedValue<'_, '_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let VerboseResolvedValue(value, query) = self;
        match value {
            Value::Null => serializer.serialize_none(),
            Value::Node(node) => VerboseNode(node).serialize(serializer),
            Value::String(s) => serializer.serialize_str(s),
            Value::Array(arr) => {
                let mut seq = serializer.serialize_seq(Some(arr.len()))?;
                for item in arr {
                    seq.serialize_element(&VerboseResolvedValue(item, query))?;
                }
                seq.end()
            }
            Value::Object(obj) => {
                let mut map = serializer.serialize_map(Some(obj.len()))?;
                for (k, v) in obj {
                    let key = query.string(*k);
                    map.serialize_entry(key, &VerboseResolvedValue(v, query))?;
                }
                map.end()
            }
            Value::Variant { tag, value } => {
                let mut state = serializer.serialize_struct("Variant", 2)?;
                let tag_str = query.string(*tag);
                state.serialize_field("$tag", tag_str)?;
                state.serialize_field("$data", &VerboseResolvedValue(value, query))?;
                state.end()
            }
        }
    }
}
