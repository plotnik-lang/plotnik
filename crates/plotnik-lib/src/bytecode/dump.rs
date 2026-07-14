//! Human-readable bytecode dump for debugging and documentation.
//!
//! See `docs/binary-format/08-dump-format.md` for the output format specification.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::core::Colors;

use super::format::{LineBuilder, Symbol, nav_symbol, width_for_count};
use super::ids::TypeId;
use super::instructions::{CodeAddr, SuccessorAddr};
use super::module::{Instruction, Module};
use super::node_kind_constraint::NodeKindConstraint;
use super::render::ModuleRenderContext;
use super::type_meta::{TypeDefKind, TypeKind};
use super::{Call, Match, Return, RoutedCall, SPAN_NO_BINDING, SplitCall};
use plotnik_rt::Nav;

/// Generate a human-readable dump of the bytecode module.
pub fn dump(module: &Module, colors: Colors) -> String {
    let mut out = String::new();
    let ctx = DumpContext::new(module, colors);

    dump_strings(&mut out, module, &ctx);
    dump_regexes(&mut out, module, &ctx);
    dump_types_defs(&mut out, module, &ctx);
    dump_types_members(&mut out, module, &ctx);
    dump_types_names(&mut out, module, &ctx);
    dump_entry_points(&mut out, module, &ctx);
    dump_spans(&mut out, module, &ctx);
    dump_code(&mut out, module, &ctx);

    out
}

/// Context for dump formatting, precomputes lookups for O(1) access.
struct DumpContext {
    /// Maps instruction addresses to entry point names for labeling.
    addr_labels: BTreeMap<CodeAddr, String>,
    /// Shared symbol/string decoding and match rendering.
    render: ModuleRenderContext,
    /// Width for string indices (S#).
    str_width: usize,
    /// Width for type indices (T#).
    type_width: usize,
    /// Width for member indices (M#).
    member_width: usize,
    /// Width for name indices (N#).
    name_width: usize,
    /// Width for instruction addresses.
    addr_width: usize,
    /// Color palette.
    colors: Colors,
}

impl DumpContext {
    fn new(module: &Module, colors: Colors) -> Self {
        let header = module.header();
        let strings = module.strings();
        let entry_points = module.entry_points();

        let mut addr_labels = BTreeMap::new();
        for ep in entry_points.iter() {
            let name = strings.get(ep.name()).to_string();
            addr_labels.insert(ep.target(), name);
        }

        let str_count = header.str_table_count as usize;

        let types = module.types();
        // `defs_count` already includes every emitted builtin and custom type.
        let type_count = types.defs_count();
        let str_width = width_for_count(str_count);
        let type_width = width_for_count(type_count);
        let member_width = width_for_count(types.members_count());
        let name_width = width_for_count(types.names_count());
        let addr_width = width_for_count(header.instruction_word_count as usize);

        Self {
            addr_labels,
            render: ModuleRenderContext::new(module),
            str_width,
            type_width,
            member_width,
            name_width,
            addr_width,
            colors,
        }
    }

    fn label_for(&self, addr: SuccessorAddr) -> Option<&str> {
        self.addr_labels
            .get(&CodeAddr::from(u16::from(addr)))
            .map(|s| s.as_str())
    }
}

fn dump_strings(out: &mut String, module: &Module, ctx: &DumpContext) {
    let c = &ctx.colors;
    let strings = module.strings();
    let count = module.header().str_table_count as usize;
    let w = ctx.str_width;

    writeln!(out, "{}[strings]{}", c.blue, c.reset).expect("writing to a String is infallible");
    for i in 0..count {
        let s = strings.at(i);
        writeln!(out, "S{i:0w$} {}{s:?}{}", c.green, c.reset)
            .expect("writing to a String is infallible");
    }
    out.push('\n');
}

