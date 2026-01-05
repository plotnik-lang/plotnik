//! Human-readable bytecode dump for debugging and documentation.
//!
//! See `docs/binary-format/07-dump-format.md` for the output format specification.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::colors::Colors;

use super::NAMED_WILDCARD;
use super::format::{LineBuilder, Symbol, format_effect, nav_symbol_epsilon, width_for_count};
use super::ids::QTypeId;
use super::instructions::StepId;
use super::module::{Instruction, Module};
use super::type_meta::TypeKind;
use super::{Call, Match, Return, Trampoline};

/// Generate a human-readable dump of the bytecode module.
pub fn dump(module: &Module, colors: Colors) -> String {
    let mut out = String::new();
    let ctx = DumpContext::new(module, colors);

    dump_header(&mut out, module, &ctx);
    dump_strings(&mut out, module, &ctx);
    dump_types_defs(&mut out, module, &ctx);
    dump_types_members(&mut out, module, &ctx);
    dump_types_names(&mut out, module, &ctx);
    dump_entrypoints(&mut out, module, &ctx);
    dump_code(&mut out, module, &ctx);

    out
}

fn dump_header(out: &mut String, module: &Module, ctx: &DumpContext) {
    let c = &ctx.colors;
    let header = module.header();
    writeln!(out, "{}[flags]{}", c.blue, c.reset).unwrap();
    writeln!(out, "linked = {}", header.is_linked()).unwrap();
    out.push('\n');
}

/// Context for dump formatting, precomputes lookups for O(1) access.
struct DumpContext {
    /// Whether the bytecode is linked (contains grammar IDs vs StringIds).
    is_linked: bool,
    /// Maps step ID to entrypoint name for labeling.
    step_labels: BTreeMap<u16, String>,
    /// Maps node type ID to name (linked mode only).
    node_type_names: BTreeMap<u16, String>,
    /// Maps node field ID to name (linked mode only).
    node_field_names: BTreeMap<u16, String>,
    /// All strings (for unlinked mode lookups).
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
        let is_linked = header.is_linked();
        let strings = module.strings();
        let entrypoints = module.entrypoints();
        let node_types = module.node_types();
        let node_fields = module.node_fields();

