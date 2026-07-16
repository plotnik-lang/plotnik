//! Tracing infrastructure for debugging VM execution.
//!
//! # Design: Zero-Cost Abstraction
//!
//! VM call sites are gated on `T::ENABLED`. `NoopTracer` sets that constant to
//! `false`, so tracing work is removed before argument evaluation; this matters
//! for arguments built from tree-sitter FFI calls such as `cursor.node()`.
//! Tracing state still lives entirely outside core execution structures.
//!
//! # Design: Tracer-Owned State
//!
//! Tracing-only state (like checkpoint creation IPs for backtrack display) is
//! maintained by the tracer itself, not in core structures like `Checkpoint`.
//! This keeps execution structures minimal and avoids "spilling" tracing concerns
//! into `exec`. For example:
//! - `trace_checkpoint_created(ip)` - tracer pushes to its own stack
//! - `trace_backtrack()` - tracer pops its stack to get the display IP
//!
//! `NoopTracer` ignores these calls (optimized away), while `PrintTracer`
//! maintains parallel state for display purposes.

use tree_sitter::Node;

use crate::bytecode::{
    BYTECODE_WORD_SIZE, CodeAddr, EffectKind, Instruction, LineBuilder, Match, Module,
    ModuleRenderContext, Nav, SECTION_ALIGN, Symbol, cols, nav_symbol, trace, truncate_text,
    width_for_count,
};
use crate::core::{Colors, NodeFieldId};

use plotnik_runtime::{JournalEvent, ReturnOutcome};

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

    /// Called when returning from a definition.
    fn trace_return(&mut self, outcome: ReturnOutcome);

    /// Called when a checkpoint is created.
    fn trace_checkpoint_created(&mut self, ip: CodeAddr);

    /// Called when backtracking occurs, with the call depth being restored to
    /// (the checkpoint's `recursion_depth`, before the cursor/frames are reset).
    fn trace_backtrack(&mut self, depth: u32);

    /// Called when entering an entry point (for section labels).
    fn trace_enter_entry_point(&mut self, target_ip: CodeAddr);
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
    fn trace_return(&mut self, _outcome: ReturnOutcome) {}

    #[inline(always)]
    fn trace_checkpoint_created(&mut self, _ip: CodeAddr) {}

    #[inline(always)]
    fn trace_backtrack(&mut self, _depth: u32) {}

    #[inline(always)]
    fn trace_enter_entry_point(&mut self, _target_ip: CodeAddr) {}
}

pub struct PrintTracer<'s> {
    pub(crate) source: &'s [u8],
    pub(crate) verbosity: Verbosity,
    pub(crate) lines: Vec<String>,
    pub(crate) builder: LineBuilder,
    pub(crate) render: ModuleRenderContext,
    /// Parallel stack of checkpoint creation IPs (for backtrack display).
    pub(crate) checkpoints: Vec<TraceCheckpoint>,
    pub(crate) call_stack: Vec<String>,
    pub(crate) deferred_return_ip: Option<CodeAddr>,
    pub(crate) addr_width: usize,
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
    /// Create a new builder with required parameters.
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

    /// Set whether to use colored output.
    pub fn colored(mut self, enabled: bool) -> Self {
        self.colors = Colors::new(enabled);
        self
    }

    /// Build the PrintTracer.
    pub fn build(self) -> PrintTracer<'s> {
        let header = self.module.header();
        let addr_width = width_for_count(header.instruction_word_count as usize);

        PrintTracer {
            source: self.source.as_bytes(),
            verbosity: self.verbosity,
            lines: Vec::new(),
            builder: LineBuilder::new(addr_width),
            render: ModuleRenderContext::new(self.module),
            checkpoints: Vec::new(),
            call_stack: Vec::new(),
            deferred_return_ip: None,
            addr_width,
            colors: self.colors,
            prev_ip: None,
        }
    }
}

enum TraceEffect {
    Node,
    ListOpen,
    ArrayPush,
    ListClose,
    RecordOpen,
    RecordClose,
    RecordSet(u16),
    VariantOpen(u16),
    VariantClose,
    Absent,
    SpanStartAt(u16),
    SpanStart(u16),
    SpanEnd(u16),
    ScalarOpen,
    ScalarMark,
    TextClose,
    BoolClose(bool),
    NodeText,
    NodeBool,
    BoolValue(bool),
}

