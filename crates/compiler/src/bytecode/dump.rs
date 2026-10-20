use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::core::Colors;

use super::format::{Symbol, nav_symbol, width_for_count};
use super::ids::TypeId;
use super::instructions::CodeAddr;
use super::module::{Instruction, Module, SwitchEntriesView};
use super::node_kind_constraint::NodeKindConstraint;
use super::render::ModuleRenderContext;
use super::type_meta::{TypeDefKind, TypeKind};
use super::{SPAN_NO_BINDING, SwitchEntry};
use plotnik_runtime::Nav;

/// Generate a human-readable dump of a compiler-produced bytecode module.
pub fn dump(module: &Module, colors: Colors) -> String {
    let mut out = String::new();
    let ctx = DumpContext::new(module, colors);

    dump_strings(&mut out, module, &ctx);
    dump_regexes(&mut out, module, &ctx);
    dump_types_defs(&mut out, module, &ctx);
    dump_types_members(&mut out, module, &ctx);
    dump_types_names(&mut out, module, &ctx);
    dump_entrypoints(&mut out, module, &ctx);
    dump_spans(&mut out, module, &ctx);
    dump_code(&mut out, module, &ctx);

    out
}

struct DumpContext {
    addr_labels: BTreeMap<CodeAddr, String>,
    render: ModuleRenderContext,
    str_width: usize,
    type_width: usize,
    member_width: usize,
    name_width: usize,
    colors: Colors,
}

impl DumpContext {
    fn new(module: &Module, colors: Colors) -> Self {
        let header = module.header();
        let strings = module.strings();
        let entrypoints = module.entrypoints();

        let mut addr_labels = BTreeMap::new();
        for ep in entrypoints.iter() {
            let name = match ep.identity() {
                super::EntrypointIdentity::Named(name) => strings.get(name),
                super::EntrypointIdentity::Expression => "<expression>",
            }
            .to_string();
            addr_labels.insert(ep.target(), name);
        }

        let str_count = header.str_table_count as usize;

        let types = module.types();
        let type_count = types.defs_count();
        let str_width = width_for_count(str_count);
        let type_width = width_for_count(type_count);
        let member_width = width_for_count(types.members_count());
        let name_width = width_for_count(types.names_count());

        Self {
            addr_labels,
            render: ModuleRenderContext::new(module, colors),
            str_width,
            type_width,
            member_width,
            name_width,
            colors,
        }
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

fn dump_entrypoints(out: &mut String, module: &Module, ctx: &DumpContext) {
    let c = &ctx.colors;
    let strings = module.strings();
    let entrypoints = module.entrypoints();
    let stw = ctx.render.addr_width;
    let tw = ctx.type_width;

    writeln!(out, "{}[entrypoints]{}", c.blue, c.reset).expect("writing to a String is infallible");

    let mut entries: Vec<_> = entrypoints
        .iter()
        .map(|ep| {
            let name = match ep.identity() {
                super::EntrypointIdentity::Named(name) => strings.get(name),
                super::EntrypointIdentity::Expression => "<expression>",
            };
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
    let kind_width = spans
        .iter()
        .map(|span| span.kind.name().len())
        .max()
        .expect("spans is nonempty");

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
            "P{i:0pw$} {:<kind_width$} {}..{} src={}{}",
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
    let fmt = DumpFormatter { ctx, module };

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

struct DumpFormatter<'a> {
    ctx: &'a DumpContext,
    module: &'a Module,
}

fn instruction_word_count(instr: &Instruction) -> u16 {
    match instr {
        Instruction::Match(m) => m.word_count(),
        Instruction::Call(call) => {
            if call.arity() == 1 {
                1
            } else {
                3
            }
        }
        Instruction::Return(_) | Instruction::Switch { .. } => 1,
    }
}

impl DumpFormatter<'_> {
    fn padding_word(&self, addr: CodeAddr) -> String {
        self.ctx
            .render
            .builder
            .instruction_prefix(addr.get(), Symbol::PADDING)
            .trim_end()
            .to_string()
    }

    fn instruction(&self, addr: CodeAddr, instr: &Instruction) -> String {
        match instr {
            Instruction::Switch { entries, .. } => self.format_switch(addr, *entries),
            _ => self.ctx.render.instruction_line(addr, instr),
        }
    }

    fn format_switch(&self, addr: CodeAddr, entries: SwitchEntriesView<'_>) -> String {
        let builder = &self.ctx.render.builder;
        let mut result =
            builder.instruction(addr.get(), nav_symbol(Nav::StayExact), "switch", "", &[]);
        let mut cases = entries.iter().collect::<Vec<_>>();
        cases.sort_by_key(|entry| entry.target);

        for entry in cases {
            let kind = self.ctx.render.dump_node_kind(self.switch_node_kind(entry));
            let target = self
                .ctx
                .render
                .format_addr(CodeAddr::from(u16::from(entry.target)));
            let row = builder.switch_case(&kind, &target);
            result.push('\n');
            result.push_str(&row);
        }

        result
    }

    fn switch_node_kind(&self, entry: SwitchEntry) -> NodeKindConstraint {
        let mut addr = CodeAddr::from(u16::from(entry.target));
        loop {
            let Instruction::Match(m) = self.module.decode_instruction(addr) else {
                unreachable!(
                    "switch branches begin with a concrete match through epsilon prefixes"
                );
            };

            if !m.is_epsilon() {
                assert_eq!(m.node_kind.kind_id(), Some(entry.kind));
                return m.node_kind;
            }

            let mut successors = m.successors();
            addr = CodeAddr::from(u16::from(
                successors.next().expect("switch prefix has a successor"),
            ));
            assert!(
                successors.next().is_none(),
                "switch prefix has one successor"
            );
        }
    }
}
