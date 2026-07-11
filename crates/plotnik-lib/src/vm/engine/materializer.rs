//! Materializes VM effect logs into output values.

use crate::bytecode::{Entrypoint, Module};
use crate::core::Colors;

use super::value::{NodeHandle, Value};
use super::verify::debug_verify_type;
use plotnik_rt::RuntimeEffect;

pub struct ValueMaterializer<'a> {
    source: &'a str,
    /// Member names resolved once, indexed by the Set/EnumOpen payload.
    /// Kills the two-table lookup and the string-table UTF-8 walk per effect.
    member_names: Box<[&'a str]>,
}

impl<'a> ValueMaterializer<'a> {
    pub fn new(source: &'a str, module: &'a Module) -> Self {
        let types = module.types();
        let strings = module.strings();
        let member_names = types.members().map(|m| strings.get(m.name_id)).collect();
        Self {
            source,
            member_names,
        }
    }

    fn resolve_member_name(&self, idx: u16) -> &'a str {
        // Effect payloads are validated during internal construction; out of bounds here is
        // a loader bug, and the slice-index panic is the assertion.
        self.member_names[idx as usize]
    }
}

/// Materialize the effect log into a [`Value`], then check it against the
/// entrypoint's declared type.
///
/// The type check is the trailing half of materialization, not an optional
/// follow-up: it catches materializer/typegen drift and compiles to a no-op in
/// release. Folding it in here keeps each call site from re-threading
/// `result_type` and from materializing a value that silently skips the check.
pub fn materialize_verified<'s>(
    source: &'s str,
    module: &'s Module,
    entrypoint: &Entrypoint,
    effects: &[RuntimeEffect<'_>],
    colors: Colors,
) -> Value<'s> {
    let materializer = ValueMaterializer::new(source, module);
    let value = materializer.materialize(effects);
    debug_verify_type(&value, entrypoint.result_type(), module, colors);
    value
}

/// Value accumulator for stack-based materialization.
enum ValueAccumulator<'s> {
    Array(Vec<Value<'s>>),
    Struct(Vec<(&'s str, Value<'s>)>),
    Enum {
        tag: &'s str,
        fields: Vec<(&'s str, Value<'s>)>,
    },
}

impl ValueAccumulator<'_> {
    fn kind(&self) -> &'static str {
        match self {
            ValueAccumulator::Array(_) => "Array",
            ValueAccumulator::Struct(_) => "Struct",
            ValueAccumulator::Enum { .. } => "Enum",
        }
    }
}

impl<'a> ValueMaterializer<'a> {
    pub fn materialize(&self, effects: &[RuntimeEffect<'_>]) -> Value<'a> {
        let mut stack: Vec<ValueAccumulator<'a>> = vec![];

        // Pending value from Node/Null (consumed by Set/Push)
        let mut pending: Option<Value<'a>> = None;

        for (effect_idx, effect) in effects.iter().enumerate() {
            match effect {
                RuntimeEffect::Node(n) => {
                    pending = Some(Value::Node(NodeHandle::from_node(*n, self.source)));
                }
                RuntimeEffect::Null => {
                    pending = Some(Value::Null);
                }
                RuntimeEffect::SpanStart { .. } | RuntimeEffect::SpanEnd(_) => {}
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

        assert!(
            stack.is_empty(),
            "unclosed builder frames after materialization"
        );
        pending.unwrap_or(Value::Null)
    }
}