fn dump_regexes(out: &mut String, module: &Module, ctx: &DumpContext) {
    let count = module.header().regex_table_count as usize;
    // Index 0 is reserved, so only print if there are actual regexes
    if count <= 1 {
        return;
    }

    let c = &ctx.colors;
    let regexes = module.regexes();
    let w = width_for_count(count);

    writeln!(out, "{}[regex]{}", c.blue, c.reset).expect("writing to a String is infallible");
    for i in 1..count {
        let string_id = regexes.pattern_string_id(i);
        let pattern = ctx.render.string(u16::from(string_id) as usize);
        writeln!(out, "R{i:0w$} {}/{pattern}/{}", c.green, c.reset)
            .expect("writing to a String is infallible");
    }
    out.push('\n');
}

fn dump_types_defs(out: &mut String, module: &Module, ctx: &DumpContext) {
    let c = &ctx.colors;
    let types = module.types();
    let strings = module.strings();
    let tw = ctx.type_width;
    let mw = ctx.member_width;

    writeln!(out, "{}[type_defs]{}", c.blue, c.reset).expect("writing to a String is infallible");

    // type_defs holds every type, builtins included.
    for (i, def) in types.iter().enumerate() {
        let (formatted, comment) = match def.decode() {
            TypeDefKind::Primitive(kind) => {
                let name = match kind {
                    TypeKind::NoValue => "<NoValue>",
                    TypeKind::Node => "<Node>",
                    TypeKind::Text => "<Text>",
                    TypeKind::Bool => "<Bool>",
                    _ => unreachable!(),
                };
                (name.to_string(), String::new())
            }
            TypeDefKind::Wrapper { kind, inner } => {
                let formatted = match kind {
                    TypeKind::Option => format!("Option(T{:0tw$})", u16::from(inner)),
                    TypeKind::ListZeroOrMore => {
                        format!("ListZeroOrMore(T{:0tw$})", u16::from(inner))
                    }
                    TypeKind::ListOneOrMore => {
                        format!("ListOneOrMore(T{:0tw$})", u16::from(inner))
                    }
                    TypeKind::Alias => format!("Alias(T{:0tw$})", u16::from(inner)),
                    _ => unreachable!(),
                };
                let comment = match kind {
                    TypeKind::Option => {
                        let inner_name = format_type_name(inner, module, ctx);
                        format!("{}  ; {}?{}", c.dim, inner_name, c.reset)
                    }
                    TypeKind::ListZeroOrMore => {
                        let inner_name = format_type_name(inner, module, ctx);
                        format!("{}  ; {}*{}", c.dim, inner_name, c.reset)
                    }
                    TypeKind::ListOneOrMore => {
                        let inner_name = format_type_name(inner, module, ctx);
                        format!("{}  ; {}+{}", c.dim, inner_name, c.reset)
                    }
                    TypeKind::Alias => String::new(),
                    _ => unreachable!(),
                };
                (formatted, comment)
            }
            TypeDefKind::Record {
                member_start,
                member_count,
            } => {
                let formatted = format!("Record  M{:0mw$}:{}", member_start, member_count);
                let fields: Vec<_> = types
                    .members_of(&def)
                    .map(|m| strings.get(m.name_id).to_string())
                    .collect();
                let comment = format!("{}  ; {{ {} }}{}", c.dim, fields.join(", "), c.reset);
                (formatted, comment)
            }
            TypeDefKind::Variant {
                member_start,
                member_count,
            } => {
                let formatted = format!("Variant M{:0mw$}:{}", member_start, member_count);
                let cases: Vec<_> = types
                    .members_of(&def)
                    .map(|m| strings.get(m.name_id).to_string())
                    .collect();
                let comment = format!("{}  ; {}{}", c.dim, cases.join(" | "), c.reset);
                (formatted, comment)
            }
        };

        writeln!(out, "T{i:0tw$} = {formatted}{comment}")
            .expect("writing to a String is infallible");
    }
    out.push('\n');
}

