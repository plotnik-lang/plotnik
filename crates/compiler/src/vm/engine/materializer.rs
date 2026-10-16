use crate::bytecode::{Entrypoint, Module, TypeDefKind};
use crate::core::Colors;

use super::value::{NodeValue, Value};
use super::verify::debug_verify_type;
use plotnik_runtime::{JournalEvent, OutputEvents};

pub struct ValueMaterializer<'s, 't> {
    source: &'s str,
    module: &'s Module,
    member_names: Box<[&'s str]>,
    record_widths: Box<[u8]>,
    stack: Vec<ValueAccumulator<'s, 't>>,
    scalar_ranges: Vec<Option<std::ops::Range<usize>>>,
}

impl<'s, 't> ValueMaterializer<'s, 't> {
    pub fn new(source: &'s str, module: &'s Module) -> Self {
        let types = module.types();
        let strings = module.strings();
        let member_names = types.members().map(|m| strings.get(m.name_id)).collect();
        let mut record_widths = vec![0; types.members_count()];
        for def in types.iter() {
            if let TypeDefKind::Record {
                member_start,
                member_count,
            } = def.decode()
            {
                let start = usize::from(member_start);
                record_widths[start..start + usize::from(member_count)].fill(member_count);
            }
        }
        Self {
            source,
            module,
            member_names,
            record_widths: record_widths.into_boxed_slice(),
            stack: Vec::new(),
            scalar_ranges: Vec::new(),
        }
    }

