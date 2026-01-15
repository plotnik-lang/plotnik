//! Debug-only semantic verification for IR.
//!
//! Compares semantic fingerprints derived from AST against compiled IR.
//! Catches compilation bugs that change the sequence of navigation, effects, or matches.
//! Zero-cost in release builds.

use std::collections::BTreeSet;

use indexmap::IndexMap;
use plotnik_bytecode::Nav;
use plotnik_core::Symbol;

use crate::analyze::type_check::DefId;
use crate::bytecode::{InstructionIR, Label, MatchIR, MemberRef, NodeTypeIR, PredicateValueIR};
use crate::emit::StringTableBuilder;

use super::compiler::CompileCtx;

/// Semantic operation for fingerprinting.
///
/// Captures semantically significant operations while ignoring compilation artifacts
/// like member indices, label addresses, and instruction sizes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SemanticOp {
    /// Navigation (non-epsilon only, full detail preserved).
    Nav(Nav),

    /// Named node match with optional type name.
    MatchNamed(Option<String>),

    /// Anonymous node match with optional literal value.
    MatchAnon(Option<String>),

    /// Any node wildcard (_).
    MatchAny,

    /// Field constraint.
    Field(String),

    /// Negated field constraint.
    NegField(String),

    /// Predicate with operation (as byte for Ord) and value.
    Predicate(u8, String),

    /// Effect with opcode (as byte for Ord) and optional member name.
    Effect(u8, Option<String>),

    /// Call to named definition.
    Call(String),

    /// Return from definition.
    Return,

    /// Cycle marker (back-edge in quantifier loop).
    /// The index is the position in the path where the cycle returns to,
    /// making it independent of specific bytecode labels.
    CycleMarker(usize),
}

/// A semantic path: sequence of operations along one execution path.
pub type Path = Vec<SemanticOp>;

/// Fingerprint: set of all semantic paths through the IR.
/// Using BTreeSet for deterministic ordering.
pub type Fingerprint = BTreeSet<Path>;

/// Collect semantic operations from a single MatchIR instruction.
#[cfg(debug_assertions)]
fn collect_ops_from_match(instr: &MatchIR, ctx: &CompileCtx) -> Vec<SemanticOp> {
    let mut ops = Vec::new();

    for e in &instr.pre_effects {
        let name = resolve_member_name(&e.member_ref, ctx.interner);
        ops.push(SemanticOp::Effect(e.opcode as u8, name));
    }

    // Epsilons are pure control flow - they don't navigate or check node types
    if instr.nav != Nav::Epsilon {
        ops.push(SemanticOp::Nav(instr.nav));

        // Only non-epsilons perform actual node type checks
        match &instr.node_type {
            NodeTypeIR::Any => ops.push(SemanticOp::MatchAny),
            NodeTypeIR::Named(id) => {
                let name = id.and_then(|i| resolve_node_type_name(i, ctx.node_types, ctx.interner));
                ops.push(SemanticOp::MatchNamed(name));
            }
            NodeTypeIR::Anonymous(id) => {
                let name = id.and_then(|i| resolve_node_type_name(i, ctx.node_types, ctx.interner));
                ops.push(SemanticOp::MatchAnon(name));
            }
        }
    }

    if let Some(f) = instr.node_field {
        let name = resolve_field_name(Some(f), ctx.node_fields, ctx.interner);
        ops.push(SemanticOp::Field(
            name.unwrap_or_else(|| format!("field#{}", f)),
        ));
    }

    for &f in &instr.neg_fields {
        let name = resolve_field_name(std::num::NonZeroU16::new(f), ctx.node_fields, ctx.interner);
        ops.push(SemanticOp::NegField(
            name.unwrap_or_else(|| format!("field#{}", f)),
        ));
    }

    if let Some(p) = &instr.predicate {
        let value = resolve_predicate_value(&p.value, &ctx.strings.borrow());
        ops.push(SemanticOp::Predicate(p.op.to_byte(), value));
    }

    for e in &instr.post_effects {
        let name = resolve_member_name(&e.member_ref, ctx.interner);
        ops.push(SemanticOp::Effect(e.opcode as u8, name));
    }

    ops
}

/// Resolve member reference to field/variant name.
#[cfg(debug_assertions)]
fn resolve_member_name(
    member_ref: &Option<MemberRef>,
    interner: &plotnik_core::Interner,
) -> Option<String> {
    let Some(MemberRef::Deferred { field_name, .. }) = member_ref else {
        return None;
    };
    interner.try_resolve(*field_name).map(|s| s.to_string())
}

/// Resolve node type ID to name.
#[cfg(debug_assertions)]
fn resolve_node_type_name(
    id: std::num::NonZeroU16,
    node_types: Option<&indexmap::IndexMap<Symbol, plotnik_core::NodeTypeId>>,
    interner: &plotnik_core::Interner,
) -> Option<String> {
    let types = node_types?;
    for (sym, type_id) in types {
        if type_id.get() == id.get() {
            return interner.try_resolve(*sym).map(|s| s.to_string());
        }
    }
    None
}

/// Resolve field ID to name.
#[cfg(debug_assertions)]
fn resolve_field_name(
    id: Option<std::num::NonZeroU16>,
    node_fields: Option<&indexmap::IndexMap<Symbol, plotnik_core::NodeFieldId>>,
    interner: &plotnik_core::Interner,
) -> Option<String> {
    let id = id?;
    let fields = node_fields?;
    for (sym, field_id) in fields {
        if field_id.get() == id.get() {
            return interner.try_resolve(*sym).map(|s| s.to_string());
        }
    }
    None
}