fn dump_types_members(out: &mut String, module: &Module, ctx: &DumpContext) {
    let c = &ctx.colors;
    let types = module.types();
    let strings = module.strings();
    let mw = ctx.member_width;
    let sw = ctx.str_width;
    let tw = ctx.type_width;

    writeln!(out, "{}[type_members]{}", c.blue, c.reset)
        .expect("writing to a String is infallible");
    for (i, member) in types.members().enumerate() {
        let name = strings.get(member.name_id);
        let type_name = format_type_name(member.type_id, module, ctx);
        writeln!(
            out,
            "M{i:0mw$}: S{:0sw$} → T{:0tw$}  {}; {name}: {type_name}{}",
            u16::from(member.name_id),
            u16::from(member.type_id),
            c.dim,
            c.reset
        )
        .expect("writing to a String is infallible");
    }
    out.push('\n');
}

fn dump_types_names(out: &mut String, module: &Module, ctx: &DumpContext) {
    let c = &ctx.colors;
    let types = module.types();
    let strings = module.strings();
    let nw = ctx.name_width;
    let sw = ctx.str_width;
    let tw = ctx.type_width;

    writeln!(out, "{}[type_names]{}", c.blue, c.reset).expect("writing to a String is infallible");
    for (i, entry) in types.names().enumerate() {
        let name = strings.get(entry.name_id);
        writeln!(
            out,
            "N{i:0nw$}: S{:0sw$} → T{:0tw$}  {}; {}{name}{}",
            u16::from(entry.name_id),
            u16::from(entry.type_id),
            c.dim,
            c.blue,
            c.reset
        )
        .expect("writing to a String is infallible");
    }
    out.push('\n');
}

/// Format a type ID as a human-readable name.
fn format_type_name(type_id: TypeId, module: &Module, ctx: &DumpContext) -> String {
    let types = module.types();
    let strings = module.strings();

    if let Some(def) = types.get(type_id)
        && let TypeDefKind::Primitive(kind) = def.decode()
        && let Some(name) = kind.primitive_name()
    {
        return format!("<{}>", name);
    }

    for entry in types.names() {
        if entry.type_id == type_id {
            return strings.get(entry.name_id).to_string();
        }
    }

    let tw = ctx.type_width;
    format!("T{:0tw$}", u16::from(type_id))
}

fn dump_entry_points(out: &mut String, module: &Module, ctx: &DumpContext) {
    let c = &ctx.colors;
    let strings = module.strings();
    let entry_points = module.entry_points();
    let stw = ctx.addr_width;
    let tw = ctx.type_width;

    writeln!(out, "{}[entry_points]{}", c.blue, c.reset)
        .expect("writing to a String is infallible");

    let mut entries: Vec<_> = entry_points
        .iter()
        .map(|ep| {
            let name = strings.get(ep.name());
            (name, ep.target(), u16::from(ep.result_type()))
        })
        .collect();
    entries.sort_by_key(|(name, _, _)| *name);

    let max_len = entries.iter().map(|(n, _, _)| n.len()).max().unwrap_or(0);

    for (name, target, type_id) in entries {
        writeln!(
            out,
            "{}{name:width$}{} = {:0stw$} :: T{type_id:0tw$}",
            c.blue,
            c.reset,
            target,
            width = max_len
        )
        .expect("writing to a String is infallible");
    }
    out.push('\n');
}

fn dump_spans(out: &mut String, module: &Module, ctx: &DumpContext) {
    let spans = module.spans();
    if spans.is_empty() {
        return;
    }

    let c = &ctx.colors;
    let pw = width_for_count(spans.len());
    let tw = ctx.type_width;
    let mw = ctx.member_width;

    writeln!(out, "{}[spans]{}", c.blue, c.reset).expect("writing to a String is infallible");
    for (i, span) in spans.iter().enumerate() {
        let binding = if span.type_id == SPAN_NO_BINDING {
            String::new()
        } else if span.member == SPAN_NO_BINDING {
            format!("  T{:0tw$}", span.type_id)
        } else {
            format!("  T{:0tw$}.M{:0mw$}", span.type_id, span.member)
        };
        writeln!(
            out,
            "P{i:0pw$} {:<10} {}..{}  source_id={}{}",
            span.kind.name(),
            span.start,
            span.end,
            span.source_id,
            binding
        )
        .expect("writing to a String is infallible");
    }
    out.push('\n');
}