impl TraceEffect {
    fn from_journal(event: &JournalEvent<'_>) -> Self {
        match event {
            JournalEvent::Node(_) => Self::Node,
            JournalEvent::ListOpen => Self::ListOpen,
            JournalEvent::ArrayPush => Self::ArrayPush,
            JournalEvent::ListClose => Self::ListClose,
            JournalEvent::RecordOpen => Self::RecordOpen,
            JournalEvent::RecordClose => Self::RecordClose,
            JournalEvent::RecordSet(idx) => Self::RecordSet(*idx),
            JournalEvent::VariantOpen(idx) => Self::VariantOpen(*idx),
            JournalEvent::VariantClose => Self::VariantClose,
            JournalEvent::Absent => Self::Absent,
            JournalEvent::SpanStart { id, node } => {
                if node.is_some() {
                    Self::SpanStartAt(*id)
                } else {
                    Self::SpanStart(*id)
                }
            }
            JournalEvent::SpanEnd(id) => Self::SpanEnd(*id),
            JournalEvent::ScalarOpen => Self::ScalarOpen,
            JournalEvent::ScalarMark(_) => Self::ScalarMark,
            JournalEvent::TextClose => Self::TextClose,
            JournalEvent::BoolClose(value) => Self::BoolClose(*value),
            JournalEvent::NodeText(_) => Self::NodeText,
            JournalEvent::NodeBool(_) => Self::NodeBool,
            JournalEvent::BoolValue(value) => Self::BoolValue(*value),
        }
    }

    fn from_opcode(opcode: EffectKind, payload: usize) -> Self {
        match opcode {
            EffectKind::Node => Self::Node,
            EffectKind::ListOpen => Self::ListOpen,
            EffectKind::ArrayPush => Self::ArrayPush,
            EffectKind::ListClose => Self::ListClose,
            EffectKind::RecordOpen => Self::RecordOpen,
            EffectKind::RecordClose => Self::RecordClose,
            EffectKind::RecordSet => Self::RecordSet(payload as u16),
            EffectKind::VariantOpen => Self::VariantOpen(payload as u16),
            EffectKind::VariantClose => Self::VariantClose,
            EffectKind::Absent => Self::Absent,
            EffectKind::SpanStartAt => Self::SpanStartAt(payload as u16),
            EffectKind::SpanStart => Self::SpanStart(payload as u16),
            EffectKind::SpanEnd => Self::SpanEnd(payload as u16),
            EffectKind::ScalarOpen => Self::ScalarOpen,
            EffectKind::ScalarMark => Self::ScalarMark,
            EffectKind::TextClose => Self::TextClose,
            EffectKind::BoolClose => Self::BoolClose(payload != 0),
            EffectKind::NodeText => Self::NodeText,
            EffectKind::NodeBool => Self::NodeBool,
            EffectKind::BoolValue => Self::BoolValue(payload != 0),
            EffectKind::SuppressBegin | EffectKind::SuppressEnd => unreachable!(),
        }
    }
}

