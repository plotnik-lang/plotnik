use std::fmt;

use serde::Serialize;
use tree_sitter::Node;

use super::trace::Tracer;
use crate::bytecode::{CodeAddr, EffectKind, Instruction, Module, Nav};
use crate::core::NodeFieldId;
use plotnik_runtime::{Account, Buffer, JournalEvent, LimitExceeded, Meter};

#[derive(Debug, Serialize)]
pub struct ExecutionTrace<'m> {
    pub v: u32,
    pub records: Buffer<TraceRecord<'m>>,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
pub struct TraceRecord<'m> {
    pub ip: u16,
    pub event: TraceEvent<'m>,
    pub query_span_id: Option<u16>,
    pub node: Option<TraceNode>,
    pub journal_len: u32,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TraceEvent<'m> {
    Instruction,
    Nav,
    NavFail,
    MatchOk,
    MatchFail,
    FieldOk,
    FieldFail,
    PredicateFail,
    NegFieldFail,
    Effect { effect: TraceLabel<'m> },
    JournalEvent { event: TraceLabel<'m> },
    SuppressedEffect { effect: TraceLabel<'m> },
    Call { target: u16 },
    Return,
    CheckpointNew,
    Backtrack { to_record: u32 },
    ExecutionStarted { target: u16 },
}

/// Labels borrow prepared member names instead of allocating formatted strings.
#[derive(Debug)]
pub enum TraceLabel<'m> {
    Plain(&'static str),
    Member(&'static str, &'m str),
    Id(&'static str, usize),
    Bool(&'static str, bool),
}

impl fmt::Display for TraceLabel<'_> {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plain(name) => out.write_str(name),
            Self::Member(name, member) => write!(out, "{name} \"{member}\""),
            Self::Id(name, id) => write!(out, "{name}#{id}"),
            Self::Bool(name, value) => write!(out, "{name}({value})"),
        }
    }
}

impl Serialize for TraceLabel<'_> {
    fn serialize<S: serde::Serializer>(&self, out: S) -> Result<S::Ok, S::Error> {
        out.collect_str(self)
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct TraceNode {
    pub kind_id: u16,
    pub start: u32,
    pub end: u32,
}

struct Shadow {
    span: Option<usize>,
    journal_len: u32,
    record_idx: u32,
}

struct Span {
    id: u16,
    parent: Option<usize>,
}

pub struct TraceRecorder<'m> {
    module: &'m Module,
    account: Option<Account>,
    error: Option<LimitExceeded>,
    records: Buffer<TraceRecord<'m>>,
    spans: Buffer<Span>,
    span: Option<usize>,
    shadow: Buffer<Shadow>,
    max_records: usize,
    truncated: bool,
    current_ip: CodeAddr,
    journal_len: u32,
    records_seen: u32,
}

impl<'m> TraceRecorder<'m> {
    pub fn new(module: &'m Module, max_records: usize) -> Self {
        Self {
            module,
            account: None,
            error: None,
            records: Buffer::new(),
            spans: Buffer::new(),
            span: None,
            shadow: Buffer::new(),
            max_records,
            truncated: false,
            current_ip: CodeAddr::ZERO,
            journal_len: 0,
            records_seen: 0,
        }
    }

    pub fn finish(self) -> ExecutionTrace<'m> {
        ExecutionTrace {
            v: 0,
            records: self.records,
            truncated: self.truncated || self.error.is_some(),
        }
    }

    pub(super) fn records(&self) -> &[TraceRecord<'m>] {
        self.records.as_slice()
    }

    fn record(
        &mut self,
        call: impl FnOnce(&mut Self, &mut Meter<'_>) -> Result<(), LimitExceeded>,
    ) {
        if self.error.is_some() {
            return;
        }

        let account = self
            .account
            .as_ref()
            .expect("tracer is attached before callbacks")
            .clone();
        let result = call(self, &mut account.meter());
        if let Err(error) = result {
            self.error = Some(error);
        }
    }

    fn add(
        &mut self,
        meter: &mut Meter<'_>,
        ip: CodeAddr,
        event: TraceEvent<'m>,
        node: Option<TraceNode>,
    ) -> Result<u32, LimitExceeded> {
        meter.charge(1)?;
        let index = self.records_seen;
        self.records_seen = index
            .checked_add(1)
            .ok_or(LimitExceeded::Allocation { bytes: u64::MAX })?;
        if self.records.len() < self.max_records {
            self.records.push(
                meter,
                TraceRecord {
                    ip: ip.get(),
                    event,
                    query_span_id: self.span.map(|index| self.spans[index].id),
                    node,
                    journal_len: self.journal_len,
                },
            )?;
        } else {
            self.truncated = true;
        }
        Ok(index)
    }

    fn event(&mut self, event: TraceEvent<'m>, node: Option<TraceNode>) {
        self.record(|this, meter| {
            this.add(meter, this.current_ip, event, node)?;
            Ok(())
        });
    }

    fn label(&self, kind: EffectKind, payload: usize) -> TraceLabel<'m> {
        use EffectKind::*;
        match kind {
            RecordSet | VariantOpen => TraceLabel::Member(
                if kind == RecordSet {
                    "RecordSet"
                } else {
                    "VariantOpen"
                },
                self.module
                    .strings()
                    .get(self.module.types().get_member(payload).name_id),
            ),
            SpanStartAt | SpanStart | SpanEnd => TraceLabel::Id(
                match kind {
                    SpanStartAt => "SpanStartAt",
                    SpanStart => "SpanStart",
                    _ => "SpanEnd",
                },
                payload,
            ),
            BoolClose | BoolValue => TraceLabel::Bool(
                if kind == BoolClose {
                    "BoolClose"
                } else {
                    "BoolValue"
                },
                payload != 0,
            ),
            _ => TraceLabel::Plain(match kind {
                Node => "Node",
                ListOpen => "ListOpen",
                ArrayPush => "ArrayPush",
                ListClose => "ListClose",
                RecordOpen => "RecordOpen",
                RecordClose => "RecordClose",
                VariantClose => "VariantClose",
                Absent => "Absent",
                ScalarOpen => "ScalarOpen",
                ScalarMark => "ScalarMark",
                TextClose => "TextClose",
                NodeText => "NodeText",
                NodeBool => "NodeBool",
                SuppressBegin => "SuppressBegin",
                SuppressEnd => "SuppressEnd",
                _ => unreachable!("operand labels handled above"),
            }),
        }
    }
}

