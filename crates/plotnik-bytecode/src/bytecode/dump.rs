//! Human-readable bytecode dump for debugging and documentation.
//!
//! See `docs/binary-format/07-dump-format.md` for the output format specification.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use plotnik_core::Colors;
use crate::predicate_op::PredicateOp;

use super::format::{LineBuilder, Symbol, format_effect, nav_symbol, width_for_count};
use super::ids::TypeId;
use super::instructions::StepId;
use super::module::{Instruction, Module};
use super::node_type_ir::NodeTypeIR;
use super::nav::Nav;
use super::type_meta::{TypeData, TypeKind};
use super::{Call, Match, Return, Trampoline};

/// Generate a human-readable dump of the bytecode module.
pub fn dump(module: &Module, colors: Colors) -> String {
    let mut out = String::new();
    let ctx = DumpContext::new(module, colors);

    dump_strings(&mut out, module, &ctx);
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
    /// Maps node type ID to name.
    node_type_names: BTreeMap<u16, String>,
    /// Maps node field ID to name.
    node_field_names: BTreeMap<u16, String>,
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
        let node_types = module.node_types();
        let node_fields = module.node_fields();

        let mut step_labels = BTreeMap::new();
        // Preamble always at step 0 (first in layout)
        step_labels.insert(0, "_ObjWrap".to_string());
        for i in 0..entrypoints.len() {
            let ep = entrypoints.get(i);
            let name = strings.get(ep.name()).to_string();
            step_labels.insert(ep.target(), name);
        }

        let mut node_type_names = BTreeMap::new();
        for i in 0..node_types.len() {
            let t = node_types.get(i);
            node_type_names.insert(t.id, strings.get(t.name).to_string());
        }

        let mut node_field_names = BTreeMap::new();
        for i in 0..node_fields.len() {
            let f = node_fields.get(i);
            node_field_names.insert(f.id, strings.get(f.name).to_string());
        }

        // Collect all strings for unlinked mode lookups
        let str_count = header.str_table_count as usize;
        let all_strings: Vec<String> = (0..str_count)
            .map(|i| strings.get_by_index(i).to_string())
            .collect();

        // Compute widths for index formatting
        let types = module.types();
        let type_count = 3 + types.defs_count(); // 3 builtins + custom types
        let str_width = width_for_count(str_count);
        let type_width = width_for_count(type_count);
        let member_width = width_for_count(types.members_count());
        let name_width = width_for_count(types.names_count());
        let step_width = width_for_count(header.transitions_count as usize);

        Self {
            step_labels,
            node_type_names,
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
        self.step_labels.get(&step.get()).map(|s| s.as_str())
    }

    /// Get the name for a node type ID.
    fn node_type_name(&self, id: u16) -> Option<&str> {
        self.node_type_names.get(&id).map(|s| s.as_str())
    }

    /// Get the name for a node field ID.
    fn node_field_name(&self, id: u16) -> Option<&str> {
        self.node_field_names.get(&id).map(|s| s.as_str())
    }
}

fn dump_strings(out: &mut String, module: &Module, ctx: &DumpContext) {
    let c = &ctx.colors;
    let strings = module.strings();
    let count = module.header().str_table_count as usize;
    let w = ctx.str_width;

    writeln!(out, "{}[strings]{}", c.blue, c.reset).unwrap();
    for i in 0..count {
        let s = strings.get_by_index(i);
        writeln!(out, "S{i:0w$} {}{s:?}{}", c.green, c.reset).unwrap();
    }
    out.push('\n');
}