/// Check if an instruction is padding (all-zeros Match8).
///
/// Padding slots contain zero bytes which decode as terminal epsilon Match8
/// with Any node kind, no field constraint, and next=0.
fn is_padding(instr: &Instruction) -> bool {
    match instr {
        Instruction::Match(m) => {
            m.is_match8()
                && m.nav == Nav::Epsilon
                && matches!(m.node_kind, NodeKindConstraint::Any)
                && m.node_field.is_none()
                && m.is_terminal()
        }
        _ => false,
    }
}

fn dump_code(out: &mut String, module: &Module, ctx: &DumpContext) {
    let c = &ctx.colors;
    let header = module.header();
    let total_words = header.instruction_word_count as usize;
    let fmt = DumpFormatter {
        ctx,
        addr_width: ctx.addr_width,
    };

    writeln!(out, "{}[instructions]{}", c.blue, c.reset)
        .expect("writing to a String is infallible");

    let mut addr = CodeAddr::ZERO;
    let mut first_label = true;
    while addr.as_usize() < total_words {
        if let Some(label) = ctx.addr_labels.get(&addr) {
            if first_label {
                writeln!(out, "{}{label}{}:", c.blue, c.reset)
                    .expect("writing to a String is infallible");
                first_label = false;
            } else {
                writeln!(out, "\n{}{label}{}:", c.blue, c.reset)
                    .expect("writing to a String is infallible");
            }
        }

        let instr = module.decode_instruction(addr);

        if is_padding(&instr) {
            writeln!(out, "{}", fmt.padding_word(addr)).expect("writing to a String is infallible");
            addr = addr
                .checked_add(1)
                .expect("instruction address fits in u16");
            continue;
        }

        let line = fmt.instruction(addr, &instr);
        out.push_str(&line);
        out.push('\n');

        let words = instruction_word_count(&instr);
        addr = addr
            .checked_add(words)
            .expect("instruction address fits in u16");
    }
}

/// Bundles the precomputed context and address width threaded through every
/// per-instruction formatting routine.
struct DumpFormatter<'a> {
    ctx: &'a DumpContext,
    addr_width: usize,
}

fn instruction_word_count(instr: &Instruction) -> u16 {
    match instr {
        Instruction::Match(m) => m.word_count(),
        Instruction::Call(_)
        | Instruction::RoutedCall(_)
        | Instruction::SplitCall(_)
        | Instruction::Return(_) => 1,
    }
}