impl Tracer for TraceRecorder<'_> {
    fn start(&mut self, account: &Account) {
        self.account = Some(account.clone());
    }
    fn check(&self) -> Result<(), LimitExceeded> {
        self.error.map_or(Ok(()), Err)
    }
    fn trace_instruction(&mut self, ip: CodeAddr, _: &Instruction<'_>) {
        self.current_ip = ip;
        self.event(TraceEvent::Instruction, None);
    }
    fn trace_nav(&mut self, _: Nav, node: Node<'_>) {
        self.event(TraceEvent::Nav, Some(trace_node(node)));
    }
    fn trace_nav_failure(&mut self, _: Nav) {
        self.event(TraceEvent::NavFail, None);
    }
    fn trace_match_success(&mut self, node: Node<'_>) {
        self.event(TraceEvent::MatchOk, Some(trace_node(node)));
    }
    fn trace_match_failure(&mut self, node: Node<'_>) {
        self.event(TraceEvent::MatchFail, Some(trace_node(node)));
    }
    fn trace_field_success(&mut self, _: NodeFieldId) {
        self.event(TraceEvent::FieldOk, None);
    }
    fn trace_field_failure(&mut self, node: Node<'_>) {
        self.event(TraceEvent::FieldFail, Some(trace_node(node)));
    }
    fn trace_predicate_failure(&mut self, node: Node<'_>) {
        self.event(TraceEvent::PredicateFail, Some(trace_node(node)));
    }
    fn trace_neg_field_failure(&mut self, node: Node<'_>, _: NodeFieldId) {
        self.event(TraceEvent::NegFieldFail, Some(trace_node(node)));
    }
    fn trace_journal_event(&mut self, event: &JournalEvent<'_>) {
        use JournalEvent::*;
        let (kind, payload, node) = match *event {
            Node(node) => (EffectKind::Node, 0, Some(node)),
            NodeText(node) => (EffectKind::NodeText, 0, Some(node)),
            NodeBool(node) => (EffectKind::NodeBool, 0, Some(node)),
            ScalarMark(node) => (EffectKind::ScalarMark, 0, Some(node)),
            ListOpen => (EffectKind::ListOpen, 0, None),
            ArrayPush => (EffectKind::ArrayPush, 0, None),
            ListClose => (EffectKind::ListClose, 0, None),
            RecordOpen => (EffectKind::RecordOpen, 0, None),
            RecordClose => (EffectKind::RecordClose, 0, None),
            RecordSet(id) => (EffectKind::RecordSet, id as usize, None),
            VariantOpen(id) => (EffectKind::VariantOpen, id as usize, None),
            VariantClose => (EffectKind::VariantClose, 0, None),
            Absent => (EffectKind::Absent, 0, None),
            ScalarOpen => (EffectKind::ScalarOpen, 0, None),
            TextClose => (EffectKind::TextClose, 0, None),
            BoolClose(value) => (EffectKind::BoolClose, value as usize, None),
            BoolValue(value) => (EffectKind::BoolValue, value as usize, None),
            SpanStart { id, node } => (
                if node.is_some() {
                    EffectKind::SpanStartAt
                } else {
                    EffectKind::SpanStart
                },
                id as usize,
                node,
            ),
            SpanEnd(id) => (EffectKind::SpanEnd, id as usize, None),
        };
        let label = self.label(kind, payload);
        self.record(|this, meter| {
            this.journal_len = this
                .journal_len
                .checked_add(1)
                .ok_or(LimitExceeded::Allocation { bytes: u64::MAX })?;
            if let SpanStart { id, .. } = event {
                let index = this.spans.len();
                this.spans.push(
                    meter,
                    Span {
                        id: *id,
                        parent: this.span,
                    },
                )?;
                this.span = Some(index);
            }
            this.add(
                meter,
                this.current_ip,
                TraceEvent::JournalEvent { event: label },
                node.map(trace_node),
            )?;
            if let SpanEnd(id) = event {
                let span = &this.spans[this.span.expect("open trace span")];
                assert_eq!(span.id, *id, "balanced trace spans");
                this.span = span.parent;
            }
            Ok(())
        });
    }
    fn trace_effect_suppressed(&mut self, kind: EffectKind, payload: usize) {
        self.event(
            TraceEvent::SuppressedEffect {
                effect: self.label(kind, payload),
            },
            None,
        );
    }
    fn trace_suppress_control(&mut self, kind: EffectKind, suppressed: bool) {
        let effect = self.label(kind, 0);
        self.event(
            if suppressed {
                TraceEvent::SuppressedEffect { effect }
            } else {
                TraceEvent::Effect { effect }
            },
            None,
        );
    }
    fn trace_call(&mut self, target: CodeAddr) {
        self.event(
            TraceEvent::Call {
                target: target.get(),
            },
            None,
        );
    }
    fn trace_return(&mut self, _: plotnik_runtime::PortId) {
        self.event(TraceEvent::Return, None);
    }
    fn trace_checkpoint_created(&mut self, ip: CodeAddr) {
        self.record(|this, meter| {
            let record_idx = this.add(meter, ip, TraceEvent::CheckpointNew, None)?;
            this.shadow.push(
                meter,
                Shadow {
                    span: this.span,
                    journal_len: this.journal_len,
                    record_idx,
                },
            )
        });
    }
    fn trace_backtrack(&mut self, _: u32) {
        self.record(|this, meter| {
            let shadow = this.shadow.pop().expect("trace checkpoint exists");
            this.span = shadow.span;
            this.journal_len = shadow.journal_len;
            this.add(
                meter,
                this.current_ip,
                TraceEvent::Backtrack {
                    to_record: shadow.record_idx,
                },
                None,
            )?;
            Ok(())
        });
    }
    fn trace_execution_started(&mut self, target: CodeAddr) {
        self.spans.clear();
        self.span = None;
        self.shadow.clear();
        self.journal_len = 0;
        self.current_ip = target;
        self.event(
            TraceEvent::ExecutionStarted {
                target: target.get(),
            },
            None,
        );
    }
}

fn trace_node(node: Node<'_>) -> TraceNode {
    TraceNode {
        kind_id: node.kind_id(),
        start: u32::try_from(node.start_byte()).expect("node offset fits u32"),
        end: u32::try_from(node.end_byte()).expect("node offset fits u32"),
    }
}
