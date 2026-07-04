//! Tracing infrastructure for debugging VM execution.
//!
//! # Design: Zero-Cost Abstraction
//!
//! The tracer is designed as a zero-cost abstraction. When `NoopTracer` is used:
//! - All trait methods are `#[inline(always)]` empty functions
//! - The compiler eliminates all tracer calls and their arguments
//! - No tracing-related state exists in core execution structures
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

use arborium_tree_sitter::Node;

use crate::bytecode::{
    EffectKind, Instruction, LineBuilder, Match, Module, ModuleRenderContext, Nav, SECTION_ALIGN,
    STEP_SIZE, Symbol, cols, nav_symbol, trace, truncate_text, width_for_count,
};
use crate::core::{Colors, NodeFieldId};

use super::effect::RuntimeEffect;

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
    /// Called before executing an instruction.
    fn trace_instruction(&mut self, ip: u16, instr: &Instruction<'_>);

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

    /// Called after emitting an effect.
    fn trace_effect(&mut self, effect: &RuntimeEffect<'_>);

    /// Called when an effect is suppressed (inside @_ capture).
    fn trace_effect_suppressed(&mut self, opcode: EffectKind, payload: usize);

    /// Called for SuppressBegin/SuppressEnd control effects.
    /// `suppressed` is true if already inside another suppress scope.
    fn trace_suppress_control(&mut self, opcode: EffectKind, suppressed: bool);

    /// Called when entering a definition via Call.
    fn trace_call(&mut self, target_ip: u16);

    /// Called when returning from a definition.
    fn trace_return(&mut self);

    /// Called when a checkpoint is created.
    fn trace_checkpoint_created(&mut self, ip: u16);

    /// Called when backtracking occurs.
    fn trace_backtrack(&mut self);

    /// Called when entering an entrypoint (for section labels).
    fn trace_enter_entrypoint(&mut self, target_ip: u16);
}

/// No-op tracer that gets optimized away completely.
pub struct NoopTracer;

impl Tracer for NoopTracer {
    #[inline(always)]
    fn trace_instruction(&mut self, _ip: u16, _instr: &Instruction<'_>) {}

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
    fn trace_effect(&mut self, _effect: &RuntimeEffect<'_>) {}

    #[inline(always)]
    fn trace_effect_suppressed(&mut self, _opcode: EffectKind, _payload: usize) {}

    #[inline(always)]
    fn trace_suppress_control(&mut self, _opcode: EffectKind, _suppressed: bool) {}

    #[inline(always)]
    fn trace_call(&mut self, _target_ip: u16) {}

    #[inline(always)]
    fn trace_return(&mut self) {}

    #[inline(always)]
    fn trace_checkpoint_created(&mut self, _ip: u16) {}

    #[inline(always)]
    fn trace_backtrack(&mut self) {}

    #[inline(always)]
    fn trace_enter_entrypoint(&mut self, _target_ip: u16) {}
}

pub struct PrintTracer<'s> {
    pub(crate) source: &'s [u8],
    pub(crate) verbosity: Verbosity,
    pub(crate) lines: Vec<String>,
    pub(crate) builder: LineBuilder,
    pub(crate) render: ModuleRenderContext,
    /// Parallel stack of checkpoint creation IPs (for backtrack display).
    pub(crate) checkpoint_creation_ips: Vec<u16>,
    pub(crate) call_stack: Vec<String>,
    pub(crate) deferred_return_ip: Option<u16>,
    pub(crate) step_width: usize,
    pub(crate) colors: Colors,
    pub(crate) prev_ip: Option<u16>,
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
        let step_width = width_for_count(header.transitions_count as usize);

        PrintTracer {
            source: self.source.as_bytes(),
            verbosity: self.verbosity,
            lines: Vec::new(),
            builder: LineBuilder::new(step_width),
            render: ModuleRenderContext::new(self.module),
            checkpoint_creation_ips: Vec::new(),
            call_stack: Vec::new(),
            deferred_return_ip: None,
            step_width,
            colors: self.colors,
            prev_ip: None,
        }
    }
}

enum TraceEffect {
    Node,
    ArrayOpen,
    Push,
    ArrayClose,
    StructOpen,
    StructClose,
    Set(u16),
    EnumOpen(u16),
    EnumClose,
    Null,
}

impl TraceEffect {
    fn from_runtime(effect: &RuntimeEffect<'_>) -> Self {
        match effect {
            RuntimeEffect::Node(_) => Self::Node,
            RuntimeEffect::ArrayOpen => Self::ArrayOpen,
            RuntimeEffect::Push => Self::Push,
            RuntimeEffect::ArrayClose => Self::ArrayClose,
            RuntimeEffect::StructOpen => Self::StructOpen,
            RuntimeEffect::StructClose => Self::StructClose,
            RuntimeEffect::Set(idx) => Self::Set(*idx),
            RuntimeEffect::EnumOpen(idx) => Self::EnumOpen(*idx),
            RuntimeEffect::EnumClose => Self::EnumClose,
            RuntimeEffect::Null => Self::Null,
        }
    }

    fn from_opcode(opcode: EffectKind, payload: usize) -> Self {
        match opcode {
            EffectKind::Node => Self::Node,
            EffectKind::ArrayOpen => Self::ArrayOpen,
            EffectKind::Push => Self::Push,
            EffectKind::ArrayClose => Self::ArrayClose,
            EffectKind::StructOpen => Self::StructOpen,
            EffectKind::StructClose => Self::StructClose,
            EffectKind::Set => Self::Set(payload as u16),
            EffectKind::EnumOpen => Self::EnumOpen(payload as u16),
            EffectKind::EnumClose => Self::EnumClose,
            EffectKind::Null => Self::Null,
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

    fn entrypoint_name(&self, ip: u16) -> &str {
        self.render.entrypoint_name(ip).unwrap_or("?")
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

        // Available content width = TOTAL_WIDTH - prefix_width + step_width.
        // prefix_width = INDENT + step_width + GAP + SYMBOL + GAP; the +step_width
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
            TraceEffect::ArrayOpen => "ArrayOpen".to_string(),
            TraceEffect::Push => "Push".to_string(),
            TraceEffect::ArrayClose => "ArrayClose".to_string(),
            TraceEffect::StructOpen => "StructOpen".to_string(),
            TraceEffect::StructClose => "StructClose".to_string(),
            TraceEffect::Set(idx) => format!("Set \"{}\"", self.member_name(idx)),
            TraceEffect::EnumOpen(idx) => format!("EnumOpen \"{}\"", self.member_name(idx)),
            TraceEffect::EnumClose => "EnumClose".to_string(),
            TraceEffect::Null => "Null".to_string(),
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

    fn add_instruction(&mut self, ip: u16, symbol: Symbol, content: &str, successors: &str) {
        let prefix = format!("  {:0sw$} {} ", ip, symbol.format(), sw = self.step_width);
        let line = self
            .builder
            .pad_successors(format!("{prefix}{content}"), successors);
        self.lines.push(line);
    }

    /// Add a sub-line (blank step area + symbol + content).
    fn add_subline(&mut self, symbol: Symbol, content: &str) {
        let step_area = cols::INDENT + self.step_width + cols::GAP;
        let prefix = format!("{:step_area$}{} ", "", symbol.format());
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
    /// Cache line = 64 bytes = 8 steps (each step is 8 bytes).
    /// Only shows separator in verbose modes (-v, -vv).
    fn check_cache_line_boundary(&mut self, ip: u16) {
        if self.verbosity == Verbosity::Default {
            self.prev_ip = Some(ip);
            return;
        }

        const STEPS_PER_CACHE_LINE: u16 = (SECTION_ALIGN / STEP_SIZE) as u16;
        if let Some(prev) = self.prev_ip
            && prev / STEPS_PER_CACHE_LINE != ip / STEPS_PER_CACHE_LINE
        {
            self.lines.push(self.format_cache_line_separator());
        }
        self.prev_ip = Some(ip);
    }
}

impl Tracer for PrintTracer<'_> {
    fn trace_instruction(&mut self, ip: u16, instr: &Instruction<'_>) {
        self.check_cache_line_boundary(ip);

        match instr {
            Instruction::Match(m) => {
                // Show ε for epsilon transitions, empty otherwise (nav shown in sublines)
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
                let name = self.entrypoint_name(u16::from(c.target));
                let content = self.format_def_ref(name);
                let successors = format!("{:02} : {:02}", u16::from(c.target), u16::from(c.next));
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

    fn trace_effect(&mut self, effect: &RuntimeEffect<'_>) {
        if self.verbosity == Verbosity::Default {
            return;
        }

        let effect_str = self.format_effect(TraceEffect::from_runtime(effect));
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

    fn trace_call(&mut self, target_ip: u16) {
        let name = self.entrypoint_name(target_ip).to_string();
        self.add_subline(trace::CALL, &self.format_def_ref(&name));
        self.push_def_header(&name);
        self.call_stack.push(name);
    }

    fn trace_return(&mut self) {
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
        let successor = if is_top_level { "◼" } else { "" };
        self.add_instruction(ip, trace::RETURN, &content, successor);
        if let Some(caller) = self.call_stack.last().cloned() {
            self.push_def_header(&caller);
        }
    }

    fn trace_checkpoint_created(&mut self, ip: u16) {
        self.checkpoint_creation_ips.push(ip);
    }

    fn trace_backtrack(&mut self) {
        let created_at = self
            .checkpoint_creation_ips
            .pop()
            .expect("backtrack without checkpoint");
        let line = format!(
            "  {:0sw$} {}",
            created_at,
            trace::BACKTRACK.format(),
            sw = self.step_width
        );
        self.lines.push(line);
    }

    fn trace_enter_entrypoint(&mut self, target_ip: u16) {
        let name = self.entrypoint_name(target_ip).to_string();
        self.push_def_header(&name);
        self.call_stack.push(name);
    }
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
