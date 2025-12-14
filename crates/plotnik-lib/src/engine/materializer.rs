//! Replays an effect stream to materialize a `Value`.

use super::effect_stream::{CapturedNode, EffectStream};
use super::value::Value;
use crate::ir::{DataFieldId, EffectOp, VariantTagId};
use std::collections::BTreeMap;

/// A container being built on the materializer's value stack.
enum Container<'tree> {
    Array(Vec<Value<'tree>>),
    Object(BTreeMap<DataFieldId, Value<'tree>>),
    Variant(VariantTagId),
}

pub struct Materializer<'a, 'tree> {
    /// The current value being processed.
    current: Option<Value<'tree>>,
    /// A stack of containers (arrays, objects, variants) being built.
    stack: Vec<Container<'tree>>,
    /// An iterator over the captured nodes from the effect stream.
    nodes: std::slice::Iter<'a, CapturedNode<'tree>>,
}

impl<'a, 'tree> Materializer<'a, 'tree> {
    /// Creates a new materializer for a given effect stream.
    fn new(stream: &'a EffectStream<'tree>) -> Self {
        Self {
            current: None,
            stack: Vec::new(),
            nodes: stream.nodes().iter(),
        }
    }

    /// Consumes the materializer and returns the final value.
    fn finish(mut self) -> Value<'tree> {
        self.current.take().unwrap_or(Value::Null)
    }

    /// Replays an effect stream to produce a final `Value`.
    pub fn materialize(stream: &'a EffectStream<'tree>) -> Value<'tree> {
        let mut materializer = Materializer::new(stream);

        for op in stream.ops() {
            materializer.apply_op(*op);
        }

        materializer.finish()
    }

    /// Applies a single effect operation to the materializer's state.
    fn apply_op(&mut self, op: EffectOp) {
        match op {
            EffectOp::CaptureNode => {
                let node = *self.nodes.next().expect("mismatched node capture");
                self.current = Some(Value::Node(node));
            }
            EffectOp::StartObject => {
                self.stack.push(Container::Object(BTreeMap::new()));
            }
            EffectOp::EndObject => match self.stack.pop() {
                Some(Container::Object(obj)) => self.current = Some(Value::Object(obj)),
                _ => panic!("invalid EndObject operation"),
            },
            EffectOp::Field(id) => {
                let value = self.current.take().unwrap_or(Value::Null);
                if let Some(Container::Object(map)) = self.stack.last_mut() {
                    map.insert(id, value);
                } else {
                    panic!("invalid Field operation without object on stack");
                }
            }
            EffectOp::StartArray => {
                self.stack.push(Container::Array(Vec::new()));
            }
            EffectOp::EndArray => match self.stack.pop() {
                Some(Container::Array(arr)) => self.current = Some(Value::Array(arr)),
                _ => panic!("invalid EndArray operation"),
            },
            EffectOp::PushElement => {
                let value = self.current.take().unwrap_or(Value::Null);
                if let Some(Container::Array(arr)) = self.stack.last_mut() {
                    arr.push(value);
                } else {
                    panic!("invalid PushElement operation without array on stack");
                }
            }
            EffectOp::ClearCurrent => {
                self.current = None;
            }
            EffectOp::StartVariant(tag) => {
                self.stack.push(Container::Variant(tag));
            }
            EffectOp::EndVariant => {
                let value = self.current.take().unwrap_or(Value::Null);
                match self.stack.pop() {
                    Some(Container::Variant(tag)) => {
                        self.current = Some(Value::Variant {
                            tag,
                            value: Box::new(value),
                        });
                    }
                    _ => panic!("invalid EndVariant operation"),
                }
            }
            EffectOp::ToString => {
                if let Some(Value::Node(node)) = self.current.take() {
                    self.current = Some(Value::String(node.text().to_string()));
                } else {
                    panic!("invalid ToString operation without a node");
                }
            }
        }
    }
}

#[cfg(test)]
mod materializer_tests;
