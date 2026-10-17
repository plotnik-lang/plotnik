use std::fmt::Write as _;

use tree_sitter::Node;

use super::execution_trace::{TraceEvent, TraceRecorder};
use crate::bytecode::{CodeAddr, EffectKind, Instruction, Module, Nav};
use crate::core::{Colors, NodeFieldId};
use plotnik_runtime::{Account, JournalEvent, LimitExceeded, PortId, Text};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Verbosity {
    #[default]
    Default,
    Verbose,
    VeryVerbose,
}

/// VM observers run without the execution fuel permit. Built-in observers
/// use the operation account and report storage failures through `check`.
pub trait Tracer {
    const ENABLED: bool = true;
    fn start(&mut self, _account: &Account) {}
    fn check(&self) -> Result<(), LimitExceeded> {
        Ok(())
    }
    fn trace_instruction(&mut self, ip: CodeAddr, instr: &Instruction<'_>);
    fn trace_nav(&mut self, nav: Nav, node: Node<'_>);
    fn trace_nav_failure(&mut self, nav: Nav);
    fn trace_match_success(&mut self, node: Node<'_>);
    fn trace_match_failure(&mut self, node: Node<'_>);
    fn trace_field_success(&mut self, field_id: NodeFieldId);
    fn trace_field_failure(&mut self, node: Node<'_>);
    fn trace_predicate_failure(&mut self, node: Node<'_>);
    fn trace_neg_field_failure(&mut self, node: Node<'_>, field: NodeFieldId);
    fn trace_journal_event(&mut self, event: &JournalEvent<'_>);
    fn trace_effect_suppressed(&mut self, opcode: EffectKind, payload: usize);
    fn trace_suppress_control(&mut self, opcode: EffectKind, suppressed: bool);
    fn trace_call(&mut self, target_ip: CodeAddr);
    fn trace_return(&mut self, port: PortId);
    fn trace_checkpoint_created(&mut self, ip: CodeAddr);
    fn trace_backtrack(&mut self, depth: u32);
    fn trace_execution_started(&mut self, target_ip: CodeAddr);
}

pub struct NoopTracer;

impl Tracer for NoopTracer {
    const ENABLED: bool = false;
    fn trace_instruction(&mut self, _: CodeAddr, _: &Instruction<'_>) {}
    fn trace_nav(&mut self, _: Nav, _: Node<'_>) {}
    fn trace_nav_failure(&mut self, _: Nav) {}
    fn trace_match_success(&mut self, _: Node<'_>) {}
    fn trace_match_failure(&mut self, _: Node<'_>) {}
    fn trace_field_success(&mut self, _: NodeFieldId) {}
    fn trace_field_failure(&mut self, _: Node<'_>) {}
    fn trace_predicate_failure(&mut self, _: Node<'_>) {}
    fn trace_neg_field_failure(&mut self, _: Node<'_>, _: NodeFieldId) {}
    fn trace_journal_event(&mut self, _: &JournalEvent<'_>) {}
    fn trace_effect_suppressed(&mut self, _: EffectKind, _: usize) {}
    fn trace_suppress_control(&mut self, _: EffectKind, _: bool) {}
    fn trace_call(&mut self, _: CodeAddr) {}
    fn trace_return(&mut self, _: PortId) {}
    fn trace_checkpoint_created(&mut self, _: CodeAddr) {}
    fn trace_backtrack(&mut self, _: u32) {}
    fn trace_execution_started(&mut self, _: CodeAddr) {}
}

/// Text and structured tracing share flat records and checkpoint links.
pub struct PrintTracer<'s, 'm> {
    source: &'s str,
    module: &'m Module,
    recorder: TraceRecorder<'m>,
    verbosity: Verbosity,
    colors: Colors,
    account: Option<Account>,
}

pub struct PrintTracerBuilder<'s, 'm> {
    source: &'s str,
    module: &'m Module,
    verbosity: Verbosity,
    colors: Colors,
}

impl<'s, 'm> PrintTracerBuilder<'s, 'm> {
    pub fn new(source: &'s str, module: &'m Module) -> Self {
        Self {
            source,
            module,
            verbosity: Verbosity::Default,
            colors: Colors::OFF,
        }
    }
    pub fn verbosity(mut self, verbosity: Verbosity) -> Self {
        self.verbosity = verbosity;
        self
    }
    pub fn colored(mut self, enabled: bool) -> Self {
        self.colors = Colors::new(enabled);
        self
    }
    pub fn build(self) -> PrintTracer<'s, 'm> {
        PrintTracer {
            source: self.source,
            module: self.module,
            recorder: TraceRecorder::new(self.module, usize::MAX),
            verbosity: self.verbosity,
            colors: self.colors,
            account: None,
        }
    }
}

impl<'s, 'm> PrintTracer<'s, 'm> {
    pub fn builder(source: &'s str, module: &'m Module) -> PrintTracerBuilder<'s, 'm> {
        PrintTracerBuilder::new(source, module)
    }

    pub fn render(&self) -> Result<Text, LimitExceeded> {
        let account = self
            .account
            .as_ref()
            .expect("a text trace follows execution");
        let mut output = Text::new(account);
        let result: Result<(), LimitExceeded> = (|| {
            for record in self.recorder.records() {
                account.meter().charge(1)?;
                if self.verbosity == Verbosity::Default
                    && matches!(
                        record.event,
                        TraceEvent::Nav
                            | TraceEvent::NavFail
                            | TraceEvent::FieldOk
                            | TraceEvent::FieldFail
                            | TraceEvent::Effect { .. }
                            | TraceEvent::JournalEvent { .. }
                            | TraceEvent::SuppressedEffect { .. }
                            | TraceEvent::PredicateFail
                            | TraceEvent::NegFieldFail
                            | TraceEvent::CheckpointNew
                    )
                {
                    continue;
                }

                if !output.is_empty() {
                    output
                        .write_char('\n')
                        .map_err(|_| output.error().expect("text sink preserves its error"))?;
                }
                write!(
                    output,
                    "{}{:04}{} ",
                    self.colors.blue, record.ip, self.colors.reset
                )
                .map_err(|_| output.error().expect("text sink preserves its error"))?;
                let formatted = match &record.event {
                    TraceEvent::Instruction => {
                        match self.module.decode_instruction(CodeAddr::from(record.ip)) {
                            Instruction::Match(m) => {
                                write!(output, "{:?} {:?}", m.nav, m.node_kind)
                            }
                            Instruction::Call(call) => write!(output, "Call {:?}", call.target),
                            Instruction::Return(ret) => write!(output, "Return {:?}", ret.port),
                        }
                    }
                    TraceEvent::Effect { effect } => write!(output, "Effect {effect}"),
                    TraceEvent::JournalEvent { event } => write!(output, "Journal {event}"),
                    TraceEvent::SuppressedEffect { effect } => {
                        write!(output, "Suppressed {effect}")
                    }
                    event => write!(output, "{event:?}"),
                };
                formatted.map_err(|_| output.error().expect("text sink preserves its error"))?;
                if let Some(node) = record.node {
                    account
                        .meter()
                        .charge(self.module.header().node_kinds_count as u64)?;
                    let kind = self
                        .module
                        .node_kinds()
                        .iter()
                        .find(|kind| u16::from(kind.symbol()) == node.kind_id);
                    if let Some(kind) = kind {
                        write!(output, " {}", self.module.strings().get(kind.name()))
                            .map_err(|_| output.error().expect("text sink preserves its error"))?;
                    } else {
                        write!(output, " node#{}", node.kind_id)
                            .map_err(|_| output.error().expect("text sink preserves its error"))?;
                    }
                    write!(output, " [{}..{}]", node.start, node.end)
                        .map_err(|_| output.error().expect("text sink preserves its error"))?;
                    if self.verbosity == Verbosity::VeryVerbose
                        || self.verbosity == Verbosity::Verbose
                            && !matches!(record.event, TraceEvent::Nav)
                    {
                        let text = self
                            .source
                            .get(node.start as usize..node.end as usize)
                            .expect("trace nodes belong to source");
                        let end = text.char_indices().nth(96).map_or(text.len(), |(at, _)| at);
                        write!(output, " {:?}", &text[..end])
                            .map_err(|_| output.error().expect("text sink preserves its error"))?;
                    }
                }
            }
            Ok(())
        })();
        result?;
        Ok(output)
    }
}

macro_rules! forward {
    ($method:ident($($name:ident: $ty:ty),*)) => {
        fn $method(&mut self, $($name: $ty),*) { self.recorder.$method($($name),*); }
    };
}

impl Tracer for PrintTracer<'_, '_> {
    fn start(&mut self, account: &Account) {
        self.account = Some(account.clone());
        self.recorder.start(account);
    }
    fn check(&self) -> Result<(), LimitExceeded> {
        self.recorder.check()
    }
    forward!(trace_instruction(ip: CodeAddr, instr: &Instruction<'_>));
    forward!(trace_nav(nav: Nav, node: Node<'_>));
    forward!(trace_nav_failure(nav: Nav));
    forward!(trace_match_success(node: Node<'_>));
    forward!(trace_match_failure(node: Node<'_>));
    forward!(trace_field_success(field: NodeFieldId));
    forward!(trace_field_failure(node: Node<'_>));
    forward!(trace_predicate_failure(node: Node<'_>));
    forward!(trace_neg_field_failure(node: Node<'_>, field: NodeFieldId));
    forward!(trace_journal_event(event: &JournalEvent<'_>));
    forward!(trace_effect_suppressed(op: EffectKind, payload: usize));
    forward!(trace_suppress_control(op: EffectKind, suppressed: bool));
    forward!(trace_call(target: CodeAddr));
    forward!(trace_return(port: PortId));
    forward!(trace_checkpoint_created(ip: CodeAddr));
    forward!(trace_backtrack(depth: u32));
    forward!(trace_execution_started(target: CodeAddr));
}
