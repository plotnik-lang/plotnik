use std::io::{self, Write as _};

use tree_sitter::Node;

use crate::bytecode::{
    BYTECODE_WORD_SIZE, CodeAddr, EffectKind, Instruction, Module, ModuleRenderContext, Nav,
    SECTION_ALIGN, Symbol, cols, nav_symbol, return_content, trace, truncate_text,
};
use crate::core::{Colors, NodeFieldId};

use plotnik_runtime::{JournalEvent, PortId};

/// Verbosity level for trace output.
///
/// Controls which sub-lines are shown and whether node text is included.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Verbosity {
    /// Default: match, backtrack, call/return. Kind only, no text.
    #[default]
    Default,
    /// Verbose (-v): all sub-lines. Text on match/failure.
    Verbose,
    /// Very verbose (-vv): all sub-lines. Text on everything including nav.
    VeryVerbose,
}

/// Tracer trait for VM execution instrumentation.
///
/// All methods receive raw data (IDs, nodes) that the VM already has.
/// Formatting and name resolution happen in the tracer implementation.
pub trait Tracer {
    /// Compile-time switch: when `false`, the VM skips tracer calls entirely,
    /// including evaluation of their arguments (an eagerly-built `Node` costs a
    /// real FFI call even when the receiving method body is empty). Defaults to
    /// `true` so only tracers that opt out lose events.
    const ENABLED: bool = true;

    /// Called before executing an instruction.
    fn trace_instruction(&mut self, ip: CodeAddr, instr: &Instruction<'_>);

    /// Called after navigation succeeds.
    fn trace_nav(&mut self, nav: Nav, node: Node<'_>);

    /// Called when navigation fails (no child/sibling exists).
    fn trace_nav_failure(&mut self, nav: Nav);

    /// Called after type check succeeds.
    fn trace_match_success(&mut self, node: Node<'_>);

    /// Called after type check fails.
    fn trace_match_failure(&mut self, node: Node<'_>);

    /// Called after field check succeeds.
    fn trace_field_success(&mut self, field_id: NodeFieldId);

    /// Called after field check fails.
    fn trace_field_failure(&mut self, node: Node<'_>);

    /// Called when a candidate node fails the text predicate.
    fn trace_predicate_failure(&mut self, node: Node<'_>);

    /// Called when a candidate node fails a negated-field constraint.
    fn trace_neg_field_failure(&mut self, node: Node<'_>, field: NodeFieldId);

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
    fn trace_nav(&mut self, _nav: Nav, _node: Node<'_>) {}

    #[inline(always)]
    fn trace_nav_failure(&mut self, _nav: Nav) {}

    #[inline(always)]
    fn trace_match_success(&mut self, _node: Node<'_>) {}

    #[inline(always)]
    fn trace_match_failure(&mut self, _node: Node<'_>) {}

    #[inline(always)]
    fn trace_field_success(&mut self, _field_id: NodeFieldId) {}

    #[inline(always)]
    fn trace_field_failure(&mut self, _node: Node<'_>) {}

    #[inline(always)]
    fn trace_predicate_failure(&mut self, _node: Node<'_>) {}

    #[inline(always)]
    fn trace_neg_field_failure(&mut self, _node: Node<'_>, _field: NodeFieldId) {}

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
        }
    }
}

impl<'s> PrintTracer<'s> {
    pub fn builder<'m>(source: &'s str, module: &'m Module) -> PrintTracerBuilder<'s, 'm> {
        PrintTracerBuilder::new(source, module)
    }

    fn node_field_name(&self, id: NodeFieldId) -> &str {
        self.render.node_field_name(id).unwrap_or("?")
    }

    fn format_kind_simple(&self, kind: &str, is_named: bool) -> String {
        if is_named {
            kind.to_string()
        } else {
            let c = &self.colors;
            format!("{}{}{}{}", c.dim, c.green, kind, c.reset)
        }
    }

    fn format_kind_with_text(&self, kind: &str, text: &str, is_named: bool) -> String {
        let c = &self.colors;

        let available = cols::TOTAL_WIDTH - (cols::INDENT + cols::GAP + cols::SYMBOL + cols::GAP);

        if is_named {
            let text_budget = available.saturating_sub(kind.len() + 1).max(12);
            let truncated = truncate_text(text, text_budget);
            format!("{} {}{}{}{}", kind, c.dim, c.green, truncated, c.reset)
        } else {
            let truncated = truncate_text(text, available);
            format!("{}{}{}{}", c.dim, c.green, truncated, c.reset)
        }
    }

    fn trace_match_result(&mut self, symbol: Symbol, node: Node<'_>) {
        let kind = node.kind();
        let content = if self.verbosity == Verbosity::Default {
            self.format_kind_simple(kind, node.is_named())
        } else {
            let text = node.utf8_text(self.source).unwrap_or("?");
            self.format_kind_with_text(kind, text, node.is_named())
        };
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
        self.check_cache_line_boundary(ip);

        if let Instruction::Return(_) = instr {
            self.deferred_return_ip = Some(ip);
            return;
        }

        self.lines.push(self.render.instruction_line(ip, instr));
    }

    fn trace_nav(&mut self, nav: Nav, node: Node<'_>) {
        if self.verbosity == Verbosity::Default {
            return;
        }

        let kind = node.kind();
        let symbol = nav_symbol(nav);

        if self.verbosity == Verbosity::VeryVerbose {
            let text = node.utf8_text(self.source).unwrap_or("?");
            let content = self.format_kind_with_text(kind, text, node.is_named());
            self.add_subline(symbol, &content);
        } else {
            let content = self.format_kind_simple(kind, node.is_named());
            self.add_subline(symbol, &content);
        }
    }

    fn trace_nav_failure(&mut self, nav: Nav) {
        if self.verbosity == Verbosity::Default {
            return;
        }

        let failed_nav = match nav {
            Nav::Stay | Nav::StayExact | Nav::Epsilon => "·".to_string(),
            _ => nav_symbol(nav).format().trim().to_string(),
        };

        self.add_subline(trace::MATCH_FAILURE, &failed_nav);
    }

    fn trace_match_success(&mut self, node: Node<'_>) {
        self.trace_match_result(trace::MATCH_SUCCESS, node);
    }

    fn trace_match_failure(&mut self, node: Node<'_>) {
        self.trace_match_result(trace::MATCH_FAILURE, node);
    }

    fn trace_field_success(&mut self, field_id: NodeFieldId) {
        if self.verbosity == Verbosity::Default {
            return;
        }

        let name = self.node_field_name(field_id);
        self.add_subline(trace::MATCH_SUCCESS, &format!("{}:", name));
    }

    fn trace_field_failure(&mut self, _node: Node<'_>) {}

    fn trace_predicate_failure(&mut self, _node: Node<'_>) {
        if self.verbosity == Verbosity::Default {
            return;
        }

        self.add_subline(trace::MATCH_FAILURE, "✗ predicate");
    }

    fn trace_neg_field_failure(&mut self, _node: Node<'_>, field: NodeFieldId) {
        if self.verbosity == Verbosity::Default {
            return;
        }

        let name = self.node_field_name(field);
        self.add_subline(trace::MATCH_FAILURE, &format!("✗ -{}", name));
    }

    fn trace_journal_event(&mut self, event: &JournalEvent<'_>) {
        if self.verbosity == Verbosity::Default {
            return;
        }

        let effect_str = self.render.format_journal_event(event);
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
