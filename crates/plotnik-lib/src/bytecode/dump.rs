//! Human-readable bytecode dump for debugging and documentation.
//!
//! See `docs/binary-format/07-dump-format.md` for the output format specification.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::bytecode::StepAddr;
use crate::bytecode::predicate_op::PredicateOp;
use crate::core::{Colors, NodeFieldId, NodeKindId};

use super::format::{
    LineBuilder, PREAMBLE_NAME, Symbol, format_effect, nav_symbol, width_for_count,
};
use super::ids::TypeId;
use super::instructions::StepId;
use super::module::{Instruction, Module};
use super::nav::Nav;
use super::node_kind_constraint::NodeKindConstraint;
use super::type_meta::{TypeDefKind, TypeKind};
use super::{Call, Match, Return, Trampoline};
use crate::bytecode::type_system::TYPE_CUSTOM_START;

/// Generate a human-readable dump of the bytecode module.
pub fn dump(module: &Module, colors: Colors) -> String {
    let mut out = String::new();
    let ctx = DumpContext::new(module, colors);

    dump_strings(&mut out, module, &ctx);
    dump_regexes(&mut out, module, &ctx);
    dump_types_defs(&mut out, module, &ctx);
    dump_types_members(&mut out, module, &ctx);
    dump_types_names(&mut out, module, &ctx);
    dump_entrypoints(&mut out, module, &ctx);
    dump_code(&mut out, module, &ctx);

    out
}

/// Context for dump formatting, precomputes lookups for O(1) access.
struct DumpContext {
    /// Maps step ID to entrypoint name for labeling.
    step_labels: BTreeMap<u16, String>,
    /// Maps node kind ID to name.
    node_kind_names: BTreeMap<NodeKindId, String>,
    /// Maps node field ID to name.
    node_field_names: BTreeMap<NodeFieldId, String>,
    /// All strings (for predicate values, regex patterns, etc).
    all_strings: Vec<String>,
    /// Width for string indices (S#).
    str_width: usize,
    /// Width for type indices (T#).
    type_width: usize,
    /// Width for member indices (M#).
    member_width: usize,
    /// Width for name indices (N#).
    name_width: usize,
    /// Width for step indices.
    step_width: usize,
    /// Color palette.
    colors: Colors,
}

impl DumpContext {
    fn new(module: &Module, colors: Colors) -> Self {
        let header = module.header();
        let strings = module.strings();
        let entrypoints = module.entrypoints();
        let node_kinds = module.node_kinds();
        let node_fields = module.node_fields();

        let mut step_labels = BTreeMap::new();
        // Preamble always at step 0 (first in layout)
        step_labels.insert(u16::from(StepAddr::PREAMBLE), PREAMBLE_NAME.to_string());
        for ep in entrypoints.iter() {
            let name = strings.get(ep.name()).to_string();
            step_labels.insert(u16::from(ep.target()), name);
        }

        let mut node_kind_names = BTreeMap::new();
        for t in node_kinds.iter() {
            if let Ok(id) = NodeKindId::try_from(t.symbol) {
                node_kind_names.insert(id, strings.get(t.name).to_string());
            }
        }

        let mut node_field_names = BTreeMap::new();
        for f in node_fields.iter() {
            if let Ok(id) = NodeFieldId::try_from(f.symbol) {
                node_field_names.insert(id, strings.get(f.name).to_string());
            }
        }

        let str_count = header.str_table_count as usize;
        let all_strings: Vec<String> = (0..str_count).map(|i| strings.at(i).to_string()).collect();

        let types = module.types();
        // Builtins precede custom types; widen for both.
        let type_count = TYPE_CUSTOM_START as usize + types.defs_count();
        let str_width = width_for_count(str_count);
        let type_width = width_for_count(type_count);
        let member_width = width_for_count(types.members_count());
        let name_width = width_for_count(types.names_count());
        let step_width = width_for_count(header.transitions_count as usize);

        Self {
            step_labels,
            node_kind_names,
            node_field_names,
            all_strings,
            str_width,
            type_width,
            member_width,
            name_width,
            step_width,
            colors,
        }
    }