impl DumpFormatter<'_> {
    /// Format a single padding word line.
    ///
    /// Output: `  07  ...` (word address and "..." in the symbol column)
    fn padding_word(&self, addr: CodeAddr) -> String {
        LineBuilder::new(self.addr_width)
            .instruction_prefix(addr.get(), Symbol::PADDING)
            .trim_end()
            .to_string()
    }

    fn instruction(&self, addr: CodeAddr, instr: &Instruction) -> String {
        match instr {
            Instruction::Match(m) => self.format_match(addr, m),
            Instruction::Call(c) => self.format_call(addr, c),
            Instruction::RoutedCall(c) => self.format_routed_call(addr, c),
            Instruction::SplitCall(c) => self.format_split_call(addr, c),
            Instruction::Return(r) => self.format_return(addr, r),
        }
    }

    fn format_routed_call(&self, addr: CodeAddr, call: &RoutedCall) -> String {
        let colors = &self.ctx.colors;
        let builder = LineBuilder::new(self.addr_width);
        let prefix = format!(
            "  {:0sw$} {} ",
            addr,
            Symbol::EMPTY.format(),
            sw = self.addr_width
        );
        let target_name = self
            .ctx
            .label_for(call.target)
            .map(String::from)
            .unwrap_or_else(|| format!("@{:0w$}", u16::from(call.target), w = self.addr_width));
        let content = format!("({}{}{})", colors.blue, target_name, colors.reset);
        let successors = format!(
            "{:0w$} : {:0w$}",
            u16::from(call.target),
            u16::from(call.next),
            w = self.addr_width
        );
        builder.pad_successors(format!("{prefix}{content}"), &successors)
    }

    fn format_split_call(&self, addr: CodeAddr, call: &SplitCall) -> String {
        let colors = &self.ctx.colors;
        let builder = LineBuilder::new(self.addr_width);
        let prefix = format!(
            "  {:0sw$} {} ",
            addr,
            Symbol::EMPTY.format(),
            sw = self.addr_width
        );
        let target_name = self
            .ctx
            .label_for(call.target)
            .map(String::from)
            .unwrap_or_else(|| format!("@{:0w$}", u16::from(call.target), w = self.addr_width));
        let content = format!("({}{}{})", colors.blue, target_name, colors.reset);
        let successors = format!(
            "{:0w$} : {:0w$} / {:0w$}",
            u16::from(call.target),
            u16::from(call.returns.matched),
            u16::from(call.returns.empty),
            w = self.addr_width
        );
        builder.pad_successors(format!("{prefix}{content}"), &successors)
    }

    fn format_match(&self, addr: CodeAddr, m: &Match) -> String {
        let builder = LineBuilder::new(self.addr_width);
        let symbol = nav_symbol(m.nav);
        let prefix = format!("  {:0aw$} {} ", addr, symbol.format(), aw = self.addr_width);

        let content = self.format_match_content(m);
        let successors = self.format_match_successors(m);

        let base = format!("{prefix}{content}");
        builder.pad_successors(base, &successors)
    }

    fn format_match_content(&self, m: &Match) -> String {
        self.ctx.render.dump_match_content(m)
    }

    fn format_match_successors(&self, m: &Match) -> String {
        if m.is_terminal() {
            "◼".to_string()
        } else {
            m.successors()
                .map(|s| self.format_addr(s))
                .collect::<Vec<_>>()
                .join(", ")
        }
    }

    fn format_call(&self, addr: CodeAddr, call: &Call) -> String {
        let c = &self.ctx.colors;
        let builder = LineBuilder::new(self.addr_width);
        let symbol = nav_symbol(call.nav);
        let prefix = format!("  {:0aw$} {} ", addr, symbol.format(), aw = self.addr_width);

        // Format field constraint if present
        let field_part = if let Some(field_id) = call.node_field {
            let name = self.ctx.render.dump_node_field_name(field_id);
            format!("{name}: ")
        } else {
            String::new()
        };

        let target_name = self
            .ctx
            .label_for(call.target)
            .map(String::from)
            .unwrap_or_else(|| format!("@{:0w$}", u16::from(call.target), w = self.addr_width));
        // Definition name in call is blue
        let content = format!("{field_part}({}{}{})", c.blue, target_name, c.reset);
        // Format as "target : return" with numeric IDs
        let successors = format!(
            "{:0w$} : {:0w$}",
            u16::from(call.target),
            u16::from(call.next),
            w = self.addr_width
        );

        let base = format!("{prefix}{content}");
        builder.pad_successors(base, &successors)
    }

    fn format_return(&self, addr: CodeAddr, return_: &Return) -> String {
        let builder = LineBuilder::new(self.addr_width);
        let prefix = format!(
            "  {:0sw$} {} ",
            addr,
            Symbol::EMPTY.format(),
            sw = self.addr_width
        );
        let outcome = match return_.mode.outcome() {
            plotnik_rt::ReturnOutcome::Matched => "▶",
            plotnik_rt::ReturnOutcome::Empty => "▶ empty",
        };
        builder.pad_successors(prefix, outcome)
    }

    fn format_addr(&self, addr: SuccessorAddr) -> String {
        let c = &self.ctx.colors;
        if let Some(label) = self.ctx.label_for(addr) {
            format!("▶({}{}{})", c.blue, label, c.reset)
        } else {
            format!("{:0w$}", u16::from(addr), w = self.addr_width)
        }
    }
}