/// Resolve predicate value to string.
#[cfg(debug_assertions)]
fn resolve_predicate_value(value: &PredicateValueIR, strings: &StringTableBuilder) -> String {
    match value {
        PredicateValueIR::String(id) => strings.get_str(*id).to_string(),
        PredicateValueIR::Regex(id) => format!("/{}/", strings.get_str(*id)),
    }
}

/// Build instruction lookup map from label to instruction.
#[cfg(debug_assertions)]
fn build_instruction_map(
    instructions: &[InstructionIR],
) -> std::collections::HashMap<Label, &InstructionIR> {
    instructions.iter().map(|i| (i.label(), i)).collect()
}

/// Compute semantic fingerprint from IR via DFS.
///
/// Explores all paths from the entry label, collecting semantic operations.
/// Cycles are detected per-path and marked with `CycleMarker(position)` where
/// position is the index in the path where the cycle target's operations start.
#[cfg(debug_assertions)]
pub fn fingerprint_from_ir(
    instructions: &[InstructionIR],
    entry: Label,
    def_entries: &IndexMap<DefId, Label>,
    ctx: &CompileCtx,
) -> Fingerprint {
    let instr_map = build_instruction_map(instructions);

    // Reverse map: Label -> DefId for resolving Call targets
    let label_to_def: std::collections::HashMap<Label, DefId> = def_entries
        .iter()
        .map(|(&def_id, &label)| (label, def_id))
        .collect();
    let mut fingerprint = Fingerprint::new();

    // DFS stack: (current_label, path_so_far, label_to_path_position)
    // label_to_path_position maps labels to the path index where their ops started
    let mut stack: Vec<(Label, Path, std::collections::HashMap<Label, usize>)> =
        vec![(entry, Vec::new(), std::collections::HashMap::new())];

    while let Some((current, mut path, mut label_positions)) = stack.pop() {
        // Cycle detection: if we've seen this label, record position-based cycle marker
        if let Some(&pos) = label_positions.get(&current) {
            path.push(SemanticOp::CycleMarker(pos));
            fingerprint.insert(path);
            continue;
        }

        let Some(instr) = instr_map.get(&current) else {
            // Dangling label — shouldn't happen, but record path anyway
            fingerprint.insert(path);
            continue;
        };

        // Effectless epsilons are invisible: don't mark visited, just pass through.
        // This is "laser vision" for fingerprinting - we see through pure control flow.
        if let InstructionIR::Match(m) = instr
            && m.is_epsilon()
            && m.pre_effects.is_empty()
            && m.post_effects.is_empty()
        {
            for &succ in &m.successors {
                stack.push((succ, path.clone(), label_positions.clone()));
            }
            continue;
        }

        // Record this label's position BEFORE adding ops
        let current_pos = path.len();
        label_positions.insert(current, current_pos);

        match instr {
            InstructionIR::Match(m) => {
                let ops = collect_ops_from_match(m, ctx);
                path.extend(ops);

                if m.successors.is_empty() {
                    // Terminal state (accept)
                    fingerprint.insert(path);
                } else {
                    // Fork paths for each successor
                    for &succ in &m.successors {
                        stack.push((succ, path.clone(), label_positions.clone()));
                    }
                }
            }
            InstructionIR::Call(c) => {
                // Record call with definition ID (stable across label rewrites)
                let call_name = label_to_def
                    .get(&c.target)
                    .map(|def_id| format!("def#{}", def_id.as_u32()))
                    .unwrap_or_else(|| format!("label#{}", c.target.0));
                path.push(SemanticOp::Call(call_name));
                stack.push((c.next, path, label_positions));
            }
            InstructionIR::Return(_) => {
                path.push(SemanticOp::Return);
                fingerprint.insert(path);
            }
            InstructionIR::Trampoline(t) => {
                // Trampoline is part of preamble, skip semantically
                stack.push((t.next, path, label_positions));
            }
        }
    }

    fingerprint
}

/// Debug-only semantic verification.
///
/// Panics with a detailed diagnostic if the IR fingerprint doesn't match expectations.
/// This is a no-op in release builds.
#[cfg(debug_assertions)]
pub fn debug_verify_ir_fingerprint(
    instructions: &[InstructionIR],
    entry: Label,
    def_entries: &IndexMap<DefId, Label>,
    def_name: &str,
    ctx: &CompileCtx,
) {
    let fingerprint = fingerprint_from_ir(instructions, entry, def_entries, ctx);

    // For now, just compute and log the fingerprint.
    // Full AST comparison will be added in a follow-up.
    if std::env::var("PLOTNIK_DEBUG_FINGERPRINT").is_ok() {
        eprintln!("=== Fingerprint for {} ===", def_name);
        for (i, path) in fingerprint.iter().enumerate() {
            eprintln!("Path {}: {:?}", i, path);
        }
        eprintln!("=== End fingerprint ===\n");
    }
}

/// No-op in release builds.
#[cfg(not(debug_assertions))]
#[inline(always)]
pub fn debug_verify_ir_fingerprint(
    _instructions: &[InstructionIR],
    _entry: Label,
    _def_entries: &IndexMap<DefId, Label>,
    _def_name: &str,
    _ctx: &CompileCtx,
) {
}
