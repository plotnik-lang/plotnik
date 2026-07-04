//! Materializes VM effect logs into output values.

use crate::bytecode::{Entrypoint, Module, StringsView, TypesView};
use crate::core::Colors;

use super::effect::RuntimeEffect;
use super::value::{NodeHandle, Value};
use super::verify::debug_verify_type;

pub struct ValueMaterializer<'a> {
    source: &'a str,
    types: TypesView<'a>,
    strings: StringsView<'a>,
}

impl<'a> ValueMaterializer<'a> {
    pub fn new(source: &'a str, module: &'a Module) -> Self {
        Self {
            source,
            types: module.types(),
            strings: module.strings(),
        }
    }

    fn resolve_member_name(&self, idx: u16) -> String {
        let member = self.types.get_member(idx as usize);
        self.strings.get(member.name_id).to_owned()
    }
}

/// Materialize the effect log into a [`Value`], then check it against the
/// entrypoint's declared type.
///
/// The type check is the trailing half of materialization, not an optional
/// follow-up: it catches materializer/typegen drift and compiles to a no-op in
/// release. Folding it in here keeps each call site from re-threading
/// `result_type` and from materializing a value that silently skips the check.
pub fn materialize_verified<'t>(
    source: &'t str,
    module: &Module,
    entrypoint: &Entrypoint,
    effects: &[RuntimeEffect<'t>],
    colors: Colors,
) -> Value {
    let materializer = ValueMaterializer::new(source, module);
    let value = materializer.materialize(effects);
    debug_verify_type(&value, entrypoint.result_type(), module, colors);
    value
}

/// Value accumulator for stack-based materialization.
enum ValueAccumulator {
    Array(Vec<Value>),
    Struct(Vec<(String, Value)>),
    Enum {
        tag: String,
        fields: Vec<(String, Value)>,
    },
}

impl ValueAccumulator {
    fn kind(&self) -> &'static str {
        match self {
            ValueAccumulator::Array(_) => "Array",
            ValueAccumulator::Struct(_) => "Struct",
            ValueAccumulator::Enum { .. } => "Enum",
        }
    }
}

impl ValueMaterializer<'_> {
    pub fn materialize<'t>(&self, effects: &[RuntimeEffect<'t>]) -> Value {
        let mut stack: Vec<ValueAccumulator> = vec![];

        // Pending value from Node/Null (consumed by Set/Push)
        let mut pending: Option<Value> = None;

        for (effect_idx, effect) in effects.iter().enumerate() {
            match effect {
                RuntimeEffect::Node(n) => {
                    pending = Some(Value::Node(NodeHandle::from_node(*n, self.source)));
                }
                RuntimeEffect::Null => {
                    pending = Some(Value::Null);
                }
                RuntimeEffect::ArrayOpen => {
                    stack.push(ValueAccumulator::Array(vec![]));
                }
                RuntimeEffect::Push => {
                    let val = pending
                        .take()
                        .expect("Push requires a produced value (verified at load)");
                    let Some(ValueAccumulator::Array(arr)) = stack.last_mut() else {
                        panic!(
                            "effect {effect_idx}: Push expects Array on stack, found {:?}",
                            stack.last().map(|b| b.kind())
                        );
                    };
                    arr.push(val);
                }
                RuntimeEffect::ArrayClose => {
                    let top = stack.pop();
                    let Some(ValueAccumulator::Array(arr)) = top else {
                        panic!(
                            "effect {effect_idx}: ArrayClose expects Array on stack, found {:?}",
                            top.as_ref().map(|b| b.kind())
                        );
                    };
                    pending = Some(Value::Array(arr));
                }
                RuntimeEffect::StructOpen => {
                    stack.push(ValueAccumulator::Struct(vec![]));
                }
                RuntimeEffect::Set(idx) => {
                    let field_name = self.resolve_member_name(*idx);
                    let val = pending
                        .take()
                        .expect("Set requires a produced value (verified at load)");
                    match stack.last_mut() {
                        Some(ValueAccumulator::Struct(fields)) => fields.push((field_name, val)),
                        Some(ValueAccumulator::Enum { fields, .. }) => {
                            fields.push((field_name, val))
                        }
                        other => panic!(
                            "effect {effect_idx}: Set expects Struct/Enum on stack, found {:?}",
                            other.map(|b| b.kind())
                        ),
                    }
                }
                RuntimeEffect::StructClose => {
                    let top = stack.pop();
                    let Some(ValueAccumulator::Struct(fields)) = top else {
                        panic!(
                            "effect {effect_idx}: StructClose expects Struct on stack, found {:?}",
                            top.as_ref().map(|b| b.kind())
                        );
                    };
                    pending = Some(Value::Struct(fields));
                }
                RuntimeEffect::EnumOpen(idx) => {
                    let tag = self.resolve_member_name(*idx);
                    stack.push(ValueAccumulator::Enum {
                        tag,
                        fields: vec![],
                    });
                }
                RuntimeEffect::EnumClose => {
                    let top = stack.pop();
                    let Some(ValueAccumulator::Enum { tag, fields }) = top else {
                        panic!(
                            "effect {effect_idx}: EnumClose expects Enum on stack, found {:?}",
                            top.as_ref().map(|b| b.kind())
                        );
                    };
                    let data = match (pending.take(), fields.is_empty()) {
                        (Some(v), true) => Some(Box::new(v)),
                        (None, false) => Some(Box::new(Value::Struct(fields))),
                        (None, true) => None,
                        (Some(_), false) => {
                            panic!(
                                "enum payload arrived both as pending value and as direct fields"
                            )
                        }
                    };
                    pending = Some(Value::Enum { tag, data });
                }
            }
        }

        debug_assert!(
            stack.is_empty(),
            "unclosed builder frames after materialization"
        );
        pending.unwrap_or(Value::Null)
    }
}