    fn label_for(&self, step: StepId) -> Option<&str> {
        self.step_labels.get(&u16::from(step)).map(|s| s.as_str())
    }

    fn node_kind_name(&self, id: NodeKindId) -> Option<&str> {
        self.node_kind_names.get(&id).map(|s| s.as_str())
    }

    fn node_field_name(&self, id: NodeFieldId) -> Option<&str> {
        self.node_field_names.get(&id).map(|s| s.as_str())
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
        let pattern = &ctx.all_strings[u16::from(string_id) as usize];
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
                    TypeKind::Void => "<Void>",
                    TypeKind::Node => "<Node>",
                    _ => unreachable!(),
                };
                (name.to_string(), String::new())
            }
            TypeDefKind::Wrapper { kind, inner } => {
                let formatted = match kind {
                    TypeKind::Optional => format!("Optional(T{:0tw$})", u16::from(inner)),
                    TypeKind::ArrayZeroOrMore => format!("ArrayStar(T{:0tw$})", u16::from(inner)),
                    TypeKind::ArrayOneOrMore => format!("ArrayPlus(T{:0tw$})", u16::from(inner)),
                    TypeKind::Alias => format!("Alias(T{:0tw$})", u16::from(inner)),
                    _ => unreachable!(),
                };
                let comment = match kind {
                    TypeKind::Optional => {
                        let inner_name = format_type_name(inner, module, ctx);
                        format!("{}  ; {}?{}", c.dim, inner_name, c.reset)
                    }
                    TypeKind::ArrayZeroOrMore => {
                        let inner_name = format_type_name(inner, module, ctx);
                        format!("{}  ; {}*{}", c.dim, inner_name, c.reset)
                    }
                    TypeKind::ArrayOneOrMore => {
                        let inner_name = format_type_name(inner, module, ctx);
                        format!("{}  ; {}+{}", c.dim, inner_name, c.reset)
                    }
                    TypeKind::Alias => String::new(),
                    _ => unreachable!(),
                };
                (formatted, comment)
            }
            TypeDefKind::Struct {
                member_start,
                member_count,
            } => {
                let formatted = format!("Struct  M{:0mw$}:{}", member_start, member_count);
                let fields: Vec<_> = types
                    .members_of(&def)
                    .map(|m| strings.get(m.name_id).to_string())
                    .collect();
                let comment = format!("{}  ; {{ {} }}{}", c.dim, fields.join(", "), c.reset);
                (formatted, comment)
            }
            TypeDefKind::Enum {
                member_start,
                member_count,
            } => {
                let formatted = format!("Enum    M{:0mw$}:{}", member_start, member_count);
                let variants: Vec<_> = types
                    .members_of(&def)
                    .map(|m| strings.get(m.name_id).to_string())
                    .collect();
                let comment = format!("{}  ; {}{}", c.dim, variants.join(" | "), c.reset);
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

fn dump_entrypoints(out: &mut String, module: &Module, ctx: &DumpContext) {
    let c = &ctx.colors;
    let strings = module.strings();
    let entrypoints = module.entrypoints();
    let stw = ctx.step_width;
    let tw = ctx.type_width;

    writeln!(out, "{}[entrypoints]{}", c.blue, c.reset).expect("writing to a String is infallible");

    let mut entries: Vec<_> = entrypoints
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
    let transitions_count = header.transitions_count as usize;
    let fmt = DumpFormatter {
        module,
        ctx,
        step_width: ctx.step_width,
    };

    writeln!(out, "{}[transitions]{}", c.blue, c.reset).expect("writing to a String is infallible");

    let mut step = 0u16;
    let mut first_label = true;
    while (step as usize) < transitions_count {
        // Check if this step has a label (using raw u16)
        if let Some(label) = ctx.step_labels.get(&step) {
            if first_label {
                writeln!(out, "{}{label}{}:", c.blue, c.reset)
                    .expect("writing to a String is infallible");
                first_label = false;
            } else {
                writeln!(out, "\n{}{label}{}:", c.blue, c.reset)
                    .expect("writing to a String is infallible");
            }
        }

        let instr = module.decode_step(step);

        if is_padding(&instr) {
            writeln!(out, "{}", fmt.padding_step(step)).expect("writing to a String is infallible");
            step += 1;
            continue;
        }

        let line = fmt.instruction(step, &instr);
        out.push_str(&line);
        out.push('\n');

        let size = instruction_step_count(&instr);
        step += size;
    }
}

/// Bundles the module, precomputed context, and step-index width threaded
/// through every per-instruction formatting routine.
struct DumpFormatter<'a> {
    module: &'a Module,
    ctx: &'a DumpContext,
    step_width: usize,
}

