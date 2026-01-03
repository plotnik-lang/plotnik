//! Materializer transforms effect logs into output values.

use crate::bytecode::{QTypeId, StringsView, TypeKind, TypesView};

use super::effect::RuntimeEffect;
use super::value::{NodeHandle, Value};

/// Materializer transforms effect logs into output values.
pub trait Materializer<'t> {
    type Output;

    fn materialize(&self, effects: &[RuntimeEffect<'t>], result_type: QTypeId) -> Self::Output;
}

/// Materializer that produces Value with resolved strings.
pub struct ValueMaterializer<'ctx> {
    source: &'ctx str,
    types: TypesView<'ctx>,
    strings: StringsView<'ctx>,
}

impl<'ctx> ValueMaterializer<'ctx> {
    pub fn new(source: &'ctx str, types: TypesView<'ctx>, strings: StringsView<'ctx>) -> Self {
        Self {
            source,
            types,
            strings,
        }
    }

    fn resolve_member_name(&self, idx: u16) -> String {
        let member = self.types.get_member(idx as usize);
        self.strings.get(member.name).to_owned()
    }

    /// Create initial builder based on result type.
    fn builder_for_type(&self, type_id: QTypeId) -> Builder {
        let def = match self.types.get(type_id) {
            Some(d) => d,
            None => return Builder::Scalar(None),
        };

        match TypeKind::from_u8(def.kind) {
            Some(TypeKind::Struct) => Builder::Object(vec![]),
            Some(TypeKind::Enum) => Builder::Scalar(None), // Enum gets built when Enum effect comes
            Some(TypeKind::ArrayZeroOrMore | TypeKind::ArrayOneOrMore) => Builder::Array(vec![]),
            _ => Builder::Scalar(None),
        }
    }
}

/// Value builder for stack-based materialization.
enum Builder {
    Scalar(Option<Value>),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
    Tagged { tag: String, fields: Vec<(String, Value)> },
}

impl Builder {
    fn build(self) -> Value {
        match self {
            Builder::Scalar(v) => v.unwrap_or(Value::Null),
            Builder::Array(arr) => Value::Array(arr),
            Builder::Object(fields) => Value::Object(fields),
            Builder::Tagged { tag, fields } => Value::Tagged {
                tag,
                data: Box::new(Value::Object(fields)),
            },
        }
    }
}

impl<'t> Materializer<'t> for ValueMaterializer<'_> {
    type Output = Value;

    fn materialize(&self, effects: &[RuntimeEffect<'t>], result_type: QTypeId) -> Value {
        // Stack of containers being built
        let mut stack: Vec<Builder> = vec![];

        // Initialize with result type container
        let result_builder = self.builder_for_type(result_type);
        stack.push(result_builder);

        // Pending value from Node/Text/Null (consumed by Set/Push)
        let mut pending: Option<Value> = None;

        for effect in effects {
            match effect {
                RuntimeEffect::Node(n) => {
                    pending = Some(Value::Node(NodeHandle::from_node(*n, self.source)));
                }
                RuntimeEffect::Text(n) => {
                    let text = n
                        .utf8_text(self.source.as_bytes())
                        .expect("invalid UTF-8")
                        .to_owned();
                    pending = Some(Value::String(text));
                }
                RuntimeEffect::Null => {
                    pending = Some(Value::Null);
                }
                RuntimeEffect::Arr => {
                    stack.push(Builder::Array(vec![]));
                }
                RuntimeEffect::Push => {
                    // Take pending value (or completed container) and push to parent array
                    let val = pending.take().unwrap_or(Value::Null);
                    if let Some(Builder::Array(arr)) = stack.last_mut() {
                        arr.push(val);
                    }
                }
                RuntimeEffect::EndArr => {
                    if let Some(Builder::Array(arr)) = stack.pop() {
                        pending = Some(Value::Array(arr));
                    }
                }
                RuntimeEffect::Obj => {
                    stack.push(Builder::Object(vec![]));
                }
                RuntimeEffect::Set(idx) => {
                    let field_name = self.resolve_member_name(*idx);
                    let val = pending.take().unwrap_or(Value::Null);
                    // Set works on both Object and Tagged (enum variant data)
                    match stack.last_mut() {
                        Some(Builder::Object(obj)) => obj.push((field_name, val)),
                        Some(Builder::Tagged { fields, .. }) => fields.push((field_name, val)),
                        _ => {}
                    }
                }
                RuntimeEffect::EndObj => {
                    if let Some(Builder::Object(fields)) = stack.pop() {
                        pending = Some(Value::Object(fields));
                    }
                }
                RuntimeEffect::Enum(idx) => {
                    let tag = self.resolve_member_name(*idx);
                    stack.push(Builder::Tagged { tag, fields: vec![] });
                }
                RuntimeEffect::EndEnum => {
                    if let Some(Builder::Tagged { tag, fields }) = stack.pop() {
                        // If inner returned a structured value (via Obj/EndObj), use it as data
                        // Otherwise use fields collected from direct Set effects
                        let data = pending.take().unwrap_or(Value::Object(fields));
                        pending = Some(Value::Tagged {
                            tag,
                            data: Box::new(data),
                        });
                    }
                }
                RuntimeEffect::Clear => {
                    pending = None;
                }
            }
        }

        // Result: pending value takes precedence, otherwise pop the result container
        pending
            .or_else(|| stack.pop().map(Builder::build))
            .unwrap_or(Value::Null)
    }
}
