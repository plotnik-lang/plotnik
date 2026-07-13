//! Materializes committed match journals into output values.

use crate::bytecode::{EntryPoint, Module};
use crate::core::Colors;

use super::value::{NodeValue, Value};
use super::verify::debug_verify_type;
use plotnik_rt::{JournalEvent, OutputEvents};

pub struct ValueMaterializer<'a> {
    source: &'a str,
    /// Member names resolved once, indexed by the `RecordSet`/`VariantOpen` payload.
    /// Kills the two-table lookup and the string-table UTF-8 walk per event.
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
        // Journal event payloads are validated at module load; out of bounds here is
        // a loader bug, and the slice-index panic is the assertion.
        self.member_names[idx as usize]
    }
}

/// Materialize the journal events into a [`Value`], then check it against the
/// entry point's declared type.
///
/// The type check is the trailing half of materialization, not an optional
/// follow-up: it catches materializer/typegen drift and compiles to a no-op in
/// release. Folding it in here keeps each call site from re-threading
/// `result_type` and from materializing a value that silently skips the check.
pub fn materialize_verified<'s>(
    source: &'s str,
    module: &'s Module,
    entry_point: &EntryPoint,
    events: OutputEvents<'_, '_>,
    colors: Colors,
) -> Value<'s> {
    let materializer = ValueMaterializer::new(source, module);
    let value = materializer.materialize(events);
    debug_verify_type(&value, entry_point.result_type(), module, colors);
    value
}

/// Value accumulator for stack-based materialization.
enum ValueAccumulator<'s> {
    List(Vec<Value<'s>>),
    Record(Vec<(&'s str, Value<'s>)>),
    Variant {
        case: &'s str,
        fields: Vec<(&'s str, Value<'s>)>,
    },
    /// Marker into the scalar-only range stack. Keeping the marker here
    /// preserves heterogeneous frame nesting checks without making ScalarMark
    /// scan lists, records, and variants.
    Scalar(usize),
}

impl ValueAccumulator<'_> {
    fn kind(&self) -> &'static str {
        match self {
            ValueAccumulator::List(_) => "List",
            ValueAccumulator::Record(_) => "Record",
            ValueAccumulator::Variant { .. } => "Variant",
            ValueAccumulator::Scalar(_) => "Scalar",
        }
    }
}

impl<'a> ValueMaterializer<'a> {
    pub fn materialize(&self, events: OutputEvents<'_, '_>) -> Value<'a> {
        let mut stack: Vec<ValueAccumulator<'a>> = vec![];
        let mut scalar_ranges: Vec<Option<std::ops::Range<usize>>> = vec![];

        // Pending output value consumed by `RecordSet` or `ArrayPush`.
        let mut pending: Option<Value<'a>> = None;

        for (event_idx, event) in events.iter().enumerate() {
            match event {
                JournalEvent::Node(n) => {
                    pending = Some(Value::Node(NodeValue::from_node(*n, self.source)));
                }
                JournalEvent::Absent => {
                    pending = Some(Value::Absent);
                }
                JournalEvent::NodeStr(node) => {
                    pending = Some(Value::Text(plotnik_rt::node_text(self.source, node)));
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
                    for range in &mut scalar_ranges {
                        *range = Some(match range.take() {
                            Some(current) => {
                                current.start.min(mark.start)..current.end.max(mark.end)
                            }
                            None => mark.clone(),
                        });
                    }
                }
                JournalEvent::StrClose => {
                    let top = stack.pop();
                    let Some(ValueAccumulator::Scalar(scalar)) = top else {
                        panic!(
                            "event {event_idx}: StrClose expects Scalar on stack, found {:?}",
                            top.as_ref().map(|frame| frame.kind())
                        );
                    };
                    assert_eq!(
                        scalar + 1,
                        scalar_ranges.len(),
                        "event {event_idx}: StrClose violates scalar frame nesting"
                    );
                    let range = scalar_ranges
                        .pop()
                        .expect("Scalar marker owns a range frame");
                    pending = Some(match range {
                        Some(range) => Value::Text(plotnik_rt::source_text(self.source, range)),
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
                    let field_name = self.resolve_member_name(*idx);
                    let val = pending
                        .take()
                        .expect("RecordSet requires a produced value (verified at load)");
                    match stack.last_mut() {
                        Some(ValueAccumulator::Record(fields)) => fields.push((field_name, val)),
                        Some(ValueAccumulator::Variant { fields, .. }) => {
                            fields.push((field_name, val))
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
                    let case = self.resolve_member_name(*idx);
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
