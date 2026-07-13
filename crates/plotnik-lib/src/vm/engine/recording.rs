//! Structured VM run recording for debugger-oriented trace output.

use serde::Serialize;
use tree_sitter::Node;

use crate::bytecode::{CodeAddr, EffectKind, Instruction, Module, ModuleRenderContext, Nav};
use crate::core::NodeFieldId;

use super::trace::Tracer;
use plotnik_rt::JournalEvent;

#[derive(Debug, Serialize)]
pub struct Recording {
    pub v: u32,
    pub steps: Vec<StepRecord>,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
pub struct StepRecord {
    pub ip: u16,
    pub event: StepEvent,
    pub span: Option<u16>,
    pub node: Option<NodeRef>,
    pub journal_len: u32,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StepEvent {
    Instruction,
    Nav,
    NavFail,
    MatchOk,
    MatchFail,
    FieldOk,
    FieldFail,
    PredicateFail,
    NegFieldFail,
    Effect { effect: String },
    JournalEvent { event: String },
    SuppressedEffect { effect: String },
    Call { target: u16 },
    Return,
    CheckpointNew,
    Backtrack { to_step: u32 },
    EnterEntrypoint { target: u16 },
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct NodeRef {
    pub kind_id: u16,
    pub start: u32,
    pub end: u32,
}

struct Shadow {
    span_depth: usize,
    journal_len: u32,
    record_idx: u32,
}

pub struct RecordingTracer {
    render: ModuleRenderContext,
    steps: Vec<StepRecord>,
    span_stack: Vec<u16>,
    shadow: Vec<Shadow>,
    max_records: usize,
    truncated: bool,
    current_ip: CodeAddr,
    journal_len: u32,
    records_seen: u32,
}

impl RecordingTracer {
    pub fn new(module: &Module, max_records: usize) -> Self {
        Self {
            render: ModuleRenderContext::new(module),
            steps: Vec::new(),
            span_stack: Vec::new(),
            shadow: Vec::new(),
            max_records,
            truncated: false,
            current_ip: CodeAddr::ZERO,
            journal_len: 0,
            records_seen: 0,
        }
    }

    pub fn finish(self) -> Recording {
        Recording {
            v: 1,
            steps: self.steps,
            truncated: self.truncated,
        }
    }

    fn add_record(&mut self, event: StepEvent, node: Option<NodeRef>) -> u32 {
        self.add_record_at(self.current_ip, event, node)
    }

    fn add_record_at(&mut self, ip: CodeAddr, event: StepEvent, node: Option<NodeRef>) -> u32 {
        let record_idx = self.records_seen;
        self.records_seen = self
            .records_seen
            .checked_add(1)
            .expect("recording step count fits in u32");

        if self.steps.len() < self.max_records {
            self.steps.push(StepRecord {
                ip: ip.get(),
                event,
                span: self.span_stack.last().copied(),
                node,
                journal_len: self.journal_len,
            });
        } else {
            self.truncated = true;
        }

        record_idx
    }

    /// Whether the next record still lands in the buffer. Once saturated,
    /// formatting effect strings would be wasted allocation — `add_record`
    /// drops the event anyway.
    fn keeps_records(&self) -> bool {
        self.steps.len() < self.max_records
    }

    fn bump_journal_len(&mut self) {
        self.journal_len = self
            .journal_len
            .checked_add(1)
            .expect("match journal length fits in u32");
    }

    fn member_name(&self, idx: u16) -> &str {
        self.render
            .member_name(idx)
            .expect("effect member index names a type member")
    }

    fn format_journal_event(&self, event: &JournalEvent<'_>) -> String {
        match event {
            JournalEvent::Node(_) => "Node".to_string(),
            JournalEvent::ListOpen => "ListOpen".to_string(),
            JournalEvent::ArrayPush => "ArrayPush".to_string(),
            JournalEvent::ListClose => "ListClose".to_string(),
            JournalEvent::RecordOpen => "RecordOpen".to_string(),
            JournalEvent::RecordClose => "RecordClose".to_string(),
            JournalEvent::RecordSet(idx) => {
                format!("RecordSet \"{}\"", self.member_name(*idx))
            }
            JournalEvent::VariantOpen(idx) => {
                format!("VariantOpen \"{}\"", self.member_name(*idx))
            }
            JournalEvent::VariantClose => "VariantClose".to_string(),
            JournalEvent::Absent => "Absent".to_string(),
            JournalEvent::ScalarOpen => "ScalarOpen".to_string(),
            JournalEvent::ScalarMark(_) => "ScalarMark".to_string(),
            JournalEvent::StrClose => "StrClose".to_string(),
            JournalEvent::BoolClose(value) => format!("BoolClose({value})"),
            JournalEvent::NodeStr(_) => "NodeStr".to_string(),
            JournalEvent::NodeBool(_) => "NodeBool".to_string(),
            JournalEvent::BoolValue(value) => format!("BoolValue({value})"),
            JournalEvent::SpanStart { id, node } => {
                if node.is_some() {
                    format!("SpanStartAt#{id}")
                } else {
                    format!("SpanStart#{id}")
                }
            }
            JournalEvent::SpanEnd(id) => format!("SpanEnd#{id}"),
        }
    }

    fn format_opcode(&self, opcode: EffectKind, payload: usize) -> String {
        match opcode {
            EffectKind::Node => "Node".to_string(),
            EffectKind::ListOpen => "ListOpen".to_string(),
            EffectKind::ArrayPush => "ArrayPush".to_string(),
            EffectKind::ListClose => "ListClose".to_string(),
            EffectKind::RecordOpen => "RecordOpen".to_string(),
            EffectKind::RecordClose => "RecordClose".to_string(),
            EffectKind::RecordSet => {
                format!("RecordSet \"{}\"", self.member_name(payload as u16))
            }
            EffectKind::VariantOpen => {
                format!("VariantOpen \"{}\"", self.member_name(payload as u16))
            }
            EffectKind::VariantClose => "VariantClose".to_string(),
            EffectKind::Absent => "Absent".to_string(),
            EffectKind::SuppressBegin => "SuppressBegin".to_string(),
            EffectKind::SuppressEnd => "SuppressEnd".to_string(),
            EffectKind::SpanStartAt => format!("SpanStartAt#{payload}"),
            EffectKind::SpanStart => format!("SpanStart#{payload}"),
            EffectKind::SpanEnd => format!("SpanEnd#{payload}"),
            EffectKind::ScalarOpen => "ScalarOpen".to_string(),
            EffectKind::ScalarMark => "ScalarMark".to_string(),
            EffectKind::StrClose => "StrClose".to_string(),
            EffectKind::BoolClose => format!("BoolClose({})", payload != 0),
            EffectKind::NodeStr => "NodeStr".to_string(),
            EffectKind::NodeBool => "NodeBool".to_string(),
            EffectKind::BoolValue => format!("BoolValue({})", payload != 0),
        }
    }

    fn event_node(event: &JournalEvent<'_>) -> Option<NodeRef> {
        match event {
            JournalEvent::Node(node) => Some(node_ref(*node)),
            JournalEvent::ScalarMark(node) => Some(node_ref(*node)),
            JournalEvent::NodeStr(node) | JournalEvent::NodeBool(node) => Some(node_ref(*node)),
            JournalEvent::SpanStart {
                node: Some(node), ..
            } => Some(node_ref(*node)),
            _ => None,
        }
    }
}

impl Tracer for RecordingTracer {
    fn trace_instruction(&mut self, ip: CodeAddr, _instr: &Instruction<'_>) {
        self.current_ip = ip;
        self.add_record(StepEvent::Instruction, None);
    }

    fn trace_nav(&mut self, _nav: Nav, node: Node<'_>) {
        self.add_record(StepEvent::Nav, Some(node_ref(node)));
    }

    fn trace_nav_failure(&mut self, _nav: Nav) {
        self.add_record(StepEvent::NavFail, None);
    }

    fn trace_match_success(&mut self, node: Node<'_>) {
        self.add_record(StepEvent::MatchOk, Some(node_ref(node)));
    }

    fn trace_match_failure(&mut self, node: Node<'_>) {
        self.add_record(StepEvent::MatchFail, Some(node_ref(node)));
    }

    fn trace_field_success(&mut self, _field_id: NodeFieldId) {
        self.add_record(StepEvent::FieldOk, None);
    }

    fn trace_field_failure(&mut self, node: Node<'_>) {
        self.add_record(StepEvent::FieldFail, Some(node_ref(node)));
    }

    fn trace_predicate_failure(&mut self, node: Node<'_>) {
        self.add_record(StepEvent::PredicateFail, Some(node_ref(node)));
    }

    fn trace_neg_field_failure(&mut self, node: Node<'_>, _field: NodeFieldId) {
        self.add_record(StepEvent::NegFieldFail, Some(node_ref(node)));
    }

    fn trace_journal_event(&mut self, event: &JournalEvent<'_>) {
        let event_name = if self.keeps_records() {
            self.format_journal_event(event)
        } else {
            String::new()
        };
        let node = Self::event_node(event);
        self.bump_journal_len();

        match event {
            JournalEvent::SpanStart { id, .. } => {
                self.span_stack.push(*id);
                self.add_record(StepEvent::JournalEvent { event: event_name }, node);
            }
            JournalEvent::SpanEnd(id) => {
                self.add_record(StepEvent::JournalEvent { event: event_name }, node);
                let popped = self
                    .span_stack
                    .pop()
                    .expect("SpanEnd requires an open recording span");
                assert_eq!(popped, *id, "recording span stack must be balanced");
            }
            _ => {
                self.add_record(StepEvent::JournalEvent { event: event_name }, node);
            }
        }
    }

    fn trace_effect_suppressed(&mut self, opcode: EffectKind, payload: usize) {
        let effect = if self.keeps_records() {
            self.format_opcode(opcode, payload)
        } else {
            String::new()
        };
        self.add_record(StepEvent::SuppressedEffect { effect }, None);
    }

    fn trace_suppress_control(&mut self, opcode: EffectKind, suppressed: bool) {
        let effect = if self.keeps_records() {
            self.format_opcode(opcode, 0)
        } else {
            String::new()
        };
        let event = if suppressed {
            StepEvent::SuppressedEffect { effect }
        } else {
            StepEvent::Effect { effect }
        };
        self.add_record(event, None);
    }

    fn trace_call(&mut self, target_ip: CodeAddr) {
        self.add_record(
            StepEvent::Call {
                target: target_ip.get(),
            },
            None,
        );
    }

    fn trace_return(&mut self, _outcome: plotnik_rt::ReturnOutcome) {
        self.add_record(StepEvent::Return, None);
    }

    fn trace_checkpoint_created(&mut self, ip: CodeAddr) {
        let record_idx = self.add_record_at(ip, StepEvent::CheckpointNew, None);
        self.shadow.push(Shadow {
            span_depth: self.span_stack.len(),
            journal_len: self.journal_len,
            record_idx,
        });
    }

    fn trace_backtrack(&mut self, _depth: u32) {
        let shadow = self
            .shadow
            .pop()
            .expect("trace_backtrack requires a matching checkpoint");
        self.span_stack.truncate(shadow.span_depth);
        // The VM truncates its match journal to the checkpoint's watermark on
        // restore; mirror that so `journal_len` keeps indexing the real journal.
        self.journal_len = shadow.journal_len;
        self.add_record(
            StepEvent::Backtrack {
                to_step: shadow.record_idx,
            },
            None,
        );
    }

    fn trace_enter_entrypoint(&mut self, target_ip: CodeAddr) {
        self.current_ip = target_ip;
        self.add_record(
            StepEvent::EnterEntrypoint {
                target: target_ip.get(),
            },
            None,
        );
    }
}

fn node_ref(node: Node<'_>) -> NodeRef {
    let range = node.byte_range();
    NodeRef {
        kind_id: node.kind_id(),
        start: u32::try_from(range.start).expect("node start byte fits in u32"),
        end: u32::try_from(range.end).expect("node end byte fits in u32"),
    }
}