fn instruction_step_count(instr: &Instruction) -> u16 {
    match instr {
        Instruction::Match(m) => m.step_count(),
        Instruction::Call(_) | Instruction::Return(_) | Instruction::Trampoline(_) => 1,
    }
}

impl DumpFormatter<'_> {
    /// Format a single padding step line.
    ///
    /// Output: `  07  ... ` (step number and " ... " in symbol column)
    fn padding_step(&self, step: u16) -> String {
        LineBuilder::new(self.step_width).instruction_prefix(step, Symbol::PADDING)
    }

    fn instruction(&self, step: u16, instr: &Instruction) -> String {
        match instr {
            Instruction::Match(m) => self.format_match(step, m),
            Instruction::Call(c) => self.format_call(step, c),
            Instruction::Return(r) => self.format_return(step, r),
            Instruction::Trampoline(t) => self.format_trampoline(step, t),
        }
    }

    fn format_match(&self, step: u16, m: &Match) -> String {
        let builder = LineBuilder::new(self.step_width);
        let symbol = nav_symbol(m.nav);
        let prefix = format!("  {:0sw$} {} ", step, symbol.format(), sw = self.step_width);

        let content = self.format_match_content(m);
        let successors = self.format_match_successors(m);

        let base = format!("{prefix}{content}");
        builder.pad_successors(base, &successors)
    }

    fn format_match_content(&self, m: &Match) -> String {
        let ctx = self.ctx;
        let mut parts = Vec::new();

        let pre: Vec<_> = m.pre_effects().map(|e| format_effect(&e)).collect();
        if !pre.is_empty() {
            parts.push(format!("[{}]", pre.join(" ")));
        }

        // Skip neg_fields and node pattern for epsilon (no node interaction)
        if !m.is_epsilon() {
            for field_id in m.neg_fields() {
                let name = ctx
                    .node_field_name(field_id)
                    .map(String::from)
                    .unwrap_or_else(|| format!("field#{field_id}"));
                parts.push(format!("-{name}"));
            }

            let node_part = self.format_node_pattern(m);
            if !node_part.is_empty() {
                parts.push(node_part);
            }

            if let Some(predicate) = m.predicate() {
                let op = PredicateOp::from_byte(predicate.op);
                let value = if predicate.is_regex {
                    let string_id = self
                        .module
                        .regexes()
                        .pattern_string_id(predicate.value_ref as usize);
                    let pattern = &ctx.all_strings[u16::from(string_id) as usize];
                    format!("/{}/", pattern)
                } else {
                    let s = &ctx.all_strings[predicate.value_ref as usize];
                    format!("{:?}", s)
                };
                parts.push(format!("{} {}", op.as_str(), value));
            }
        }

        let post: Vec<_> = m.post_effects().map(|e| format_effect(&e)).collect();
        if !post.is_empty() {
            parts.push(format!("[{}]", post.join(" ")));
        }

        parts.join(" ")
    }

    /// Format node pattern: `field: (type)` or `(type)` or `field: _` or `(_)` or `"text"`
    fn format_node_pattern(&self, m: &Match) -> String {
        let ctx = self.ctx;
        let mut result = String::new();

        if let Some(field_id) = m.node_field {
            let name = ctx
                .node_field_name(field_id)
                .map(String::from)
                .unwrap_or_else(|| format!("field#{field_id}"));
            result.push_str(&name);
            result.push_str(": ");
        }

        match m.node_kind {
            NodeKindConstraint::Any => {
                result.push('_');
            }
            NodeKindConstraint::Named(None) => {
                result.push_str("(_)");
            }
            NodeKindConstraint::Named(Some(type_id)) => {
                let name = ctx
                    .node_kind_name(type_id)
                    .map(String::from)
                    .unwrap_or_else(|| format!("node#{type_id}"));
                result.push('(');
                result.push_str(&name);
                result.push(')');
            }
            NodeKindConstraint::Anonymous(None) => {
                // future syntax: anonymous wildcard has no concrete form yet
                result.push_str("\"_\"");
            }
            NodeKindConstraint::Anonymous(Some(type_id)) => {
                let name = ctx
                    .node_kind_name(type_id)
                    .map(String::from)
                    .unwrap_or_else(|| format!("anon#{type_id}"));
                result.push('"');
                result.push_str(&name);
                result.push('"');
            }
        }

        result
    }

    fn format_match_successors(&self, m: &Match) -> String {
        if m.is_terminal() {
            "◼".to_string()
        } else {
            m.successors()
                .map(|s| self.format_step(s))
                .collect::<Vec<_>>()
                .join(", ")
        }
    }

    fn format_call(&self, step: u16, call: &Call) -> String {
        let c = &self.ctx.colors;
        let builder = LineBuilder::new(self.step_width);
        let symbol = nav_symbol(call.nav);
        let prefix = format!("  {:0sw$} {} ", step, symbol.format(), sw = self.step_width);

        // Format field constraint if present
        let field_part = if let Some(field_id) = call.node_field {
            let name = self
                .ctx
                .node_field_name(field_id)
                .map(String::from)
                .unwrap_or_else(|| format!("field#{field_id}"));
            format!("{name}: ")
        } else {
            String::new()
        };

        let target_name = self
            .ctx
            .label_for(call.target)
            .map(String::from)
            .unwrap_or_else(|| format!("@{:0w$}", u16::from(call.target), w = self.step_width));
        // Definition name in call is blue
        let content = format!("{field_part}({}{}{})", c.blue, target_name, c.reset);
        // Format as "target : return" with numeric IDs
        let successors = format!(
            "{:0w$} : {:0w$}",
            u16::from(call.target),
            u16::from(call.next),
            w = self.step_width
        );

        let base = format!("{prefix}{content}");
        builder.pad_successors(base, &successors)
    }

    fn format_return(&self, step: u16, _r: &Return) -> String {
        let builder = LineBuilder::new(self.step_width);
        let prefix = format!(
            "  {:0sw$} {} ",
            step,
            Symbol::EMPTY.format(),
            sw = self.step_width
        );
        builder.pad_successors(prefix, "▶")
    }

    fn format_trampoline(&self, step: u16, t: &Trampoline) -> String {
        let builder = LineBuilder::new(self.step_width);
        let prefix = format!(
            "  {:0sw$} {} ",
            step,
            Symbol::EMPTY.format(),
            sw = self.step_width
        );
        let content = "Trampoline";
        let successors = format!("{:0w$}", u16::from(t.next), w = self.step_width);
        let base = format!("{prefix}{content}");
        builder.pad_successors(base, &successors)
    }

    fn format_step(&self, step: StepId) -> String {
        let c = &self.ctx.colors;
        if let Some(label) = self.ctx.label_for(step) {
            format!("▶({}{}{})", c.blue, label, c.reset)
        } else {
            format!("{:0w$}", u16::from(step), w = self.step_width)
        }
    }
}
