//! Human-readable bytecode dump for debugging and documentation.
//!
//! See `docs/wip/bytecode.md` for the output format specification.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::effects::EffectOpcode;
use super::ids::{QTypeId, StepId, StringId};
use super::module::{Instruction, Module};
use super::nav::Nav;
use super::type_meta::TypeKind;
use super::{Call, Match, Return};

/// Generate a human-readable dump of the bytecode module.
pub fn dump(module: &Module) -> String {
    let mut out = String::new();
    let ctx = DumpContext::new(module);

    dump_header(&mut out, module);
    dump_strings(&mut out, module, &ctx);
    dump_types_defs(&mut out, module, &ctx);
    dump_types_members(&mut out, module, &ctx);
    dump_types_names(&mut out, module, &ctx);
    dump_entrypoints(&mut out, module, &ctx);
    dump_code(&mut out, module, &ctx);

    out
}

fn dump_header(out: &mut String, module: &Module) {
    let header = module.header();
    out.push_str("[flags]\n");
    writeln!(out, "linked = {}", header.is_linked()).unwrap();
    out.push('\n');
}

/// Calculate the minimum width needed to display numbers up to `count - 1`.
fn width_for_count(count: usize) -> usize {
    if count <= 1 {
        1
    } else {
        ((count - 1) as f64).log10().floor() as usize + 1
    }
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
}

impl DumpContext {
    fn new(module: &Module) -> Self {
        let header = module.header();
        let is_linked = header.is_linked();
        let strings = module.strings();
        let entrypoints = module.entrypoints();
        let node_types = module.node_types();
        let node_fields = module.node_fields();

        let mut step_labels = BTreeMap::new();
        for i in 0..entrypoints.len() {
            let ep = entrypoints.get(i);
            let name = strings.get(ep.name).to_string();
            step_labels.insert(ep.target.0, name);
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
            .map(|i| strings.get(StringId(i as u16)).to_string())
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
        }
    }

    fn label_for(&self, step: StepId) -> Option<&str> {
        self.step_labels.get(&step.0).map(|s| s.as_str())
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
    let strings = module.strings();
    let count = module.header().str_table_count as usize;
    let w = ctx.str_width;

    out.push_str("[strings]\n");
    for i in 0..count {
        let s = strings.get(StringId(i as u16));
        writeln!(out, "S{i:0w$} {s:?}").unwrap();
    }
    out.push('\n');
}

fn dump_types_defs(out: &mut String, module: &Module, ctx: &DumpContext) {
    let types = module.types();
    let strings = module.strings();
    let tw = ctx.type_width;
    let mw = ctx.member_width;

    out.push_str("[type_defs]\n");

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

        // Generate comment for non-primitives
        let comment = match kind {
            TypeKind::Void | TypeKind::Node | TypeKind::String => String::new(),
            TypeKind::Struct => {
                let fields: Vec<_> = types
                    .members_of(&def)
                    .map(|m| strings.get(m.name).to_string())
                    .collect();
                format!("  ; {{ {} }}", fields.join(", "))
            }
            TypeKind::Enum => {
                let variants: Vec<_> = types
                    .members_of(&def)
                    .map(|m| strings.get(m.name).to_string())
                    .collect();
                format!("  ; {}", variants.join(" | "))
            }
            TypeKind::Optional => {
                let inner_name = format_type_name(QTypeId(def.data), module, ctx);
                format!("  ; {}?", inner_name)
            }
            TypeKind::ArrayZeroOrMore => {
                let inner_name = format_type_name(QTypeId(def.data), module, ctx);
                format!("  ; {}*", inner_name)
            }
            TypeKind::ArrayOneOrMore => {
                let inner_name = format_type_name(QTypeId(def.data), module, ctx);
                format!("  ; {}+", inner_name)
            }
            TypeKind::Alias => String::new(),
        };

        writeln!(out, "T{i:0tw$} = {formatted}{comment}").unwrap();
    }
    out.push('\n');
}

fn dump_types_members(out: &mut String, module: &Module, ctx: &DumpContext) {
    let types = module.types();
    let strings = module.strings();
    let mw = ctx.member_width;
    let sw = ctx.str_width;
    let tw = ctx.type_width;

    out.push_str("[type_members]\n");
    for i in 0..types.members_count() {
        let member = types.get_member(i);
        let name = strings.get(member.name);
        let type_name = format_type_name(member.type_id, module, ctx);
        writeln!(
            out,
            "M{i:0mw$}: S{:0sw$} → T{:0tw$}  ; {name}: {type_name}",
            member.name.0, member.type_id.0
        )
        .unwrap();
    }
    out.push('\n');
}