    pub fn materialize_verified(
        &mut self,
        entrypoint: &Entrypoint,
        events: OutputEvents<'_, 't>,
        colors: Colors,
    ) -> Value<'s, 't> {
        let value = self.materialize(events);
        debug_verify_type(&value, entrypoint.result_type(), self.module, colors);
        value
    }
}

/// Materialize the journal events into a [`Value`], then check it against the
/// entrypoint's declared type.
///
/// Type checking is debug-only and stops at 512 nested values.
pub fn materialize_verified<'s, 't>(
    source: &'s str,
    module: &'s Module,
    entrypoint: &Entrypoint,
    events: OutputEvents<'_, 't>,
    colors: Colors,
) -> Value<'s, 't> {
    ValueMaterializer::new(source, module).materialize_verified(entrypoint, events, colors)
}

enum ValueAccumulator<'s, 't> {
    List(Vec<Value<'s, 't>>),
    Record(Vec<(&'s str, Value<'s, 't>)>),
    Variant {
        case: &'s str,
        fields: Vec<(&'s str, Value<'s, 't>)>,
    },
    Scalar(usize),
}

impl ValueAccumulator<'_, '_> {
    fn kind(&self) -> &'static str {
        match self {
            ValueAccumulator::List(_) => "List",
            ValueAccumulator::Record(_) => "Record",
            ValueAccumulator::Variant { .. } => "Variant",
            ValueAccumulator::Scalar(_) => "Scalar",
        }
    }
}

impl<'s, 't> ValueMaterializer<'s, 't> {
    pub fn materialize(&mut self, events: OutputEvents<'_, 't>) -> Value<'s, 't> {
        let Self {
            source,
            module: _,
            member_names,
            record_widths,
            stack,
            scalar_ranges,
        } = self;
        assert!(stack.is_empty(), "previous builder frames must be closed");
        assert!(
            scalar_ranges.is_empty(),
            "previous scalar frames must be closed"
        );

        let mut pending: Option<Value<'s, 't>> = None;

        for (event_idx, event) in events.iter().enumerate() {
            match event {
                JournalEvent::Node(n) => {
                    pending = Some(Value::Node(NodeValue::from_node(*n, source)));
                }
                JournalEvent::Absent => {
                    pending = Some(Value::Absent);
                }
                JournalEvent::NodeText(node) => {
                    pending = Some(Value::Text(plotnik_runtime::node_text(source, node)));
                }
                JournalEvent::NodeBool(_) => {
                    pending = Some(Value::Bool(true));
                }
                JournalEvent::BoolValue(value) => {
                    pending = Some(Value::Bool(*value));
                }
                JournalEvent::ScalarOpen => {
                    let scalar = scalar_ranges.len();
                    scalar_ranges.push(None);
                    stack.push(ValueAccumulator::Scalar(scalar));
                }
                JournalEvent::ScalarMark(node) => {
                    let mark = node.start_byte()..node.end_byte();
                    for range in scalar_ranges.iter_mut() {
                        *range = Some(match range.take() {
                            Some(current) => {
                                current.start.min(mark.start)..current.end.max(mark.end)
                            }
                            None => mark.clone(),
                        });
                    }
                }
                JournalEvent::TextClose => {
                    let top = stack.pop();
                    let Some(ValueAccumulator::Scalar(scalar)) = top else {
                        panic!(
                            "event {event_idx}: TextClose expects Scalar on stack, found {:?}",
                            top.as_ref().map(|frame| frame.kind())
                        );
                    };
                    assert_eq!(
                        scalar + 1,
                        scalar_ranges.len(),
                        "event {event_idx}: TextClose violates scalar frame nesting"
                    );
                    let range = scalar_ranges
                        .pop()
                        .expect("Scalar marker owns a range frame");
                    pending = Some(match range {
                        Some(range) => Value::Text(plotnik_runtime::source_text(source, range)),
                        None => Value::Absent,
                    });
                }
                JournalEvent::BoolClose(value) => {
                    let top = stack.pop();
                    let Some(ValueAccumulator::Scalar(scalar)) = top else {
                        panic!(
                            "event {event_idx}: BoolClose expects Scalar on stack, found {:?}",
                            top.as_ref().map(|frame| frame.kind())
                        );
                    };
                    assert_eq!(
                        scalar + 1,
                        scalar_ranges.len(),
                        "event {event_idx}: BoolClose violates scalar frame nesting"
                    );
                    scalar_ranges
                        .pop()
                        .expect("Scalar marker owns a range frame");
                    pending = Some(Value::Bool(*value));
                }
                JournalEvent::SpanStart { .. } | JournalEvent::SpanEnd(_) => {}
                JournalEvent::ListOpen => {
                    stack.push(ValueAccumulator::List(vec![]));
                }
                JournalEvent::ArrayPush => {
                    let val = pending
                        .take()
                        .expect("ArrayPush requires a produced value (verified at load)");
                    let Some(ValueAccumulator::List(items)) = stack.last_mut() else {
                        panic!(
                            "event {event_idx}: ArrayPush expects List on stack, found {:?}",
                            stack.last().map(|b| b.kind())
                        );
                    };
                    items.push(val);
                }
                JournalEvent::ListClose => {
                    let top = stack.pop();
                    let Some(ValueAccumulator::List(items)) = top else {
                        panic!(
                            "event {event_idx}: ListClose expects List on stack, found {:?}",
                            top.as_ref().map(|b| b.kind())
                        );
                    };
                    pending = Some(Value::List(items));
                }
                JournalEvent::RecordOpen => {
                    stack.push(ValueAccumulator::Record(vec![]));
                }
                JournalEvent::RecordSet(idx) => {
                    let field_name = member_names[*idx as usize];
                    let val = pending
                        .take()
                        .expect("RecordSet requires a produced value (verified at load)");
                    match stack.last_mut() {
                        Some(
                            ValueAccumulator::Record(fields)
                            | ValueAccumulator::Variant { fields, .. },
                        ) => {
                            if fields.is_empty() {
                                fields.reserve_exact(usize::from(record_widths[*idx as usize]));
                            }
                            fields.push((field_name, val));
                        }
                        other => panic!(
                            "event {event_idx}: RecordSet expects Record/Variant on stack, found {:?}",
                            other.map(|b| b.kind())
                        ),
                    }
                }
                JournalEvent::RecordClose => {
                    let top = stack.pop();
                    let Some(ValueAccumulator::Record(fields)) = top else {
                        panic!(
                            "event {event_idx}: RecordClose expects Record on stack, found {:?}",
                            top.as_ref().map(|b| b.kind())
                        );
                    };
                    pending = Some(Value::Record(fields));
                }
                JournalEvent::VariantOpen(idx) => {
                    let case = member_names[*idx as usize];
                    stack.push(ValueAccumulator::Variant {
                        case,
                        fields: vec![],
                    });
                }
                JournalEvent::VariantClose => {
                    let top = stack.pop();
                    let Some(ValueAccumulator::Variant { case, fields }) = top else {
                        panic!(
                            "event {event_idx}: VariantClose expects Variant on stack, found {:?}",
                            top.as_ref().map(|b| b.kind())
                        );
                    };
                    let payload = match (pending.take(), fields.is_empty()) {
                        (Some(v), true) => Some(Box::new(v)),
                        (None, false) => Some(Box::new(Value::Record(fields))),
                        (None, true) => None,
                        (Some(_), false) => {
                            panic!(
                                "variant payload arrived both as pending value and as direct fields"
                            )
                        }
                    };
                    pending = Some(Value::Variant { case, payload });
                }
            }
        }

        assert!(
            stack.is_empty(),
            "unclosed builder frames after materialization"
        );
        assert!(
            scalar_ranges.is_empty(),
            "unclosed scalar frames after materialization"
        );
        pending.unwrap_or(Value::Absent)
    }
}