impl<'s> PrintTracer<'s> {
    /// Create a builder for PrintTracer.
    pub fn builder<'m>(source: &'s str, module: &'m Module) -> PrintTracerBuilder<'s, 'm> {
        PrintTracerBuilder::new(source, module)
    }

    fn node_field_name(&self, id: NodeFieldId) -> &str {
        self.render.node_field_name(id).unwrap_or("?")
    }

    fn member_name(&self, idx: u16) -> &str {
        self.render.member_name(idx).unwrap_or("?")
    }

    /// The definition name for a call or entry target. Only entry points carry a
    /// name; a call into an internal body (or an entry that isn't a named
    /// definition) falls back to its address `@{ip}`, exactly as the bytecode
    /// dump renders the same target.
    fn def_ref_name(&self, ip: CodeAddr) -> String {
        self.render
            .entry_point_name(ip.get())
            .map(str::to_string)
            .unwrap_or_else(|| format!("@{:0w$}", ip, w = self.addr_width))
    }

    /// Format kind without text content.
    ///
    /// - Named nodes: `kind` (e.g., `identifier`)
    /// - Anonymous nodes: `kind` dim green (e.g., `let`)
    fn format_kind_simple(&self, kind: &str, is_named: bool) -> String {
        if is_named {
            kind.to_string()
        } else {
            let c = &self.colors;
            format!("{}{}{}{}", c.dim, c.green, kind, c.reset)
        }
    }

    /// Format kind with source text, dynamically truncated to fit content width.
    ///
    /// - Named nodes: `kind text` (e.g., `identifier fetchData`)
    /// - Anonymous nodes: just `text` in green (kind == text, no redundancy)
    fn format_kind_with_text(&self, kind: &str, text: &str, is_named: bool) -> String {
        let c = &self.colors;

        // Available content width = TOTAL_WIDTH - prefix_width + addr_width.
        // prefix_width = INDENT + addr_width + GAP + SYMBOL + GAP; the +addr_width
        // cancels because the ellipsis can extend into the successors column
        // (sub-lines have no successors, so we reuse that space).
        let available = cols::TOTAL_WIDTH - (cols::INDENT + cols::GAP + cols::SYMBOL + cols::GAP);

        if is_named {
            let text_budget = available.saturating_sub(kind.len() + 1).max(12);
            let truncated = truncate_text(text, text_budget);
            format!("{} {}{}{}{}", kind, c.dim, c.green, truncated, c.reset)
        } else {
            // Anonymous: kind == text, so showing kind would duplicate.
            let truncated = truncate_text(text, available);
            format!("{}{}{}{}", c.dim, c.green, truncated, c.reset)
        }
    }

    fn format_effect(&self, effect: TraceEffect) -> String {
        match effect {
            TraceEffect::Node => "Node".to_string(),
            TraceEffect::ListOpen => "ListOpen".to_string(),
            TraceEffect::ArrayPush => "ArrayPush".to_string(),
            TraceEffect::ListClose => "ListClose".to_string(),
            TraceEffect::RecordOpen => "RecordOpen".to_string(),
            TraceEffect::RecordClose => "RecordClose".to_string(),
            TraceEffect::RecordSet(idx) => {
                format!("RecordSet \"{}\"", self.member_name(idx))
            }
            TraceEffect::VariantOpen(idx) => format!("VariantOpen \"{}\"", self.member_name(idx)),
            TraceEffect::VariantClose => "VariantClose".to_string(),
            TraceEffect::Absent => "Absent".to_string(),
            TraceEffect::SpanStartAt(id) => format!("SpanStartAt#{id}"),
            TraceEffect::SpanStart(id) => format!("SpanStart#{id}"),
            TraceEffect::SpanEnd(id) => format!("SpanEnd#{id}"),
            TraceEffect::ScalarOpen => "ScalarOpen".to_string(),
            TraceEffect::ScalarMark => "ScalarMark".to_string(),
            TraceEffect::TextClose => "TextClose".to_string(),
            TraceEffect::BoolClose(value) => format!("BoolClose({value})"),
            TraceEffect::NodeText => "NodeText".to_string(),
            TraceEffect::NodeBool => "NodeBool".to_string(),
            TraceEffect::BoolValue(value) => format!("BoolValue({value})"),
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

    /// Format match content for instruction line (matches dump format exactly).
    ///
    /// Order: field/type/predicate content, one effects group, then successors.
    fn format_match_content(&self, m: &Match<'_>) -> String {
        self.render.trace_match_content(m)
    }

    pub fn print(&self) {
        for line in &self.lines {
            println!("{}", line);
        }
    }

    /// Render the accumulated trace as a single string — the buffer analogue of
    /// [`print`](Self::print).
    pub fn render(&self) -> String {
        self.lines.join("\n")
    }

    fn add_instruction(&mut self, ip: CodeAddr, symbol: Symbol, content: &str, successors: &str) {
        let prefix = format!("  {:0aw$} {} ", ip, symbol.format(), aw = self.addr_width);
        let line = self
            .builder
            .pad_successors(format!("{prefix}{content}"), successors);
        self.lines.push(line);
    }

    /// Add a sub-line (blank address area + symbol + content).
    fn add_subline(&mut self, symbol: Symbol, content: &str) {
        let addr_area = cols::INDENT + self.addr_width + cols::GAP;
        let prefix = format!("{:addr_area$}{} ", "", symbol.format());
        self.lines.push(format!("{prefix}{content}"));
    }

    /// Format definition name (blue). User definitions get parentheses.
    fn format_def_ref(&self, name: &str) -> String {
        let c = self.colors;
        if name.starts_with('_') {
            // Internal labels: no parentheses.
            format!("{}{}{}", c.blue, name, c.reset)
        } else {
            // User definitions: wrap in parentheses
            format!("({}{}{})", c.blue, name, c.reset)
        }
    }

    fn format_def_header(&self, name: &str) -> String {
        let c = self.colors;
        format!("{}{}{}:", c.blue, name, c.reset)
    }

    /// Push a definition header, with empty line separator (except for first header).
    fn push_def_header(&mut self, name: &str) {
        if !self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.lines.push(self.format_def_header(name));
    }

    fn format_cache_line_separator(&self) -> String {
        // Content spans TOTAL_WIDTH (same as instruction lines before successors)
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

    /// Check if IPs cross a cache line boundary and insert separator if so.
    ///
    /// Cache line = 64 bytes = 8 bytecode words.
    /// Only shows separator in verbose modes (-v, -vv).
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

        match instr {
            Instruction::Match(m) => {
                // Show ε for epsilon instructions, empty otherwise (nav shown in sublines)
                let symbol = if m.is_epsilon() {
                    Symbol::EPSILON
                } else {
                    Symbol::EMPTY
                };
                let content = self.format_match_content(m);
                let successors = format_match_successors(m);
                self.add_instruction(ip, symbol, &content, &successors);
            }
            Instruction::Call(c) => {
                let name = self.def_ref_name(CodeAddr::from(u16::from(c.target)));
                let content = self.format_def_ref(&name);
                let successors = format!("{:02} : {:02}", u16::from(c.target), u16::from(c.next));
                self.add_instruction(ip, Symbol::EMPTY, &content, &successors);
            }
            Instruction::RoutedCall(c) => {
                let name = self.def_ref_name(CodeAddr::from(u16::from(c.target)));
                let content = self.format_def_ref(&name);
                let successors = format!("{:02} : {:02}", u16::from(c.target), u16::from(c.next));
                self.add_instruction(ip, Symbol::EMPTY, &content, &successors);
            }
            Instruction::SplitCall(c) => {
                let name = self.def_ref_name(CodeAddr::from(u16::from(c.target)));
                let content = self.format_def_ref(&name);
                let successors = format!(
                    "{:02} : {:02} / {:02}",
                    u16::from(c.target),
                    u16::from(c.returns.matched),
                    u16::from(c.returns.empty)
                );
                self.add_instruction(ip, Symbol::EMPTY, &content, &successors);
            }
            Instruction::Return(_) => {
                self.deferred_return_ip = Some(ip);
            }
        }
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

    fn trace_field_failure(&mut self, _node: Node<'_>) {
        // Field failures are silent - we just backtrack
    }

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

        let effect_str = self.format_effect(TraceEffect::from_journal(event));
        self.add_subline(trace::EFFECT, &effect_str);
    }

    fn trace_effect_suppressed(&mut self, opcode: EffectKind, payload: usize) {
        if self.verbosity == Verbosity::Default {
            return;
        }

        let effect_str = self.format_effect(TraceEffect::from_opcode(opcode, payload));
        self.add_subline(trace::EFFECT_SUPPRESSED, &effect_str);
    }

    fn trace_suppress_control(&mut self, opcode: EffectKind, suppressed: bool) {
        if self.verbosity == Verbosity::Default {
            return;
        }

        let name = match opcode {
            EffectKind::SuppressBegin => "SuppressBegin",
            EffectKind::SuppressEnd => "SuppressEnd",
            _ => unreachable!(),
        };
        let symbol = if suppressed {
            trace::EFFECT_SUPPRESSED
        } else {
            trace::EFFECT
        };
        self.add_subline(symbol, name);
    }

    fn trace_call(&mut self, target_ip: CodeAddr) {
        let name = self.def_ref_name(target_ip);
        self.add_subline(trace::CALL, &self.format_def_ref(&name));
        self.push_def_header(&name);
        self.call_stack.push(name);
    }

    fn trace_return(&mut self, outcome: ReturnOutcome) {
        let ip = self
            .deferred_return_ip
            .take()
            .expect("trace_return without trace_instruction");
        let name = self
            .call_stack
            .pop()
            .expect("trace_return requires balanced call stack");
        let content = self.format_def_ref(&name);
        // Show ◼ when returning from top-level (stack now empty)
        let is_top_level = self.call_stack.is_empty();
        let successor = match (is_top_level, outcome) {
            (true, ReturnOutcome::Matched) => "◼",
            (false, ReturnOutcome::Matched) => "",
            (false, ReturnOutcome::Empty) => "empty",
            (true, ReturnOutcome::Empty) => {
                unreachable!("entry point empty returns are rejected during module validation")
            }
        };
        self.add_instruction(ip, trace::RETURN, &content, successor);
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
        // Backtracking can restore a frame that already returned. Retain the
        // checkpoint's actual call path rather than only its depth, which can
        // shrink a trace stack but cannot reconstruct a popped callee name.
        self.call_stack = checkpoint.call_stack;
        debug_assert_eq!(self.call_stack.len(), depth as usize + 1);
        let line = format!(
            "  {:0aw$} {}",
            checkpoint.ip,
            trace::BACKTRACK.format(),
            aw = self.addr_width
        );
        self.lines.push(line);
    }

    fn trace_enter_entry_point(&mut self, target_ip: CodeAddr) {
        let name = self.def_ref_name(target_ip);
        self.push_def_header(&name);
        self.call_stack.push(name);
    }
}

#[derive(Clone)]
pub(crate) struct TraceCheckpoint {
    ip: CodeAddr,
    call_stack: Vec<String>,
}

fn format_match_successors(m: &Match<'_>) -> String {
    if m.is_terminal() {
        "◼".to_string()
    } else if m.succ_count() == 1 {
        format!("{:02}", u16::from(m.successor(0)))
    } else {
        let succs: Vec<_> = m
            .successors()
            .map(|s| format!("{:02}", u16::from(s)))
            .collect();
        succs.join(", ")
    }
}