fn dump_types_names(out: &mut String, module: &Module, ctx: &DumpContext) {
    let types = module.types();
    let strings = module.strings();
    let nw = ctx.name_width;
    let sw = ctx.str_width;
    let tw = ctx.type_width;

    out.push_str("[type_names]\n");
    for i in 0..types.names_count() {
        let entry = types.get_name(i);
        let name = strings.get(entry.name);
        writeln!(
            out,
            "N{i:0nw$}: S{:0sw$} → T{:0tw$}  ; {name}",
            entry.name.0, entry.type_id.0
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
    let strings = module.strings();
    let entrypoints = module.entrypoints();
    let stw = ctx.step_width;
    let tw = ctx.type_width;

    out.push_str("[entrypoints]\n");

    // Collect and sort by name for display
    let mut entries: Vec<_> = (0..entrypoints.len())
        .map(|i| {
            let ep = entrypoints.get(i);
            let name = strings.get(ep.name);
            (name, ep.target.0, ep.result_type.0)
        })
        .collect();
    entries.sort_by_key(|(name, _, _)| *name);

    // Find max name length for alignment
    let max_len = entries.iter().map(|(n, _, _)| n.len()).max().unwrap_or(0);

    for (name, target, type_id) in entries {
        writeln!(
            out,
            "{name:width$} = {:0stw$} :: T{type_id:0tw$}",
            target,
            width = max_len
        )
        .unwrap();
    }
    out.push('\n');
}

fn dump_code(out: &mut String, module: &Module, ctx: &DumpContext) {
    let header = module.header();
    let transitions_count = header.transitions_count as usize;
    let step_width = ctx.step_width;

    out.push_str("[transitions]\n");

    let mut step = 0u16;
    while (step as usize) < transitions_count {
        // Check if this step has a label
        if let Some(label) = ctx.label_for(StepId(step)) {
            writeln!(out, "\n{label}:").unwrap();
        }

        let instr = module.decode_step(StepId(step));
        let line = format_instruction(step, &instr, module, ctx, step_width);
        out.push_str(&line);
        out.push('\n');

        // Advance by instruction size
        let size = instruction_step_count(&instr);
        step += size;
    }
}

/// Pad a base string to a target column width, then append a suffix.
/// Ensures at least 2 spaces between base and suffix.
fn pad_to_column(base: String, col: usize, suffix: &str) -> String {
    let padding = col.saturating_sub(base.chars().count()).max(2);
    format!("{base}{:padding$}{suffix}", "")
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
        Instruction::Call(_) | Instruction::Return(_) => 1,
    }
}

// =============================================================================
// Instruction Line Format
// =============================================================================
//
// Each instruction line follows this column layout:
//
//   <indent><step><gap><nav><marker><content>...<successors>
//   ├──────┤├───┤├─┤├──┤├────┤├──────────────┤├──────────┤
//      2     var  1   3   3       variable      pad to 44
//
// - indent:  2 spaces
// - step:    step number, zero-padded to `step_width`
// - gap:     1 space
// - nav:     3-char navigation symbol (↓*, *↑¹, etc.) or 𝜀 for Stay
// - marker:  3-char marker column (" ▶ " for Call, "   " otherwise)
// - content: variable-width instruction content
// - successors: right-aligned at column 44
//
// =============================================================================

/// Column widths for instruction formatting.
#[allow(dead_code)]
mod cols {
    pub const INDENT: usize = 2;
    pub const GAP: usize = 1;
    pub const NAV: usize = 3;
    pub const MARKER: usize = 3;
    pub const TOTAL_WIDTH: usize = 44;
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
    }
}

/// Build instruction line prefix: `  <step>  <nav>`
///
/// The `is_epsilon` flag controls whether to display `𝜀` for the navigation column.
/// True epsilon transitions require all three conditions:
/// - `nav == Stay` (no cursor movement)
/// - `node_type == None` (no type constraint)
/// - `node_field == None` (no field constraint)
///
/// A Match with `nav == Stay` but type/field constraints is NOT epsilon—it matches
/// at the current position. Only true epsilon transitions display the `𝜀` symbol.
fn line_prefix(step: u16, nav: Nav, is_epsilon: bool, step_width: usize) -> String {
    let nav_str = if is_epsilon {
        " 𝜀 ".to_string()
    } else {
        format_nav(nav)
    };
    format!(
        "{:indent$}{:0sw$}{:gap$}{nav_str}",
        "",
        step,
        "",
        indent = cols::INDENT,
        sw = step_width,
        gap = cols::GAP,
    )
}