        let mut step_labels = BTreeMap::new();
        // Preamble always at step 0 (first in layout)
        step_labels.insert(0, "_ObjWrap".to_string());
        for i in 0..entrypoints.len() {
            let ep = entrypoints.get(i);
            let name = strings.get(ep.name).to_string();
            step_labels.insert(ep.target, name);
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
            is_linked,
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
    ///
    /// In linked mode, this looks up the grammar's node type symbol table.
    /// In unlinked mode, this looks up the StringId from the strings table.
    fn node_type_name(&self, id: u16) -> Option<&str> {
        if self.is_linked {
            self.node_type_names.get(&id).map(|s| s.as_str())
        } else {
            // In unlinked mode, id is a StringId
            self.all_strings.get(id as usize).map(|s| s.as_str())
        }
    }

    /// Get the name for a node field ID.
    ///
    /// In linked mode, this looks up the grammar's node field symbol table.
    /// In unlinked mode, this looks up the StringId from the strings table.
    fn node_field_name(&self, id: u16) -> Option<&str> {
        if self.is_linked {
            self.node_field_names.get(&id).map(|s| s.as_str())
        } else {
            // In unlinked mode, id is a StringId
            self.all_strings.get(id as usize).map(|s| s.as_str())
        }
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
        let kind = def.type_kind().expect("valid type kind");

        let formatted = match kind {
            // Primitive types
            TypeKind::Void => "<Void>".to_string(),
            TypeKind::Node => "<Node>".to_string(),
            TypeKind::String => "<String>".to_string(),
            // Composite types
            TypeKind::Struct => format!("Struct  M{:0mw$}:{}", def.data, def.count),
            TypeKind::Enum => format!("Enum    M{:0mw$}:{}", def.data, def.count),
            // Wrapper types
            TypeKind::Optional => format!("Optional(T{:0tw$})", def.data),
            TypeKind::ArrayZeroOrMore => format!("ArrayStar(T{:0tw$})", def.data),
            TypeKind::ArrayOneOrMore => format!("ArrayPlus(T{:0tw$})", def.data),
            TypeKind::Alias => format!("Alias(T{:0tw$})", def.data),
        };

        // Generate comment for non-primitives (comments are dim)
        let comment = match kind {
            TypeKind::Void | TypeKind::Node | TypeKind::String => String::new(),
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
            TypeKind::Optional => {
                let inner_name = format_type_name(QTypeId(def.data), module, ctx);
                format!("{}  ; {}?{}", c.dim, inner_name, c.reset)
            }
            TypeKind::ArrayZeroOrMore => {
                let inner_name = format_type_name(QTypeId(def.data), module, ctx);
                format!("{}  ; {}*{}", c.dim, inner_name, c.reset)
            }
            TypeKind::ArrayOneOrMore => {
                let inner_name = format_type_name(QTypeId(def.data), module, ctx);
                format!("{}  ; {}+{}", c.dim, inner_name, c.reset)
            }
            TypeKind::Alias => String::new(),
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
fn format_type_name(type_id: QTypeId, module: &Module, ctx: &DumpContext) -> String {
    let types = module.types();
    let strings = module.strings();

    // Check if it's a primitive type
    if let Some(def) = types.get(type_id)
        && let Some(kind) = def.type_kind()
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
            let name = strings.get(ep.name);
            (name, ep.target, ep.result_type.0)
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

        let instr = module.decode_step_alloc(step);
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
            let slots = m.pre_effects.len()
                + m.neg_fields.len()
                + m.post_effects.len()
                + m.successors.len();

            if m.pre_effects.is_empty()
                && m.neg_fields.is_empty()
                && m.post_effects.is_empty()
                && m.successors.len() <= 1
            {
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
    _module: &Module,
    ctx: &DumpContext,
    step_width: usize,
) -> String {
    let builder = LineBuilder::new(step_width);
    let symbol = nav_symbol_epsilon(m.nav, m.is_epsilon());
    let prefix = format!("  {:0sw$} {} ", step, symbol.format(), sw = step_width);

    let content = format_match_content(m, ctx);
    let successors = format_successors(&m.successors, ctx, step_width);

    let base = format!("{prefix}{content}");
    builder.pad_successors(base, &successors)
}

/// Format Match instruction content (effects, node pattern, etc.)
fn format_match_content(m: &Match, ctx: &DumpContext) -> String {
    let mut parts = Vec::new();

    // Pre-effects
    if !m.pre_effects.is_empty() {
        let effects: Vec<_> = m.pre_effects.iter().map(format_effect).collect();
        parts.push(format!("[{}]", effects.join(" ")));
    }

    // Negated fields
    for &field_id in &m.neg_fields {
        let name = ctx
            .node_field_name(field_id)
            .map(String::from)
            .unwrap_or_else(|| format!("field#{field_id}"));
        parts.push(format!("-{name}"));
    }

    // Field constraint and node type
    let node_part = format_node_pattern(m, ctx);
    if !node_part.is_empty() {
        parts.push(node_part);
    }

    // Post-effects
    if !m.post_effects.is_empty() {
        let effects: Vec<_> = m.post_effects.iter().map(format_effect).collect();
        parts.push(format!("[{}]", effects.join(" ")));
    }

    parts.join(" ")
}

/// Format node pattern: `field: (type)` or `(type)` or `field: _` or `(_)`
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

    if let Some(type_id) = m.node_type {
        if type_id.get() == NAMED_WILDCARD {
            // Named wildcard: any named node
            result.push_str("(_)");
        } else {
            let name = ctx
                .node_type_name(type_id.get())
                .map(String::from)
                .unwrap_or_else(|| format!("node#{}", type_id.get()));
            result.push('(');
            result.push_str(&name);
            result.push(')');
        }
    } else if m.node_field.is_some() {
        result.push('_');
    }

    result
}

/// Format successors list or terminal symbol.
fn format_successors(successors: &[StepId], ctx: &DumpContext, step_width: usize) -> String {
    if successors.is_empty() {
        "◼".to_string()
    } else {
        successors
            .iter()
            .map(|s| format_step(*s, ctx, step_width))
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
    let symbol = nav_symbol_epsilon(call.nav, false);
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
