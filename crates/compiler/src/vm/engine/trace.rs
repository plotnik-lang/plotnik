use std::io::{self, Write as _};

use tree_sitter::Node;

use crate::bytecode::{
    BYTECODE_WORD_SIZE, CodeAddr, EffectKind, Instruction, Module, ModuleRenderContext, Nav,
    SECTION_ALIGN, Symbol, cols, pattern_constraints, return_content, trace, truncate_text,
};
use crate::core::{Colors, NodeFieldId};

use plotnik_runtime::{JournalEvent, PortId};

/// Verbosity level for trace output.
///
/// Controls which sub-lines are shown and whether node text is included.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Verbosity {
    /// Default: instructions and candidates, with candidate text only for predicate tests.
    #[default]
    Default,
    /// Verbose (-v): suppression sublines, cache-line separators, and truncated,
    /// quoted, escaped text for named-node candidates.
    Verbose,
    /// Very verbose (-vv): all applied journal effects with runtime values where
    /// available, and text without truncation.
    VeryVerbose,
}

/// Tracer trait for VM execution instrumentation.
///
/// All methods receive raw data (IDs, nodes) that the VM already has.
/// Formatting and name resolution happen in the tracer implementation.
/// Candidate callbacks receive the cursor's actual field, not the requested field.
pub trait Tracer {
    /// Compile-time switch: when `false`, the VM skips tracer calls entirely,
    /// including evaluation of their arguments (an eagerly-built `Node` costs a
    /// real FFI call even when the receiving method body is empty). Defaults to
    /// `true` so only tracers that opt out lose events.
    const ENABLED: bool = true;

