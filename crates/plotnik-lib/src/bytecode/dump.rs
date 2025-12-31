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
    dump_strings(&mut out, module);
    dump_types_defs(&mut out, module, &ctx);
    dump_types_members(&mut out, module, &ctx);
    dump_types_names(&mut out, module, &ctx);
    dump_entrypoints(&mut out, module, &ctx);
    dump_code(&mut out, module, &ctx);

    out
}

fn dump_header(out: &mut String, module: &Module) {
    let header = module.header();
    out.push_str("[header]\n");
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
    /// Entrypoint names by index (for Return formatting).
    entrypoint_names: Vec<String>,
    /// All strings (for unlinked mode lookups).
    all_strings: Vec<String>,
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
        let mut entrypoint_names = Vec::with_capacity(entrypoints.len());
        for i in 0..entrypoints.len() {
            let ep = entrypoints.get(i);
            let name = strings.get(ep.name).to_string();
            step_labels.insert(ep.target.0, name.clone());
            entrypoint_names.push(name);
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

        Self {
            is_linked,
            step_labels,
            node_type_names,
            node_field_names,
            entrypoint_names,
            all_strings,
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

    fn entrypoint_name(&self, index: usize) -> Option<&str> {
        self.entrypoint_names.get(index).map(|s| s.as_str())
    }
}

fn dump_strings(out: &mut String, module: &Module) {
    let strings = module.strings();
    let count = module.header().str_table_count as usize;

    out.push_str("[strings]\n");
    for i in 0..count {
        let s = strings.get(StringId(i as u16));
        writeln!(out, "S{i:02} {s:?}").unwrap();
    }
    out.push('\n');
}

fn dump_types_defs(out: &mut String, module: &Module, ctx: &DumpContext) {
    let types = module.types();
    let strings = module.strings();

    out.push_str("[types.defs]\n");

    // Builtins (T00-T02)
    out.push_str("T00 = void\n");
    out.push_str("T01 = Node\n");
    out.push_str("T02 = str\n");

    // Custom types (T03+)
    for i in 0..types.defs_count() {
        let def = types.get_def(i);
        let type_id = i + 3; // Custom types start at index 3

        let kind = def.type_kind().expect("valid type kind");
        let formatted = match kind {
            TypeKind::Struct => format!("Struct(M{}, {})", def.data, def.count),
            TypeKind::Enum => format!("Enum(M{}, {})", def.data, def.count),
            TypeKind::Optional => format!("Optional(T{:02})", def.data),
            TypeKind::ArrayZeroOrMore => format!("ArrayStar(T{:02})", def.data),
            TypeKind::ArrayOneOrMore => format!("ArrayPlus(T{:02})", def.data),
            TypeKind::Alias => format!("Alias(T{:02})", def.data),
        };

        // Generate comment for composites
        let comment = match kind {
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

        writeln!(out, "T{type_id:02} = {formatted}{comment}").unwrap();
    }
    out.push('\n');
}

fn dump_types_members(out: &mut String, module: &Module, ctx: &DumpContext) {
    let types = module.types();
    let strings = module.strings();

    out.push_str("[types.members]\n");
    for i in 0..types.members_count() {
        let member = types.get_member(i);
        let name = strings.get(member.name);
        let type_name = format_type_name(member.type_id, module, ctx);
        writeln!(
            out,
            "M{i} = (S{:02}, T{:02})  ; {name}: {type_name}",
            member.name.0, member.type_id.0
        )
        .unwrap();
    }
    out.push('\n');
}

fn dump_types_names(out: &mut String, module: &Module, _ctx: &DumpContext) {
    let types = module.types();
    let strings = module.strings();

    out.push_str("[types.names]\n");
    for i in 0..types.names_count() {
        let entry = types.get_name(i);
        let name = strings.get(entry.name);
        writeln!(
            out,
            "N{i} = (S{:02}, T{:02})  ; {name}",
            entry.name.0, entry.type_id.0
        )
        .unwrap();
    }
    out.push('\n');
}

/// Format a type ID as a human-readable name.
fn format_type_name(type_id: QTypeId, module: &Module, _ctx: &DumpContext) -> String {
    if type_id.is_builtin() {
        return match type_id.0 {
            0 => "void".to_string(),
            1 => "Node".to_string(),
            2 => "str".to_string(),
            _ => unreachable!(),
        };
    }

    // Try to find a name in types.names
    let types = module.types();
    let strings = module.strings();

    for i in 0..types.names_count() {
        let entry = types.get_name(i);
        if entry.type_id == type_id {
            return strings.get(entry.name).to_string();
        }
    }

    // Fall back to T## format
    format!("T{:02}", type_id.0)
}

fn dump_entrypoints(out: &mut String, module: &Module, _ctx: &DumpContext) {
    let strings = module.strings();
    let entrypoints = module.entrypoints();

    out.push_str("[entry]\n");

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
            "{name:width$} = {:02} :: T{type_id:02}",
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

    // Calculate step number width based on total steps
    let step_width = if transitions_count == 0 {
        2
    } else {
        ((transitions_count as f64).log10().floor() as usize + 1).max(2)
    };

    out.push_str("[code]\n");

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
fn pad_to_column(base: String, col: usize, suffix: &str) -> String {
    let padding = col.saturating_sub(base.chars().count());
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

fn format_match(
    step: u16,
    m: &Match,
    _module: &Module,
    ctx: &DumpContext,
    step_width: usize,
) -> String {
    // Nav column: 7 chars total for content alignment
    // 𝜀 (epsilon) centered for Stay, others left-aligned with 2-char gap
    let nav_col = if m.nav == Nav::Stay {
        "   𝜀   ".to_string()
    } else {
        let sym = format_nav(m.nav);
        let sym_len = sym.chars().count();
        let gap2 = 7usize.saturating_sub(2 + sym_len).max(1);
        format!("  {sym}{:gap2$}", "")
    };

    let mut content_parts = Vec::new();

    // Pre-effects
    if !m.pre_effects.is_empty() {
        let effects: Vec<_> = m.pre_effects.iter().map(format_effect).collect();
        content_parts.push(format!("[{}]", effects.join(" ")));
    }

    // Negated fields
    for &field_id in &m.neg_fields {
        let name = ctx
            .node_field_name(field_id)
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("field#{field_id}"));
        content_parts.push(format!("!{name}"));
    }

    // Field constraint and node type
    let mut node_part = String::new();

    if let Some(field_id) = m.node_field {
        let name = ctx
            .node_field_name(field_id.get())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("field#{}", field_id.get()));
        node_part.push_str(&name);
        node_part.push_str(": ");
    }

    if let Some(type_id) = m.node_type {
        let name = ctx
            .node_type_name(type_id.get())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("node#{}", type_id.get()));
        node_part.push('(');
        node_part.push_str(&name);
        node_part.push(')');
    } else if m.node_field.is_some() {
        node_part.push('_');
    }

    if !node_part.is_empty() {
        content_parts.push(node_part);
    }

    // Post-effects
    if !m.post_effects.is_empty() {
        let effects: Vec<_> = m.post_effects.iter().map(format_effect).collect();
        content_parts.push(format!("[{}]", effects.join(" ")));
    }

    // Successors
    let succ_str = if m.successors.is_empty() {
        "◼".to_string()
    } else {
        m.successors
            .iter()
            .map(|s| format_step(*s, ctx, step_width))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let content = content_parts.join(" ");
    let base = if content.is_empty() {
        format!("  {:0sw$}{nav_col}", step, sw = step_width)
    } else {
        format!("  {:0sw$}{nav_col}{content}", step, sw = step_width)
    };
    pad_to_column(base, 44, &succ_str)
}

fn format_call(
    step: u16,
    c: &Call,
    _module: &Module,
    ctx: &DumpContext,
    step_width: usize,
) -> String {
    let target_name = ctx
        .label_for(c.target)
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("@{:0w$}", c.target.0, w = step_width));

    let base = format!("  {:0w$}   ▶   ({target_name})", step, w = step_width);
    pad_to_column(base, 44, &format_step(c.next, ctx, step_width))
}

fn format_return(
    step: u16,
    r: &Return,
    _module: &Module,
    ctx: &DumpContext,
    step_width: usize,
) -> String {
    let name = ctx
        .entrypoint_name(r.ref_id as usize)
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("ref#{}", r.ref_id));

    let base = format!("  {:0w$}      ({name})", step, w = step_width);
    pad_to_column(base, 44, "▶")
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

/// Format navigation symbol. Called only for non-Stay navigation.
fn format_nav(nav: Nav) -> String {
    match nav {
        Nav::Stay => unreachable!("Stay is handled specially in format_match"),
        Nav::Down => "*↓".to_string(),
        Nav::DownSkip => "~↓".to_string(),
        Nav::DownExact => ".↓".to_string(),
        Nav::Next => "* ".to_string(),
        Nav::NextSkip => "~ ".to_string(),
        Nav::NextExact => ". ".to_string(),
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
        EffectOpcode::A => "A".to_string(),
        EffectOpcode::Push => "Push".to_string(),
        EffectOpcode::EndA => "EndA".to_string(),
        EffectOpcode::S => "S".to_string(),
        EffectOpcode::EndS => "EndS".to_string(),
        EffectOpcode::Set => format!("Set(M{})", effect.payload),
        EffectOpcode::E => format!("E(M{})", effect.payload),
        EffectOpcode::EndE => "EndE".to_string(),
        EffectOpcode::Text => "Text".to_string(),
        EffectOpcode::Clear => "Clear".to_string(),
        EffectOpcode::Null => "Null".to_string(),
    }
}
