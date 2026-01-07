//! Materializer transforms effect logs into output values.

use crate::bytecode::{StringsView, TypeData, TypeId, TypeKind, TypesView};

use super::effect::RuntimeEffect;
use super::value::{NodeHandle, Value};

/// Materializer transforms effect logs into output values.
pub trait Materializer<'t> {
    type Output;

    fn materialize(&self, effects: &[RuntimeEffect<'t>], result_type: TypeId) -> Self::Output;
}

/// Materializer that produces Value with resolved strings.
pub struct ValueMaterializer<'a> {
    source: &'a str,
    types: TypesView<'a>,
    strings: StringsView<'a>,
}

impl<'a> ValueMaterializer<'a> {
    pub fn new(source: &'a str, types: TypesView<'a>, strings: StringsView<'a>) -> Self {
        Self {
            source,
            types,
            strings,
        }
    }

    fn resolve_member_name(&self, idx: u16) -> String {
        let member = self.types.get_member(idx as usize);
        self.strings.get(member.name()).to_owned()
    }

    fn resolve_member_type(&self, idx: u16) -> TypeId {
        self.types.get_member(idx as usize).type_id()
    }

    fn is_void_type(&self, type_id: TypeId) -> bool {
        self.types
            .get(type_id)
            .is_some_and(|def| matches!(def.classify(), TypeData::Primitive(TypeKind::Void)))
    }

    /// Create initial builder based on result type.
    fn builder_for_type(&self, type_id: TypeId) -> Builder {
        let def = self
            .types
            .get(type_id)
            .unwrap_or_else(|| panic!("unknown type_id {}", type_id.0));

        match def.classify() {
            TypeData::Composite {
                kind: TypeKind::Struct,
                ..
            } => Builder::Object(vec![]),
            TypeData::Composite {
                kind: TypeKind::Enum,
                ..
            } => Builder::Scalar(None),
            TypeData::Wrapper {
                kind: TypeKind::ArrayZeroOrMore | TypeKind::ArrayOneOrMore,
                ..
            } => Builder::Array(vec![]),
            _ => Builder::Scalar(None),
        }
    }
}

/// Value builder for stack-based materialization.
enum Builder {
    Scalar(Option<Value>),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
    Tagged {
        tag: String,
        payload_type: TypeId,
        fields: Vec<(String, Value)>,
    },
}

impl Builder {
    fn build(self) -> Value {
        match self {
            Builder::Scalar(v) => v.unwrap_or(Value::Null),
            Builder::Array(arr) => Value::Array(arr),
            Builder::Object(fields) => Value::Object(fields),
            Builder::Tagged { tag, fields, .. } => Value::Tagged {
                tag,
                data: Some(Box::new(Value::Object(fields))),
            },
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Builder::Scalar(_) => "Scalar",
            Builder::Array(_) => "Array",
            Builder::Object(_) => "Object",
            Builder::Tagged { .. } => "Tagged",
        }
    }
}

impl<'t> Materializer<'t> for ValueMaterializer<'_> {
    type Output = Value;

    fn materialize(&self, effects: &[RuntimeEffect<'t>], result_type: TypeId) -> Value {
        // Stack of containers being built
        let mut stack: Vec<Builder> = vec![];

        // Initialize with result type container
        let result_builder = self.builder_for_type(result_type);
        stack.push(result_builder);

        // Pending value from Node/Text/Null (consumed by Set/Push)
        let mut pending: Option<Value> = None;

        for (effect_idx, effect) in effects.iter().enumerate() {
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
                    let val = pending.take().unwrap_or(Value::Null);
                    let Some(Builder::Array(arr)) = stack.last_mut() else {
                        panic!(
                            "effect {effect_idx}: Push expects Array on stack, found {:?}",
                            stack.last().map(|b| b.kind())
                        );
                    };
                    arr.push(val);
                }
                RuntimeEffect::EndArr => {
                    let top = stack.pop();
                    let Some(Builder::Array(arr)) = top else {
                        panic!(
                            "effect {effect_idx}: EndArr expects Array on stack, found {:?}",
                            top.as_ref().map(|b| b.kind())
                        );
                    };
                    pending = Some(Value::Array(arr));
                }
                RuntimeEffect::Obj => {
                    stack.push(Builder::Object(vec![]));
                }
                RuntimeEffect::Set(idx) => {
                    let field_name = self.resolve_member_name(*idx);
                    let val = pending.take().unwrap_or(Value::Null);
                    match stack.last_mut() {
                        Some(Builder::Object(obj)) => obj.push((field_name, val)),
                        Some(Builder::Tagged { fields, .. }) => fields.push((field_name, val)),
                        other => panic!(
                            "effect {effect_idx}: Set expects Object/Tagged on stack, found {:?}",
                            other.map(|b| b.kind())
                        ),
                    }
                }
                RuntimeEffect::EndObj => {
                    let top = stack.pop();
                    let Some(Builder::Object(fields)) = top else {
                        panic!(
                            "effect {effect_idx}: EndObj expects Object on stack, found {:?}",
                            top.as_ref().map(|b| b.kind())
                        );
                    };
                    if !fields.is_empty() {
                        // Non-empty object: always produce the object value
                        pending = Some(Value::Object(fields));
                    } else if pending.is_none() {
                        // Empty object with no pending value:
                        // - If nested (stack.len() > 1): produce empty object {}
                        //   This handles captured empty sequences like `{ } @x`
                        //   Note: stack always has at least the result_builder, so we check > 1
                        // - If at root (stack.len() <= 1): void result → null
                        if stack.len() > 1 {
                            pending = Some(Value::Object(vec![]));
                        }
                        // else: pending stays None (void result)
                    }
                    // else: pending has a value, keep it (passthrough for enums, suppressive, etc.)
                }
                RuntimeEffect::Enum(idx) => {
                    let tag = self.resolve_member_name(*idx);
                    let payload_type = self.resolve_member_type(*idx);
                    stack.push(Builder::Tagged {
                        tag,
                        payload_type,
                        fields: vec![],
                    });
                }
                RuntimeEffect::EndEnum => {
                    let top = stack.pop();
                    let Some(Builder::Tagged {
                        tag,
                        payload_type,
                        fields,
                    }) = top
                    else {
                        panic!(
                            "effect {effect_idx}: EndEnum expects Tagged on stack, found {:?}",
                            top.as_ref().map(|b| b.kind())
                        );
                    };
                    // Void payloads produce no $data field
                    let data = if self.is_void_type(payload_type) {
                        None
                    } else {
                        // If inner returned a structured value (via Obj/EndObj), use it as data
                        // Otherwise use fields collected from direct Set effects
                        Some(Box::new(pending.take().unwrap_or(Value::Object(fields))))
                    };
                    pending = Some(Value::Tagged { tag, data });
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