    /// Called before executing an instruction.
    fn trace_instruction(&mut self, ip: CodeAddr, instr: &Instruction<'_>);

    /// Called after a switch selects a target or finds no matching kind.
    fn trace_switch(&mut self, ip: CodeAddr, node: Node<'_>, target: Option<CodeAddr>);

    /// Called after navigation succeeds.
    fn trace_nav(&mut self, nav: Nav, node: Node<'_>);

    /// Called when navigation fails (no child/sibling exists).
    fn trace_nav_failure(&mut self, nav: Nav);

    /// Called when a candidate passes all instruction constraints.
    fn trace_match_success(&mut self, node: Node<'_>, field: Option<NodeFieldId>);

    /// Called after type check fails.
    fn trace_match_failure(&mut self, node: Node<'_>, field: Option<NodeFieldId>);

    /// Called after field check succeeds.
    fn trace_field_success(&mut self, field_id: NodeFieldId);

    /// Called after field check fails.
    fn trace_field_failure(&mut self, node: Node<'_>, field: Option<NodeFieldId>);

    /// Called when a candidate node fails the text predicate.
    fn trace_predicate_failure(&mut self, node: Node<'_>, field: Option<NodeFieldId>);

    /// Called when a candidate node fails a negated-field constraint.
    fn trace_neg_field_failure(
        &mut self,
        node: Node<'_>,
        field: Option<NodeFieldId>,
        forbidden: NodeFieldId,
    );

    /// Called after appending a journal event.
    fn trace_journal_event(&mut self, event: &JournalEvent<'_>);

    /// Called when an effect is suppressed (inside @_ capture).
    fn trace_effect_suppressed(&mut self, opcode: EffectKind, payload: usize);

    /// Called for SuppressBegin/SuppressEnd control effects.
    /// `suppressed` is true if already inside another suppress scope.
    fn trace_suppress_control(&mut self, opcode: EffectKind, suppressed: bool);

    /// Called when entering a definition via Call.
    fn trace_call(&mut self, target_ip: CodeAddr);

    /// Called when returning to a continuation, or with no target when the entrypoint returns.
    fn trace_return(&mut self, port: PortId, target: Option<CodeAddr>);

    /// Called when a checkpoint is created.
    fn trace_checkpoint_created(&mut self, ip: CodeAddr);

    /// Called when backtracking occurs, with the call depth being restored to
    /// (the checkpoint's `recursion_depth`, before the cursor/frames are reset).
    fn trace_backtrack(&mut self, depth: u32);

    /// Called when execution starts at the selected entrypoint (for section labels).
    fn trace_execution_started(&mut self, target_ip: CodeAddr);
}

/// No-op tracer that gets optimized away completely.
pub struct NoopTracer;

impl Tracer for NoopTracer {
    const ENABLED: bool = false;

    #[inline(always)]
    fn trace_instruction(&mut self, _ip: CodeAddr, _instr: &Instruction<'_>) {}

    #[inline(always)]
    fn trace_switch(&mut self, _ip: CodeAddr, _node: Node<'_>, _target: Option<CodeAddr>) {}

    #[inline(always)]
    fn trace_nav(&mut self, _nav: Nav, _node: Node<'_>) {}

    #[inline(always)]
    fn trace_nav_failure(&mut self, _nav: Nav) {}

    #[inline(always)]
    fn trace_match_success(&mut self, _node: Node<'_>, _field: Option<NodeFieldId>) {}

    #[inline(always)]
    fn trace_match_failure(&mut self, _node: Node<'_>, _field: Option<NodeFieldId>) {}

    #[inline(always)]
    fn trace_field_success(&mut self, _field_id: NodeFieldId) {}

    #[inline(always)]
    fn trace_field_failure(&mut self, _node: Node<'_>, _field: Option<NodeFieldId>) {}

    #[inline(always)]
    fn trace_predicate_failure(&mut self, _node: Node<'_>, _field: Option<NodeFieldId>) {}

    #[inline(always)]
    fn trace_neg_field_failure(
        &mut self,
        _node: Node<'_>,
        _field: Option<NodeFieldId>,
        _forbidden: NodeFieldId,
    ) {
    }

    #[inline(always)]
    fn trace_journal_event(&mut self, _event: &JournalEvent<'_>) {}

    #[inline(always)]
    fn trace_effect_suppressed(&mut self, _opcode: EffectKind, _payload: usize) {}

    #[inline(always)]
    fn trace_suppress_control(&mut self, _opcode: EffectKind, _suppressed: bool) {}

    #[inline(always)]
    fn trace_call(&mut self, _target_ip: CodeAddr) {}

    #[inline(always)]
    fn trace_return(&mut self, _port: PortId, _target: Option<CodeAddr>) {}

    #[inline(always)]
    fn trace_checkpoint_created(&mut self, _ip: CodeAddr) {}

    #[inline(always)]
    fn trace_backtrack(&mut self, _depth: u32) {}

    #[inline(always)]
    fn trace_execution_started(&mut self, _target_ip: CodeAddr) {}
}

pub struct PrintTracer<'s> {
    pub(crate) source: &'s [u8],
    pub(crate) verbosity: Verbosity,
    pub(crate) lines: Vec<String>,
    pub(crate) render: ModuleRenderContext,
    pub(crate) checkpoints: Vec<TraceCheckpoint>,
    pub(crate) call_stack: Vec<String>,
    pub(crate) deferred_return_ip: Option<CodeAddr>,
    pub(crate) colors: Colors,
    pub(crate) prev_ip: Option<CodeAddr>,
    tested_field: Option<NodeFieldId>,
    tested_predicate: bool,
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

    pub fn build(self) -> PrintTracer<'s> {
        PrintTracer {
            source: self.source.as_bytes(),
            verbosity: self.verbosity,
            lines: Vec::new(),
            render: ModuleRenderContext::new(self.module, self.colors),
            checkpoints: Vec::new(),
            call_stack: Vec::new(),
            deferred_return_ip: None,
            colors: self.colors,
            prev_ip: None,
            tested_field: None,
            tested_predicate: false,
        }
    }
}

impl<'s> PrintTracer<'s> {
    pub fn builder<'m>(source: &'s str, module: &'m Module) -> PrintTracerBuilder<'s, 'm> {
        PrintTracerBuilder::new(source, module)
    }

    fn node_pattern(&self, node: Node<'_>) -> String {
        if node.is_named() {
            return format!("({})", node.kind());
        }

        format!("{:?}", node.kind())
    }

    fn node_text(&self, node: Node<'_>) -> String {
        format!(
            "{:?}",
            node.utf8_text(self.source)
                .expect("node addresses UTF-8 source")
        )
    }

    fn trace_match_result(
        &mut self,
        symbol: Symbol,
        node: Node<'_>,
        field: Option<NodeFieldId>,
        facts: Vec<String>,
    ) {
        let mut content = String::new();
        if self.tested_field.is_some()
            && let Some(field) = field
        {
            let language = node.language();
            let name = language
                .field_name_for_id(u16::from(field))
                .expect("cursor field belongs to the node language");
            content.push_str(name);
            content.push_str(": ");
        }
        content.push_str(&pattern_constraints(self.node_pattern(node), facts));
        if node.is_named() && (self.verbosity != Verbosity::Default || self.tested_predicate) {
            let quoted = self.node_text(node);
            let text = if self.verbosity == Verbosity::VeryVerbose {
                quoted
            } else {
                let available =
                    cols::TOTAL_WIDTH - (cols::INDENT + cols::GAP + cols::SYMBOL + cols::GAP);
                let budget = available
                    .saturating_sub(content.chars().count() + 1)
                    .max(12);
                format!(
                    "\"{}\"",
                    truncate_text(&quoted[1..quoted.len() - 1], budget - 2)
                )
            };
            content.push_str(&format!(
                " {}{}{}{}",
                self.colors.dim, self.colors.green, text, self.colors.reset
            ));
        }
        self.add_subline(symbol, &content);
    }

    pub fn print(&self) -> io::Result<()> {
        let mut stdout = io::stdout().lock();
        for line in &self.lines {
            writeln!(stdout, "{line}")?;
        }
        Ok(())
    }

    /// Render the accumulated trace as a single string — the buffer analogue of
    /// [`print`](Self::print).
    pub fn render(&self) -> String {
        self.lines.join("\n")
    }

    fn add_instruction(&mut self, ip: CodeAddr, symbol: Symbol, content: &str, successors: &str) {
        let line = self
            .render
            .builder
            .instruction(ip.get(), symbol, content, successors, &[]);
        self.lines.push(line);
    }

    fn add_subline(&mut self, symbol: Symbol, content: &str) {
        let addr_area = cols::INDENT + self.render.addr_width + cols::GAP;
        let prefix = format!("{:addr_area$}{} ", "", symbol.format());
        self.lines.push(format!("{prefix}{content}"));
    }

    fn format_def_ref(&self, name: &str) -> String {
        let c = self.colors;
        if name.starts_with('_') {
            format!("{}{}{}", c.blue, name, c.reset)
        } else {
            format!("({}{}{})", c.blue, name, c.reset)
        }
    }

    fn format_def_header(&self, name: &str) -> String {
        let c = self.colors;
        format!("{}{}{}:", c.blue, name, c.reset)
    }

    fn push_def_header(&mut self, name: &str) {
        if !self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.lines.push(self.format_def_header(name));
    }

    fn format_cache_line_separator(&self) -> String {
        let c = self.colors;
        format!(
            "{:indent$}{}{}{}",
            "",
            c.dim,
            "-".repeat(cols::TOTAL_WIDTH),
            c.reset,
            indent = cols::INDENT,
        )
    }

    fn check_cache_line_boundary(&mut self, ip: CodeAddr) {
        if self.verbosity == Verbosity::Default {
            self.prev_ip = Some(ip);
            return;
        }

        const WORDS_PER_CACHE_LINE: u16 = (SECTION_ALIGN / BYTECODE_WORD_SIZE) as u16;
        if let Some(prev) = self.prev_ip
            && prev.get() / WORDS_PER_CACHE_LINE != ip.get() / WORDS_PER_CACHE_LINE
        {
            self.lines.push(self.format_cache_line_separator());
        }
        self.prev_ip = Some(ip);
    }
}

impl Tracer for PrintTracer<'_> {
    fn trace_instruction(&mut self, ip: CodeAddr, instr: &Instruction<'_>) {
        match instr {
            Instruction::Switch { .. } => return,
            Instruction::Match(m) => {
                self.tested_field = m.node_field;
                self.tested_predicate = m.predicate().is_some();
            }
            Instruction::Call(c) => {
                self.tested_field = c.node_field;
                self.tested_predicate = false;
            }
            Instruction::Return(_) => {
                self.deferred_return_ip = Some(ip);
                self.check_cache_line_boundary(ip);
                return;
            }
        }
        self.check_cache_line_boundary(ip);
        self.lines.push(self.render.instruction_line(ip, instr));
    }

    fn trace_switch(&mut self, ip: CodeAddr, node: Node<'_>, target: Option<CodeAddr>) {
        self.check_cache_line_boundary(ip);
        let symbol = if target.is_some() {
            trace::MATCH_SUCCESS
        } else {
            trace::MATCH_FAILURE
        };
        let successors = target
            .map(|addr| self.render.format_addr(addr))
            .unwrap_or_default();
        self.add_instruction(
            ip,
            symbol,
            &format!("switch {}", self.node_pattern(node)),
            &successors,
        );
    }

    fn trace_nav(&mut self, _nav: Nav, _node: Node<'_>) {}

    fn trace_nav_failure(&mut self, _nav: Nav) {
        self.add_subline(trace::MATCH_FAILURE, "no candidate");
    }

    fn trace_match_success(&mut self, node: Node<'_>, field: Option<NodeFieldId>) {
        self.trace_match_result(trace::MATCH_SUCCESS, node, field, Vec::new());
    }

    fn trace_match_failure(&mut self, node: Node<'_>, field: Option<NodeFieldId>) {
        self.trace_match_result(trace::MATCH_FAILURE, node, field, Vec::new());
    }

    fn trace_field_success(&mut self, _field_id: NodeFieldId) {}

    fn trace_field_failure(&mut self, node: Node<'_>, field: Option<NodeFieldId>) {
        self.trace_match_result(trace::MATCH_FAILURE, node, field, Vec::new());
    }

    fn trace_predicate_failure(&mut self, node: Node<'_>, field: Option<NodeFieldId>) {
        self.trace_match_result(trace::MATCH_FAILURE, node, field, Vec::new());
    }

    fn trace_neg_field_failure(
        &mut self,
        node: Node<'_>,
        field: Option<NodeFieldId>,
        forbidden: NodeFieldId,
    ) {
        let fact = format!("{}: _", self.render.dump_node_field_name(forbidden));
        self.trace_match_result(trace::MATCH_FAILURE, node, field, vec![fact]);
    }

    fn trace_journal_event(&mut self, event: &JournalEvent<'_>) {
        if self.verbosity != Verbosity::VeryVerbose {
            return;
        }

        let mut effect_str = self.render.format_journal_event(event);
        match event {
            JournalEvent::Node(node)
            | JournalEvent::NodeText(node)
            | JournalEvent::ScalarMark(node)
            | JournalEvent::SpanStart {
                node: Some(node), ..
            } => {
                effect_str.push_str(&format!(" {}", self.node_pattern(*node)));
                if node.is_named() {
                    effect_str.push_str(&format!(" {}", self.node_text(*node)));
                }
            }
            JournalEvent::NodeBool(_) => effect_str.push_str(" true"),
            JournalEvent::BoolClose(value) => effect_str = format!("BoolClose {value}"),
            JournalEvent::BoolValue(value) => effect_str = format!("BoolValue {value}"),
            _ => {}
        }
        self.add_subline(trace::EFFECT, &effect_str);
    }

    fn trace_effect_suppressed(&mut self, opcode: EffectKind, payload: usize) {
        if self.verbosity == Verbosity::Default {
            return;
        }

        let effect_str = self.render.format_effect(opcode, payload);
        self.add_subline(trace::EFFECT_SUPPRESSED, &effect_str);
    }

    fn trace_suppress_control(&mut self, opcode: EffectKind, suppressed: bool) {
        if self.verbosity == Verbosity::Default {
            return;
        }

        let name = self.render.format_effect(opcode, 0);
        let symbol = if suppressed {
            trace::EFFECT_SUPPRESSED
        } else {
            trace::EFFECT
        };
        self.add_subline(symbol, &name);
    }

    fn trace_call(&mut self, target_ip: CodeAddr) {
        let name = self.render.def_ref_name(target_ip);
        self.add_subline(trace::CALL, &self.format_def_ref(&name));
        self.push_def_header(&name);
        self.call_stack.push(name);
    }

    fn trace_return(&mut self, port: PortId, target: Option<CodeAddr>) {
        let ip = self
            .deferred_return_ip
            .take()
            .expect("trace_return without trace_instruction");
        self.call_stack
            .pop()
            .expect("trace_return requires balanced call stack");
        let successor = target
            .map(|addr| format!("{:0w$}", addr, w = self.render.addr_width))
            .unwrap_or_else(|| "◼".to_string());
        self.add_instruction(ip, trace::RETURN, &return_content(port), &successor);
        if let Some(caller) = self.call_stack.last().cloned() {
            self.push_def_header(&caller);
        }
    }

    fn trace_checkpoint_created(&mut self, ip: CodeAddr) {
        self.checkpoints.push(TraceCheckpoint {
            ip,
            call_stack: self.call_stack.clone(),
        });
    }

    fn trace_backtrack(&mut self, depth: u32) {
        let checkpoint = self
            .checkpoints
            .pop()
            .expect("backtrack without checkpoint");
        self.call_stack = checkpoint.call_stack;
        assert_eq!(
            self.call_stack.len(),
            depth as usize + 1,
            "execution trace restored a call stack whose length disagrees with the runtime \
             checkpoint depth"
        );
        let line = format!(
            "  {:0aw$} {}",
            checkpoint.ip,
            trace::BACKTRACK.format(),
            aw = self.render.addr_width
        );
        self.lines.push(line.trim_end().to_string());
    }

    fn trace_execution_started(&mut self, target_ip: CodeAddr) {
        self.checkpoints.clear();
        self.call_stack.clear();
        self.deferred_return_ip = None;
        self.prev_ip = None;
        let name = self.render.def_ref_name(target_ip);
        self.push_def_header(&name);
        self.call_stack.push(name);
    }
}

#[derive(Clone)]
pub(crate) struct TraceCheckpoint {
    ip: CodeAddr,
    call_stack: Vec<String>,
}