fn dump_types_defs(out: &mut String, module: &Module, ctx: &DumpContext) {
    let c = &ctx.colors;
    let types = module.types();
    let strings = module.strings();
    let tw = ctx.type_width;
    let mw = ctx.member_width;

    writeln!(out, "{}[type_defs]{}", c.blue, c.reset).unwrap();

    // All types are now in type_defs, including builtins
    for i in 0..types.defs_count() {
        let def = types.get_def(i);

        let (formatted, comment) = match def.classify() {
            TypeData::Primitive(kind) => {
                let name = match kind {
                    TypeKind::Void => "<Void>",
                    TypeKind::Node => "<Node>",
                    TypeKind::String => "<String>",
                    _ => unreachable!(),
                };
                (name.to_string(), String::new())
            }
            TypeData::Wrapper { kind, inner } => {
                let formatted = match kind {
                    TypeKind::Optional => format!("Optional(T{:0tw$})", inner.0),
                    TypeKind::ArrayZeroOrMore => format!("ArrayStar(T{:0tw$})", inner.0),
                    TypeKind::ArrayOneOrMore => format!("ArrayPlus(T{:0tw$})", inner.0),
                    TypeKind::Alias => format!("Alias(T{:0tw$})", inner.0),
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
            TypeData::Composite {
                kind,
                member_start,
                member_count,
            } => {
                let formatted = match kind {
                    TypeKind::Struct => {
                        format!("Struct  M{:0mw$}:{}", member_start, member_count)
                    }
                    TypeKind::Enum => format!("Enum    M{:0mw$}:{}", member_start, member_count),
                    _ => unreachable!(),
                };
                let comment = match kind {
                    TypeKind::Struct => {
                        let fields: Vec<_> = types
                            .members_of(&def)
                            .map(|m| strings.get(m.name).to_string())
                            .collect();
                        format!("{}  ; {{ {} }}{}", c.dim, fields.join(", "), c.reset)
                    }
                    TypeKind::Enum => {
                        let variants: Vec<_> = types
                            .members_of(&def)
                            .map(|m| strings.get(m.name).to_string())
                            .collect();
                        format!("{}  ; {}{}", c.dim, variants.join(" | "), c.reset)
                    }
                    _ => unreachable!(),
                };
                (formatted, comment)
            }
        };

        writeln!(out, "T{i:0tw$} = {formatted}{comment}").unwrap();
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

    writeln!(out, "{}[type_members]{}", c.blue, c.reset).unwrap();
    for i in 0..types.members_count() {
        let member = types.get_member(i);
        let name = strings.get(member.name);
        let type_name = format_type_name(member.type_id, module, ctx);
        writeln!(
            out,
            "M{i:0mw$}: S{:0sw$} → T{:0tw$}  {}; {name}: {type_name}{}",
            member.name.0, member.type_id.0, c.dim, c.reset
        )
        .unwrap();
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

    writeln!(out, "{}[type_names]{}", c.blue, c.reset).unwrap();
    for i in 0..types.names_count() {
        let entry = types.get_name(i);
        let name = strings.get(entry.name);
        writeln!(
            out,
            "N{i:0nw$}: S{:0sw$} → T{:0tw$}  {}; {}{name}{}",
            entry.name.0, entry.type_id.0, c.dim, c.blue, c.reset
        )
        .unwrap();
    }
    out.push('\n');
}

/// Format a type ID as a human-readable name.
fn format_type_name(type_id: TypeId, module: &Module, ctx: &DumpContext) -> String {
    let types = module.types();
    let strings = module.strings();

    // Check if it's a primitive type
    if let Some(def) = types.get(type_id)
        && let TypeData::Primitive(kind) = def.classify()
        && let Some(name) = kind.primitive_name()
    {
        return format!("<{}>", name);
    }

    // Try to find a name in types.names
    for i in 0..types.names_count() {
        let entry = types.get_name(i);
        if entry.type_id == type_id {
            return strings.get(entry.name).to_string();
        }
    }

    // Fall back to T# format
    let tw = ctx.type_width;
    format!("T{:0tw$}", type_id.0)
}

fn dump_entrypoints(out: &mut String, module: &Module, ctx: &DumpContext) {
    let c = &ctx.colors;
    let strings = module.strings();
    let entrypoints = module.entrypoints();
    let stw = ctx.step_width;
    let tw = ctx.type_width;

    writeln!(out, "{}[entrypoints]{}", c.blue, c.reset).unwrap();

    // Collect and sort by name for display
    let mut entries: Vec<_> = (0..entrypoints.len())
        .map(|i| {
            let ep = entrypoints.get(i);
            let name = strings.get(ep.name());
            (name, ep.target(), ep.result_type().0)
        })
        .collect();
    entries.sort_by_key(|(name, _, _)| *name);

    // Find max name length for alignment
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
        .unwrap();
    }
    out.push('\n');
}

/// Check if an instruction is padding (all-zeros Match8).
///
/// Padding slots contain zero bytes which decode as terminal epsilon Match8
/// with Any node type, no field constraint, and next=0.
fn is_padding(instr: &Instruction) -> bool {
    match instr {
        Instruction::Match(m) => {
            m.is_match8()
                && m.nav == Nav::Epsilon
                && matches!(m.node_type, NodeTypeIR::Any)
                && m.node_field.is_none()
                && m.is_terminal()
        }
        _ => false,
    }
}

/// Format a single padding step line.
///
/// Output: `  07  ... ` (step number and " ... " in symbol column)
fn format_padding_step(step: u16, step_width: usize) -> String {
    LineBuilder::new(step_width).instruction_prefix(step, Symbol::PADDING)
}

fn dump_code(out: &mut String, module: &Module, ctx: &DumpContext) {
    let c = &ctx.colors;
    let header = module.header();
    let transitions_count = header.transitions_count as usize;
    let step_width = ctx.step_width;

    writeln!(out, "{}[transitions]{}", c.blue, c.reset).unwrap();

    let mut step = 0u16;
    let mut first_label = true;
    while (step as usize) < transitions_count {
        // Check if this step has a label (using raw u16)
        if let Some(label) = ctx.step_labels.get(&step) {
            if first_label {
                writeln!(out, "{}{label}{}:", c.blue, c.reset).unwrap();
                first_label = false;
            } else {
                writeln!(out, "\n{}{label}{}:", c.blue, c.reset).unwrap();
            }
        }

        let instr = module.decode_step(step);

        // Check for padding (all-zeros Match8 instruction)
        if is_padding(&instr) {
            writeln!(out, "{}", format_padding_step(step, step_width)).unwrap();
            step += 1;
            continue;
        }

        let line = format_instruction(step, &instr, module, ctx, step_width);
        out.push_str(&line);
        out.push('\n');

        // Advance by instruction size
        let size = instruction_step_count(&instr);
        step += size;
    }
}

fn instruction_step_count(instr: &Instruction) -> u16 {
    match instr {
        Instruction::Match(m) => {
            let pre = m.pre_effects().count();
            let neg = m.neg_fields().count();
            let post = m.post_effects().count();
            let succ = m.succ_count();
            let pred = if m.has_predicate() { 2 } else { 0 };
            let slots = pre + neg + post + pred + succ;

            if pre == 0 && neg == 0 && post == 0 && pred == 0 && succ <= 1 {
                1 // Match8
            } else if slots <= 4 {
                2 // Match16
            } else if slots <= 8 {
                3 // Match24
            } else if slots <= 12 {
                4 // Match32
            } else if slots <= 20 {
                6 // Match48
            } else {
                8 // Match64
            }
        }
        Instruction::Call(_) | Instruction::Return(_) | Instruction::Trampoline(_) => 1,
    }
}

fn format_instruction(
    step: u16,
    instr: &Instruction,
    module: &Module,
    ctx: &DumpContext,
    step_width: usize,
) -> String {
    match instr {
        Instruction::Match(m) => format_match(step, m, module, ctx, step_width),
        Instruction::Call(c) => format_call(step, c, module, ctx, step_width),
        Instruction::Return(r) => format_return(step, r, module, ctx, step_width),
        Instruction::Trampoline(t) => format_trampoline(step, t, ctx, step_width),
    }
}

fn format_match(
    step: u16,
    m: &Match,
    module: &Module,
    ctx: &DumpContext,
    step_width: usize,
) -> String {
    let builder = LineBuilder::new(step_width);
    let symbol = nav_symbol(m.nav);
    let prefix = format!("  {:0sw$} {} ", step, symbol.format(), sw = step_width);

    let content = format_match_content(m, module, ctx);
    let successors = format_match_successors(m, ctx, step_width);

    let base = format!("{prefix}{content}");
    builder.pad_successors(base, &successors)
}

fn format_match_content(m: &Match, module: &Module, ctx: &DumpContext) -> String {
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

        let node_part = format_node_pattern(m, ctx);
        if !node_part.is_empty() {
            parts.push(node_part);
        }

        // Format predicate if present
        if let Some((op, is_regex, value_ref)) = m.predicate() {
            let op = PredicateOp::from_byte(op);
            let value = if is_regex {
                let string_id = module.regexes().get_string_id(value_ref as usize);
                let pattern = &ctx.all_strings[string_id.get() as usize];
                format!("/{}/", pattern)
            } else {
                let s = &ctx.all_strings[value_ref as usize];
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
fn format_node_pattern(m: &Match, ctx: &DumpContext) -> String {
    let mut result = String::new();

    if let Some(field_id) = m.node_field {
        let name = ctx
            .node_field_name(field_id.get())
            .map(String::from)
            .unwrap_or_else(|| format!("field#{}", field_id.get()));
        result.push_str(&name);
        result.push_str(": ");
    }

    match m.node_type {
        NodeTypeIR::Any => {
            // Any node wildcard: `_`
            result.push('_');
        }
        NodeTypeIR::Named(None) => {
            // Named wildcard: any named node
            result.push_str("(_)");
        }
        NodeTypeIR::Named(Some(type_id)) => {
            // Specific named node type
            let name = ctx
                .node_type_name(type_id.get())
                .map(String::from)
                .unwrap_or_else(|| format!("node#{}", type_id.get()));
            result.push('(');
            result.push_str(&name);
            result.push(')');
        }
        NodeTypeIR::Anonymous(None) => {
            // Anonymous wildcard: any anonymous node (future syntax)
            result.push_str("\"_\"");
        }
        NodeTypeIR::Anonymous(Some(type_id)) => {
            // Specific anonymous node (literal token)
            let name = ctx
                .node_type_name(type_id.get())
                .map(String::from)
                .unwrap_or_else(|| format!("anon#{}", type_id.get()));
            result.push('"');
            result.push_str(&name);
            result.push('"');
        }
    }

    result
}

fn format_match_successors(m: &Match, ctx: &DumpContext, step_width: usize) -> String {
    if m.is_terminal() {
        "◼".to_string()
    } else {
        m.successors()
            .map(|s| format_step(s, ctx, step_width))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn format_call(
    step: u16,
    call: &Call,
    _module: &Module,
    ctx: &DumpContext,
    step_width: usize,
) -> String {
    let c = &ctx.colors;
    let builder = LineBuilder::new(step_width);
    let symbol = nav_symbol(call.nav());
    let prefix = format!("  {:0sw$} {} ", step, symbol.format(), sw = step_width);

    // Format field constraint if present
    let field_part = if let Some(field_id) = call.node_field {
        let name = ctx
            .node_field_name(field_id.get())
            .map(String::from)
            .unwrap_or_else(|| format!("field#{}", field_id.get()));
        format!("{name}: ")
    } else {
        String::new()
    };

    let target_name = ctx
        .label_for(call.target)
        .map(String::from)
        .unwrap_or_else(|| format!("@{:0w$}", call.target.0, w = step_width));
    // Definition name in call is blue
    let content = format!("{field_part}({}{}{})", c.blue, target_name, c.reset);
    // Format as "target : return" with numeric IDs
    let successors = format!(
        "{:0w$} : {:0w$}",
        call.target.get(),
        call.next.get(),
        w = step_width
    );

    let base = format!("{prefix}{content}");
    builder.pad_successors(base, &successors)
}

fn format_return(
    step: u16,
    _r: &Return,
    _module: &Module,
    _ctx: &DumpContext,
    step_width: usize,
) -> String {
    let builder = LineBuilder::new(step_width);
    let prefix = format!(
        "  {:0sw$} {} ",
        step,
        Symbol::EMPTY.format(),
        sw = step_width
    );
    builder.pad_successors(prefix, "▶")
}

fn format_trampoline(step: u16, t: &Trampoline, _ctx: &DumpContext, step_width: usize) -> String {
    let builder = LineBuilder::new(step_width);
    let prefix = format!(
        "  {:0sw$} {} ",
        step,
        Symbol::EMPTY.format(),
        sw = step_width
    );
    let content = "Trampoline";
    let successors = format!("{:0w$}", t.next.get(), w = step_width);
    let base = format!("{prefix}{content}");
    builder.pad_successors(base, &successors)
}

/// Format a step ID, showing entrypoint label or numeric ID.
fn format_step(step: StepId, ctx: &DumpContext, step_width: usize) -> String {
    let c = &ctx.colors;
    if let Some(label) = ctx.label_for(step) {
        format!("▶({}{}{})", c.blue, label, c.reset)
    } else {
        format!("{:0w$}", step.get(), w = step_width)
    }
}