fn format_match(
    step: u16,
    m: &Match,
    _module: &Module,
    ctx: &DumpContext,
    step_width: usize,
) -> String {
    let prefix = line_prefix(step, m.nav, m.is_epsilon(), step_width);
    let marker = "   "; // No marker for Match

    let content = format_match_content(m, ctx);
    let successors = format_successors(&m.successors, ctx, step_width);

    let base = format!("{prefix}{marker}{content}");
    pad_to_column(base, cols::TOTAL_WIDTH, &successors)
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
        parts.push(format!("!{name}"));
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

/// Format node pattern: `field: (type)` or `(type)` or `field: _`
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
        let name = ctx
            .node_type_name(type_id.get())
            .map(String::from)
            .unwrap_or_else(|| format!("node#{}", type_id.get()));
        result.push('(');
        result.push_str(&name);
        result.push(')');
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
    c: &Call,
    _module: &Module,
    ctx: &DumpContext,
    step_width: usize,
) -> String {
    // Call is never epsilon—it always invokes a target definition
    let prefix = line_prefix(step, c.nav, false, step_width);
    let marker = " ▶ "; // Call marker (centered)

    // Format field constraint if present
    let field_part = if let Some(field_id) = c.node_field {
        let name = ctx
            .node_field_name(field_id.get())
            .map(String::from)
            .unwrap_or_else(|| format!("field#{}", field_id.get()));
        format!("{name}: ")
    } else {
        String::new()
    };

    let target_name = ctx
        .label_for(c.target)
        .map(String::from)
        .unwrap_or_else(|| format!("@{:0w$}", c.target.0, w = step_width));
    let content = format!("{field_part}({target_name})");
    let successors = format_step(c.next, ctx, step_width);

    let base = format!("{prefix}{marker}{content}");
    pad_to_column(base, cols::TOTAL_WIDTH, &successors)
}

fn format_return(
    step: u16,
    _r: &Return,
    _module: &Module,
    _ctx: &DumpContext,
    step_width: usize,
) -> String {
    // Return is never epsilon—it's a control flow instruction, not a match
    let prefix = line_prefix(step, Nav::Stay, false, step_width);
    // Return just shows the return marker - context makes the definition clear
    let base = prefix.to_string();
    pad_to_column(base, cols::TOTAL_WIDTH, "▶")
}

/// Format a step ID, showing entrypoint label or numeric ID.
fn format_step(step: StepId, ctx: &DumpContext, step_width: usize) -> String {
    if step == StepId::ACCEPT {
        return "◼".to_string();
    }
    if let Some(label) = ctx.label_for(step) {
        format!("▶({label})")
    } else {
        format!("{:0w$}", step.0, w = step_width)
    }
}

/// Format navigation symbol as exactly 3 chars (except multi-digit Up levels).
fn format_nav(nav: Nav) -> String {
    match nav {
        // Stay: 3 spaces (no movement). The 𝜀 symbol is handled separately
        // for true epsilon transitions (Stay + no type + no field).
        Nav::Stay => "   ".to_string(),
        // Down: space + arrow + modifier
        Nav::Down => " ↓*".to_string(),
        Nav::DownSkip => " ↓~".to_string(),
        Nav::DownExact => " ↓.".to_string(),
        // Next: centered modifier (no arrow)
        Nav::Next => " * ".to_string(),
        Nav::NextSkip => " ~ ".to_string(),
        Nav::NextExact => " . ".to_string(),
        // Up: modifier + arrow + superscript level
        Nav::Up(n) => format!("*↑{}", superscript(n)),
        Nav::UpSkipTrivia(n) => format!("~↑{}", superscript(n)),
        Nav::UpExact(n) => format!(".↑{}", superscript(n)),
    }
}

fn superscript(n: u8) -> String {
    const DIGITS: &[char] = &['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
    if n < 10 {
        DIGITS[n as usize].to_string()
    } else {
        n.to_string()
            .chars()
            .map(|c| DIGITS[c.to_digit(10).unwrap() as usize])
            .collect()
    }
}

fn format_effect(effect: &super::EffectOp) -> String {
    match effect.opcode {
        EffectOpcode::Node => "Node".to_string(),
        EffectOpcode::Arr => "Arr".to_string(),
        EffectOpcode::Push => "Push".to_string(),
        EffectOpcode::EndArr => "EndArr".to_string(),
        EffectOpcode::Obj => "Obj".to_string(),
        EffectOpcode::EndObj => "EndObj".to_string(),
        EffectOpcode::Set => format!("Set(M{})", effect.payload),
        EffectOpcode::Enum => format!("Enum(M{})", effect.payload),
        EffectOpcode::EndEnum => "EndEnum".to_string(),
        EffectOpcode::Text => "Text".to_string(),
        EffectOpcode::Clear => "Clear".to_string(),
        EffectOpcode::Null => "Null".to_string(),
    }
}
