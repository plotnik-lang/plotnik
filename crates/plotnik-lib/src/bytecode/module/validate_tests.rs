//! Module loader hardening.
//!
//! Each test emits a bytecode buffer, deliberately corrupts one field, and
//! recomputes the CRC so the checksum gate still passes. It then asserts that
//! [`Module::load_compiler_output`] returns a clean [`ModuleError`] before a
//! view, decoder, VM, or materializer can observe the buffer. Together these
//! tests preserve the mandatory checks between compiler emission and VM use.
//!
//! These live in-crate rather than under `tests/`: mutating exact byte offsets
//! reads `SectionOffsets`' `pub(crate)` fields (via [`Module::offsets`]), which an
//! external integration test cannot reach. Minting a real module needs the
//! compiler, which depends on this crate — a cycle that is fine through a
//! `[dev-dependencies]` edge, since it never enters the build graph. The test
//! helper links against a small synthetic grammar instead of a real language package.

use std::fmt::Write as _;

use crate::compiler::test_utils::synthetic_grammar as grammar;
use crate::compiler::{
    BytecodeConfig, BytecodeInspection, QueryBuilder, SourceMap, SourcePath,
    reset_semantic_body_analyses, semantic_body_analyses,
};
use indoc::indoc;

use super::effect_stack::{
    body_analyses as loader_body_analyses, reset_body_analyses as reset_loader_body_analyses,
};
use super::{ByteStorage, Module, ModuleError};
use crate::bytecode::effects::{EFFECT_PAYLOAD_BITS, EFFECT_PAYLOAD_MAX, EffectKind};
use crate::bytecode::type_meta::TypeDefKind;
use crate::bytecode::type_system::TypeKind;
use crate::bytecode::{
    BYTECODE_WORD_SIZE, CodeAddr, Header, Nav, SPAN_ENTRY_SIZE, SPAN_NO_BINDING, SpanEntry,
    SpanKind,
};

fn emit_bytes(query_src: &str) -> Vec<u8> {
    let mut source_map = SourceMap::new();
    source_map.add_file(SourcePath::new("query.ptk"), query_src);
    let compiled = QueryBuilder::new(source_map)
        .compile(grammar())
        .expect("query parsing should not exhaust fuel");
    assert!(
        compiled.is_valid(),
        "query should compile: {query_src}\n{}",
        compiled.diagnostics().render(compiled.source_map())
    );
    compiled
        .emit(BytecodeConfig::new())
        .expect("bytecode emission answers")
        .into_artifact()
        .expect("compiled query has bytecode")
        .bytes()
        .to_vec()
}

const SPLIT_CALL_QUERY: &str = indoc! {r#"
    Body = [ Rec: {(comment) @c (B)} Base: (comment) @c ]
    B = { (Body)?? @first (Body)? @second }
    Q = (program (B) @x :: str)
"#};

const ROUTED_CALL_QUERY: &str = indoc! {r#"
    A = (statement_block (A)+ (identifier))?
    Q = (program (A) (expression_statement) @e)
"#};

/// Recompute the CRC32 checked by the module loader so a tampered body
/// reaches the structural validators exercised by these tests.
fn reseal(bytes: &mut [u8]) {
    let crc = crc32fast::hash(&bytes[64..]);
    bytes[8..12].copy_from_slice(&crc.to_le_bytes());
}

const MANY_DEFINITIONS: usize = 4_096;
// Both verifiers currently need six body walks per source definition. Keep
// modest scheduling slack while making any definition-count rescan fail.
const MAX_BODY_ANALYSES_PER_DEFINITION: usize = 8;

fn assert_linear_body_analyses(stage: &str, analyses: usize) {
    assert!(
        analyses <= MANY_DEFINITIONS * MAX_BODY_ANALYSES_PER_DEFINITION,
        "{stage} performed {analyses} body analyses for {MANY_DEFINITIONS} definitions"
    );
}

#[test]
fn many_callable_definitions_load_without_global_fixpoint_rescans() {
    // Every selectable definition emits both a wrapper and a called body. This
    // shape used to spend quadratic time deduplicating roots and repeatedly
    // rescanning the whole definition set as body summaries became known.
    let mut query = String::new();
    for index in 0..MANY_DEFINITIONS {
        writeln!(query, "Query{index} = (identifier)").expect("writing to a string succeeds");
    }

    reset_semantic_body_analyses();
    reset_loader_body_analyses();
    let bytes = emit_bytes(&query);
    let semantic_analyses = semantic_body_analyses();
    let emission_load_analyses = loader_body_analyses();
    assert!(
        bytes.len() > 63 * 1_024,
        "regression module must stay large"
    );

    assert_linear_body_analyses("semantic verifier", semantic_analyses);
    assert_linear_body_analyses("emission loader", emission_load_analyses);

    reset_loader_body_analyses();
    let module = Module::load_compiler_output(&bytes).expect("compiler output validates");
    let reload_analyses = loader_body_analyses();
    assert_linear_body_analyses("module reload", reload_analyses);

    assert_eq!(module.entry_points().len(), MANY_DEFINITIONS);
}

/// Byte offset of the first predicated Match's 4-byte predicate
/// (`op_and_flags` u16 || `value_ref` u16) in the instruction stream.
fn find_predicate_off(bytes: &[u8]) -> usize {
    let (base, word_count) = {
        let m = Module::load_compiler_output(bytes).expect("module validates before tampering");
        (
            m.offsets().instructions as usize,
            m.header().instruction_word_count,
        )
    };
    let mut addr = CodeAddr::ZERO;
    while addr.get() < word_count {
        let instr = base + addr.as_usize() * BYTECODE_WORD_SIZE;
        let opcode = bytes[instr] & 0x0F;
        let size = match opcode {
            0 | 6 | 7 | 8 => 8,
            1 => 16,
            2 => 24,
            3 => 32,
            4 => 48,
            5 => 64,
            other => panic!("unexpected opcode {other}"),
        };
        if (1..=5).contains(&opcode) {
            let counts = u16::from_le_bytes([bytes[instr + 6], bytes[instr + 7]]);
            if (counts >> 3) & 1 != 0 {
                let effects = ((counts >> 12) & 0xF) as usize;
                let neg = ((counts >> 9) & 0x7) as usize;
                return instr + 8 + (effects + neg) * 2;
            }
        }
        addr = addr
            .checked_add((size / BYTECODE_WORD_SIZE) as u16)
            .expect("instruction address fits in u16");
    }
    panic!("query must emit a string predicate");
}

#[test]
fn forged_invalid_entry_point_name_is_rejected() {
    // `0` is the reserved easter-egg index (never a real reference) and `u16::MAX`
    // is past the table; both must yield a clean error, not panic during StringId decoding.
    for forged in [0u16, u16::MAX] {
        let mut bytes = emit_bytes(r#"Top = (identifier) @id"#);
        let ep_off = Module::load_compiler_output(&bytes)
            .expect("module validates before tampering")
            .offsets()
            .entry_points as usize;

        bytes[ep_off..ep_off + 2].copy_from_slice(&forged.to_le_bytes());
        reseal(&mut bytes);

        let err = Module::load_compiler_output(&bytes)
            .expect_err("forged entry-point name must be rejected");
        assert!(
            matches!(err, ModuleError::InvalidStringId(_)),
            "forged name {forged}: expected InvalidStringId, got {err:?}"
        );
    }
}

#[test]
fn forged_out_of_range_predicate_operand_is_rejected() {
    // The `== "needle"` predicate stores a string-table index as its value_ref.
    let mut bytes = emit_bytes(r#"Q = (identifier == "needle")"#);
    let str_count = Module::load_compiler_output(&bytes)
        .expect("module validates before tampering")
        .header()
        .str_table_count;

    let pred_off = find_predicate_off(&bytes);
    bytes[pred_off + 2..pred_off + 4].copy_from_slice(&str_count.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("forged predicate operand must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidPredicateOperand(_)),
        "expected InvalidPredicateOperand, got {err:?}"
    );
}

#[test]
fn forged_invalid_predicate_op_is_rejected() {
    let mut bytes = emit_bytes(r#"Q = (identifier == "needle")"#);

    // The op is the low byte of the predicate; `7` is not a valid PredicateOp and
    // would panic in PredicateOp::from_byte when the predicate is evaluated/dumped.
    let pred_off = find_predicate_off(&bytes);
    bytes[pred_off] = 7;
    reseal(&mut bytes);

    let err =
        Module::load_compiler_output(&bytes).expect_err("forged predicate op must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidPredicateOperand(_)),
        "expected InvalidPredicateOperand, got {err:?}"
    );
}

/// A record-producing query that emits extended Matches carrying `Node`/`RecordSet` effects and
/// successors — the shapes the per-instruction forging tests below target.
const RECORD_QUERY: &str =
    r#"Top = (binary_expression left: (identifier) @l right: (identifier) @r)"#;

fn instruction_section(bytes: &[u8]) -> (usize, u16) {
    let m = Module::load_compiler_output(bytes).expect("module validates before tampering");
    (
        m.offsets().instructions as usize,
        m.header().instruction_word_count,
    )
}

/// Byte size of an instruction from its opcode nibble (mirrors `Opcode::size`).
fn instr_size(opcode: u8) -> usize {
    match opcode {
        0 | 6 | 7 | 8 | 9 => 8,
        1 => 16,
        2 => 24,
        3 => 32,
        4 => 48,
        5 => 64,
        other => panic!("unexpected opcode {other}"),
    }
}

fn first_instr(bytes: &[u8], want: impl Fn(u8) -> bool) -> usize {
    let (base, word_count) = instruction_section(bytes);
    let mut addr = CodeAddr::ZERO;
    while addr.get() < word_count {
        let off = base + addr.as_usize() * BYTECODE_WORD_SIZE;
        let opcode = bytes[off] & 0x0F;
        if want(opcode) {
            return off;
        }
        addr = addr
            .checked_add((instr_size(opcode) / BYTECODE_WORD_SIZE) as u16)
            .expect("instruction address fits in u16");
    }
    panic!("no matching instruction in the instruction stream");
}

fn first_match_nav(bytes: &[u8], want: impl Fn(u8) -> bool) -> usize {
    let (base, word_count) = instruction_section(bytes);
    let mut addr = CodeAddr::ZERO;
    while addr.get() < word_count {
        let off = base + addr.as_usize() * BYTECODE_WORD_SIZE;
        let opcode = bytes[off] & 0x0F;
        if (0..=5).contains(&opcode) && want(bytes[off + 1]) {
            return off + 1;
        }
        addr = addr
            .checked_add((instr_size(opcode) / BYTECODE_WORD_SIZE) as u16)
            .expect("instruction address fits in u16");
    }
    panic!("no matching match nav in the instruction stream");
}

/// Byte offsets of every effect slot in the stream. Negated-field slots are
/// skipped: those are plain field ids, not decoded effects.
fn effect_slots(bytes: &[u8]) -> Vec<usize> {
    let (base, word_count) = instruction_section(bytes);
    let mut slots = Vec::new();
    let mut addr = CodeAddr::ZERO;
    while addr.get() < word_count {
        let off = base + addr.as_usize() * BYTECODE_WORD_SIZE;
        let opcode = bytes[off] & 0x0F;
        if (1..=5).contains(&opcode) {
            let counts = u16::from_le_bytes([bytes[off + 6], bytes[off + 7]]);
            let effects = ((counts >> 12) & 0xF) as usize;
            slots.extend((0..effects).map(|i| off + 8 + i * 2));
        }
        addr = addr
            .checked_add((instr_size(opcode) / BYTECODE_WORD_SIZE) as u16)
            .expect("instruction address fits in u16");
    }
    slots
}

/// Address of the first interior word of the first multi-word instruction.
fn first_multiword_interior_addr(bytes: &[u8]) -> CodeAddr {
    let (base, word_count) = instruction_section(bytes);
    let mut addr = CodeAddr::ZERO;
    while addr.get() < word_count {
        let off = base + addr.as_usize() * BYTECODE_WORD_SIZE;
        let words = (instr_size(bytes[off] & 0x0F) / BYTECODE_WORD_SIZE) as u16;
        if words > 1 {
            return addr.checked_add(1).expect("interior address fits in u16");
        }
        addr = addr
            .checked_add(words)
            .expect("instruction address fits in u16");
    }
    panic!("no multi-word instruction in the instruction stream");
}

fn first_ext_successor(bytes: &[u8]) -> usize {
    let (base, word_count) = instruction_section(bytes);
    let mut addr = CodeAddr::ZERO;
    while addr.get() < word_count {
        let off = base + addr.as_usize() * BYTECODE_WORD_SIZE;
        let opcode = bytes[off] & 0x0F;
        if (1..=5).contains(&opcode) {
            let counts = u16::from_le_bytes([bytes[off + 6], bytes[off + 7]]);
            let effects = ((counts >> 12) & 0xF) as usize;
            let neg = ((counts >> 9) & 0x7) as usize;
            let succ = ((counts >> 4) & 0x1F) as usize;
            let has_pred = (counts >> 3) & 1 != 0;
            if succ > 0 {
                return off + 8 + (effects + neg) * 2 + if has_pred { 4 } else { 0 };
            }
        }
        addr = addr
            .checked_add((instr_size(opcode) / BYTECODE_WORD_SIZE) as u16)
            .expect("instruction address fits in u16");
    }
    panic!("no extended-match successor in the instruction stream");
}

/// Byte offset of the first effect slot whose opcode satisfies `want`.
fn first_effect_op(bytes: &[u8], want: impl Fn(u16) -> bool) -> usize {
    effect_slots(bytes)
        .into_iter()
        .find(|&off| want(u16::from_le_bytes([bytes[off], bytes[off + 1]]) >> EFFECT_PAYLOAD_BITS))
        .expect("no matching effect slot in the instruction stream")
}

fn effect_word(kind: EffectKind) -> [u8; 2] {
    ((kind as u16) << EFFECT_PAYLOAD_BITS).to_le_bytes()
}

fn effect_word_with_payload(kind: EffectKind, payload: u16) -> [u8; 2] {
    (((kind as u16) << EFFECT_PAYLOAD_BITS) | payload).to_le_bytes()
}

fn effect_payload(bytes: &[u8], slot: usize) -> u16 {
    u16::from_le_bytes([bytes[slot], bytes[slot + 1]]) & EFFECT_PAYLOAD_MAX as u16
}

fn type_member_type_id_off(bytes: &[u8], member: u16) -> usize {
    let m = Module::load_compiler_output(bytes).expect("module validates before tampering");
    m.offsets().type_members as usize + member as usize * 4 + 2
}

/// Byte offset of the first non-empty inter-section alignment gap — the padding
/// the emitter zero-fills before each aligned section (or the final tail).
fn first_section_gap(bytes: &[u8]) -> usize {
    let m = Module::load_compiler_output(bytes).expect("module validates before tampering");
    let o = m.offsets();
    let h = m.header();
    // (section start, data length) in layout order, terminated by the buffer end.
    let sections = [
        (o.str_blob, h.str_blob_size),
        (o.regex_blob, h.regex_blob_size),
        (o.str_table, (h.str_table_count as u32 + 1) * 4),
        (o.regex_table, (h.regex_table_count as u32 + 1) * 8),
        (o.node_kinds, h.node_kinds_count as u32 * 4),
        (o.node_fields, h.node_fields_count as u32 * 4),
        (o.type_defs, h.type_defs_count as u32 * 4),
        (o.type_members, h.type_members_count as u32 * 4),
        (o.type_names, h.type_names_count as u32 * 4),
        (o.entry_points, h.entry_points_count as u32 * 8),
        (o.instructions, h.instruction_word_count as u32 * 8),
        (o.spans, h.spans_count as u32 * SPAN_ENTRY_SIZE as u32),
        (h.total_size, 0),
    ];
    sections
        .windows(2)
        .find_map(|w| {
            let end = w[0].0 + w[0].1;
            (end < w[1].0).then_some(end as usize)
        })
        .expect("query must leave at least one alignment gap")
}

#[test]
fn forged_nonzero_section_padding_is_rejected() {
    // The emitter zero-fills the alignment gap before every aligned section; a
    // non-zero byte in any gap is smuggled state at a section boundary that the
    // CRC alone would carry along, so the loader must reject it.
    let mut bytes = emit_bytes(RECORD_QUERY);
    let pad_off = first_section_gap(&bytes);
    bytes[pad_off] = 1;
    reseal(&mut bytes);

    let err =
        Module::load_compiler_output(&bytes).expect_err("forged section padding must be rejected");
    assert!(
        matches!(err, ModuleError::NonZeroSectionPadding),
        "expected NonZeroSectionPadding, got {err:?}"
    );
}

#[test]
fn forged_unknown_opcode_is_rejected() {
    // `10` is unassigned; the VM's instruction decoder would
    // `.expect()` on the `None` from `Opcode::from_u8` at this address.
    let mut bytes = emit_bytes(RECORD_QUERY);
    let off = first_instr(&bytes, |_| true);
    bytes[off] = (bytes[off] & 0xF0) | 0x0A;
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes).expect_err("forged opcode must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidOpcode { opcode: 0x0A, .. }),
        "expected InvalidOpcode, got {err:?}"
    );
}

#[test]
fn forged_nonzero_segment_is_rejected() {
    // Segment bits (header bits 6-7) are reserved at zero; the call/return
    // decoders `assert!` on a non-zero segment.
    let mut bytes = emit_bytes(RECORD_QUERY);
    let off = first_instr(&bytes, |_| true);
    bytes[off] |= 0x40;
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes).expect_err("forged segment must be rejected");
    assert!(
        matches!(err, ModuleError::MalformedInstructionStream),
        "expected MalformedInstructionStream, got {err:?}"
    );
}

#[test]
fn forged_nonzero_call_return_node_kind_is_rejected() {
    // node_class_bits (header bits 4-5) is meaningful only for Match variants; the
    // Call/Return decoders ignore it, so the format pins those bits to zero.
    // This query emits both via a `(Leaf)` reference and definition returns.
    const REF_QUERY: &str = indoc!(
        "
        Top = (binary_expression left: (Leaf) @l)
        Leaf = (identifier) @id
    "
    );
    for opcode in [6u8, 7] {
        let mut bytes = emit_bytes(REF_QUERY);
        let off = first_instr(&bytes, |o| o == opcode);
        bytes[off] |= 0x10; // set node_class_bits bit 4
        reseal(&mut bytes);

        let err = Module::load_compiler_output(&bytes)
            .expect_err("forged node_class_bits must be rejected");
        assert!(
            matches!(err, ModuleError::MalformedInstructionStream),
            "opcode {opcode}: expected MalformedInstructionStream, got {err:?}"
        );
    }
}

#[test]
fn forged_reserved_node_kind_is_rejected() {
    // node_class_bits `0b11` (header bits 4-5) is reserved; `NodeKindConstraint::from_bytes`
    // would panic on it.
    let mut bytes = emit_bytes(RECORD_QUERY);
    let off = first_instr(&bytes, |o| o <= 5);
    bytes[off] |= 0x30;
    reseal(&mut bytes);

    let err =
        Module::load_compiler_output(&bytes).expect_err("forged node_class_bits must be rejected");
    assert!(
        matches!(err, ModuleError::MalformedInstructionStream),
        "expected MalformedInstructionStream, got {err:?}"
    );
}

#[test]
fn forged_invalid_nav_is_rejected() {
    // `0x80` is an Up-family byte (bit 7 set) with a zero level; `Nav::from_byte`
    // would panic, so the loader must reject it.
    let mut bytes = emit_bytes(RECORD_QUERY);
    let off = first_instr(&bytes, |o| o <= 5);
    bytes[off + 1] = 0x80;
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes).expect_err("forged nav must be rejected");
    assert!(
        matches!(err, ModuleError::MalformedInstructionStream),
        "expected MalformedInstructionStream, got {err:?}"
    );
}

#[test]
fn forged_invalid_effect_opcode_is_rejected() {
    // One past the last effect opcode: `EffectKind::from_u8` would panic when
    // the VM emits this effect.
    let mut bytes = emit_bytes(RECORD_QUERY);
    let slot = effect_slots(&bytes)[0];
    let existing = u16::from_le_bytes([bytes[slot], bytes[slot + 1]]);
    let invalid_op = EffectKind::BoolValue as u16 + 1;
    let forged = (invalid_op << EFFECT_PAYLOAD_BITS) | (existing & EFFECT_PAYLOAD_MAX as u16);
    bytes[slot..slot + 2].copy_from_slice(&forged.to_le_bytes());
    reseal(&mut bytes);

    let err =
        Module::load_compiler_output(&bytes).expect_err("forged effect opcode must be rejected");
    assert!(
        matches!(err, ModuleError::MalformedInstructionStream),
        "expected MalformedInstructionStream, got {err:?}"
    );
}

#[test]
fn forged_nonzero_unit_effect_payload_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let slot = effect_slots(&bytes)[0];
    bytes[slot..slot + 2].copy_from_slice(&effect_word_with_payload(EffectKind::ScalarMark, 1));
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("unit effects must reject a nonzero payload");
    assert!(
        matches!(err, ModuleError::MalformedInstructionStream),
        "expected MalformedInstructionStream, got {err:?}"
    );
}

#[test]
fn forged_bool_close_payload_out_of_range_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let slot = effect_slots(&bytes)[0];
    bytes[slot..slot + 2].copy_from_slice(&effect_word_with_payload(EffectKind::BoolClose, 2));
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("BoolClose payload must be exactly zero or one");
    assert!(
        matches!(err, ModuleError::MalformedInstructionStream),
        "expected MalformedInstructionStream, got {err:?}"
    );
}

#[test]
fn forged_span_effect_before_spans_section_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let slot = effect_slots(&bytes)[0];
    bytes[slot..slot + 2].copy_from_slice(&effect_word(EffectKind::SpanStart));
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes).expect_err("span effects need a spans section");
    assert!(
        matches!(err, ModuleError::InvalidSpanPayload(_)),
        "expected InvalidSpanPayload, got {err:?}"
    );
}

#[test]
fn forged_span_effect_payload_out_of_range_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let span = SpanEntry {
        source_id: 0,
        kind: SpanKind::Def,
        start: 0,
        end: 42,
        type_id: SPAN_NO_BINDING,
        member: SPAN_NO_BINDING,
    };
    add_single_span(&mut bytes, span.to_bytes());

    let slot = effect_slots(&bytes)[0];
    bytes[slot..slot + 2].copy_from_slice(&effect_word_with_payload(EffectKind::SpanStart, 1));
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("span payload must address an existing span");
    assert!(
        matches!(err, ModuleError::InvalidSpanPayload(_)),
        "expected InvalidSpanPayload, got {err:?}"
    );
}

#[test]
fn forged_oob_member_operand_is_rejected() {
    // A `RecordSet`/`VariantOpen` payload indexes the type-member table via the materializer's
    // `get_member`, which asserts the index is in bounds.
    let mut bytes = emit_bytes(RECORD_QUERY);
    let members = Module::load_compiler_output(&bytes)
        .expect("module validates before tampering")
        .header()
        .type_members_count;
    let slot = effect_slots(&bytes)
        .into_iter()
        .find(|&off| {
            let e = u16::from_le_bytes([bytes[off], bytes[off + 1]]);
            matches!(
                EffectKind::try_from_u8((e >> EFFECT_PAYLOAD_BITS) as u8),
                Some(EffectKind::RecordSet | EffectKind::VariantOpen)
            )
        })
        .expect("record query must emit a RecordSet/VariantOpen effect");
    let opcode_bits =
        u16::from_le_bytes([bytes[slot], bytes[slot + 1]]) & !(EFFECT_PAYLOAD_MAX as u16);
    let forged = opcode_bits | (members & EFFECT_PAYLOAD_MAX as u16);
    bytes[slot..slot + 2].copy_from_slice(&forged.to_le_bytes());
    reseal(&mut bytes);

    let err =
        Module::load_compiler_output(&bytes).expect_err("forged member operand must be rejected");
    assert!(
        matches!(err, ModuleError::MalformedInstructionStream),
        "expected MalformedInstructionStream, got {err:?}"
    );
}

fn add_single_span(bytes: &mut Vec<u8>, span_bytes: [u8; SPAN_ENTRY_SIZE]) {
    let mut header = Header::from_bytes(&bytes[..64]);
    header.spans_count = 1;
    let offsets = header.compute_offsets();
    let span_off = offsets.spans as usize;
    bytes.resize(span_off + SPAN_ENTRY_SIZE, 0);
    bytes[span_off..span_off + SPAN_ENTRY_SIZE].copy_from_slice(&span_bytes);
    header.total_size = bytes.len() as u32;
    header.checksum = crc32fast::hash(&bytes[64..]);
    bytes[..64].copy_from_slice(&header.to_bytes());
}

#[test]
fn span_section_view_decodes_valid_entry() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let span = SpanEntry {
        source_id: 0,
        kind: SpanKind::Def,
        start: 0,
        end: 42,
        type_id: 0,
        member: SPAN_NO_BINDING,
    };
    add_single_span(&mut bytes, span.to_bytes());

    let module = Module::load_compiler_output(&bytes).expect("valid span entry should load");

    assert_eq!(module.spans().len(), 1);
    assert_eq!(module.spans().get(0), span);
}

#[test]
fn forged_invalid_span_kind_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let mut span = SpanEntry {
        source_id: 0,
        kind: SpanKind::Def,
        start: 0,
        end: 42,
        type_id: SPAN_NO_BINDING,
        member: SPAN_NO_BINDING,
    }
    .to_bytes();
    span[2] = 99;
    add_single_span(&mut bytes, span);

    let err = Module::load_compiler_output(&bytes).expect_err("forged span kind must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidSpanEntry(0)),
        "expected InvalidSpanEntry, got {err:?}"
    );
}

#[test]
fn forged_invalid_span_range_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let span = SpanEntry {
        source_id: 0,
        kind: SpanKind::Pattern,
        start: 42,
        end: 7,
        type_id: SPAN_NO_BINDING,
        member: SPAN_NO_BINDING,
    };
    add_single_span(&mut bytes, span.to_bytes());

    let err = Module::load_compiler_output(&bytes).expect_err("forged span range must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidSpanEntry(0)),
        "expected InvalidSpanEntry, got {err:?}"
    );
}

#[test]
fn forged_member_binding_without_type_is_rejected() {
    // The emitter never writes a live member with no type; consumers key the
    // whole binding off `type_id`, so this combination is smuggled state.
    let mut bytes = emit_bytes(RECORD_QUERY);
    let span = SpanEntry {
        source_id: 0,
        kind: SpanKind::Capture,
        start: 3,
        end: 8,
        type_id: SPAN_NO_BINDING,
        member: 0,
    };
    add_single_span(&mut bytes, span.to_bytes());

    let err = Module::load_compiler_output(&bytes)
        .expect_err("member binding without type must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidSpanEntry(0)),
        "expected InvalidSpanEntry, got {err:?}"
    );
}

/// Like [`emit_bytes`], but with inspection spans compiled in — the module
/// carries a spans section and real span-bracket effects to tamper with.
fn emit_inspection_bytes(query_src: &str) -> Vec<u8> {
    let mut source_map = SourceMap::new();
    source_map.add_file(SourcePath::new("query.ptk"), query_src);
    let compiled = QueryBuilder::new(source_map)
        .compile(grammar())
        .expect("query parsing should not exhaust fuel");
    assert!(compiled.is_valid(), "query should compile: {query_src}");
    compiled
        .emit(BytecodeConfig::new().inspection(BytecodeInspection::Spans))
        .expect("bytecode emission answers")
        .into_artifact()
        .expect("compiled query has bytecode")
        .bytes()
        .to_vec()
}

/// Effect slots holding one of the given opcodes, in instruction order.
fn effect_slots_of(bytes: &[u8], kinds: &[EffectKind]) -> Vec<usize> {
    effect_slots(bytes)
        .into_iter()
        .filter(|&off| {
            let e = u16::from_le_bytes([bytes[off], bytes[off + 1]]);
            EffectKind::try_from_u8((e >> EFFECT_PAYLOAD_BITS) as u8)
                .is_some_and(|k| kinds.contains(&k))
        })
        .collect()
}

#[test]
fn forged_unclosed_span_bracket_is_rejected() {
    // Rewriting a SpanEnd into a SpanStart leaves its span open (and mis-pairs
    // every close after it); the balance verifier must reject the module.
    let mut bytes = emit_inspection_bytes(RECORD_QUERY);
    let slot = *effect_slots_of(&bytes, &[EffectKind::SpanEnd])
        .first()
        .expect("inspection module must emit a SpanEnd");
    let payload = u16::from_le_bytes([bytes[slot], bytes[slot + 1]]) & EFFECT_PAYLOAD_MAX as u16;
    bytes[slot..slot + 2]
        .copy_from_slice(&effect_word_with_payload(EffectKind::SpanStart, payload));
    reseal(&mut bytes);

    let err =
        Module::load_compiler_output(&bytes).expect_err("unclosed span bracket must be rejected");
    assert!(
        matches!(err, ModuleError::SpanImbalance(_)),
        "expected SpanImbalance, got {err:?}"
    );
}

#[test]
fn forged_unopened_span_close_is_rejected() {
    // Rewriting the first span open into a SpanEnd makes some path close a span
    // that was never opened.
    let mut bytes = emit_inspection_bytes(RECORD_QUERY);
    let slot = *effect_slots_of(&bytes, &[EffectKind::SpanStart, EffectKind::SpanStartAt])
        .first()
        .expect("inspection module must emit a span open");
    let payload = u16::from_le_bytes([bytes[slot], bytes[slot + 1]]) & EFFECT_PAYLOAD_MAX as u16;
    bytes[slot..slot + 2].copy_from_slice(&effect_word_with_payload(EffectKind::SpanEnd, payload));
    reseal(&mut bytes);

    let err =
        Module::load_compiler_output(&bytes).expect_err("unopened span close must be rejected");
    assert!(
        matches!(err, ModuleError::SpanImbalance(_)),
        "expected SpanImbalance, got {err:?}"
    );
}

#[test]
fn forged_mispaired_span_ids_are_rejected() {
    // Depth stays balanced, but a SpanEnd names a different span than the
    // matching open — inspection extraction asserts pairing, so the loader must
    // prove it.
    let mut bytes = emit_inspection_bytes(RECORD_QUERY);
    let spans_count = Module::load_compiler_output(&bytes)
        .expect("module validates before tampering")
        .header()
        .spans_count;
    assert!(spans_count >= 2, "test needs two spans to mis-pair");
    let slot = *effect_slots_of(&bytes, &[EffectKind::SpanEnd])
        .first()
        .expect("inspection module must emit a SpanEnd");
    let payload = u16::from_le_bytes([bytes[slot], bytes[slot + 1]]) & EFFECT_PAYLOAD_MAX as u16;
    let other = (payload + 1) % spans_count;
    bytes[slot..slot + 2].copy_from_slice(&effect_word_with_payload(EffectKind::SpanEnd, other));
    reseal(&mut bytes);

    let err =
        Module::load_compiler_output(&bytes).expect_err("mis-paired span ids must be rejected");
    assert!(
        matches!(err, ModuleError::SpanImbalance(_)),
        "expected SpanImbalance, got {err:?}"
    );
}

#[test]
fn forged_invalid_span_binding_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let module = Module::load_compiler_output(&bytes).expect("module validates before tampering");
    let span = SpanEntry {
        source_id: 0,
        kind: SpanKind::Capture,
        start: 3,
        end: 8,
        type_id: module.header().type_defs_count,
        member: SPAN_NO_BINDING,
    };
    add_single_span(&mut bytes, span.to_bytes());

    let err =
        Module::load_compiler_output(&bytes).expect_err("forged span binding must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidSpanEntry(0)),
        "expected InvalidSpanEntry, got {err:?}"
    );
}

#[test]
fn forged_zero_successor_is_rejected() {
    // `0` cannot become a `SuccessorAddr`; it is the terminal marker
    // only for the `Match8` fast path, never an extended successor slot.
    let mut bytes = emit_bytes(RECORD_QUERY);
    let succ_off = first_ext_successor(&bytes);
    bytes[succ_off..succ_off + 2].copy_from_slice(&0u16.to_le_bytes());
    reseal(&mut bytes);

    let err =
        Module::load_compiler_output(&bytes).expect_err("forged zero successor must be rejected");
    assert!(
        matches!(err, ModuleError::MalformedInstructionStream),
        "expected MalformedInstructionStream, got {err:?}"
    );
}

#[test]
fn forged_out_of_range_successor_is_rejected() {
    // A successor past the word count would slice past the instruction buffer.
    let mut bytes = emit_bytes(RECORD_QUERY);
    let succ_off = first_ext_successor(&bytes);
    bytes[succ_off..succ_off + 2].copy_from_slice(&u16::MAX.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("forged out-of-range successor must be rejected");
    assert!(
        matches!(err, ModuleError::MalformedInstructionStream),
        "expected MalformedInstructionStream, got {err:?}"
    );
}

#[test]
fn forged_depth_imbalance_is_rejected() {
    let mut bytes = emit_bytes(r#"Q = (program)"#);

    let nav_off = first_match_nav(&bytes, |nav| nav == Nav::StayExact.to_byte());
    bytes[nav_off] = Nav::Down.to_byte();
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("forged cursor-depth imbalance must be rejected");
    assert!(
        matches!(err, ModuleError::DepthImbalance(_)),
        "expected DepthImbalance, got {err:?}"
    );
}

#[test]
fn forged_regex_pattern_string_id_is_rejected() {
    // A regex entry's `string_id` is display metadata that `dump`/`trace` resolve
    // through the panicking `pattern_string_id` (StringId construction) and then index the
    // string blob; `0` (reserved) and an out-of-range id must be rejected at load.
    for forged in [0u16, u16::MAX] {
        let mut bytes = emit_bytes(r#"Q = (identifier =~ /x/)"#);
        let (regex_off, regex_count, str_count) = {
            let m =
                Module::load_compiler_output(&bytes).expect("module validates before tampering");
            (
                m.offsets().regex_table as usize,
                m.header().regex_table_count,
                m.header().str_table_count,
            )
        };
        assert!(regex_count > 1, "query must emit a regex entry");

        // Entry 1 is the first real regex (index 0 is reserved); `string_id` is its
        // leading u16. `u16::MAX` collapses to `str_count` to stay a valid forge.
        let value = if forged == u16::MAX {
            str_count
        } else {
            forged
        };
        let sid_off = regex_off + 8;
        bytes[sid_off..sid_off + 2].copy_from_slice(&value.to_le_bytes());
        reseal(&mut bytes);

        let err = Module::load_compiler_output(&bytes)
            .expect_err("forged regex string_id must be rejected");
        assert!(
            matches!(err, ModuleError::InvalidStringId(_)),
            "forged regex string_id {value}: expected InvalidStringId, got {err:?}"
        );
    }
}

#[test]
fn forged_nonzero_regex_table_reserved_is_rejected() {
    // Each regex-table entry is `string_id(u16) | reserved(u16) | offset(u32)`; the
    // reserved field is pinned to zero (docs/binary-format/03-symbols.md). A forged
    // non-zero value must be rejected at load, not carried as smuggled state.
    let mut bytes = emit_bytes(r#"Q = (identifier =~ /x/)"#);
    let (regex_off, regex_count) = {
        let m = Module::load_compiler_output(&bytes).expect("module validates before tampering");
        (
            m.offsets().regex_table as usize,
            m.header().regex_table_count,
        )
    };
    assert!(regex_count > 1, "query must emit a regex entry");

    // Entry 1's reserved u16 sits 2 bytes into its 8-byte record (index 0 reserved).
    let reserved_off = regex_off + 8 + 2;
    bytes[reserved_off..reserved_off + 2].copy_from_slice(&1u16.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("forged regex reserved field must be rejected");
    assert!(
        matches!(err, ModuleError::MalformedRegexTable),
        "expected MalformedRegexTable, got {err:?}"
    );
}

#[test]
fn forged_corrupt_regex_dfa_is_rejected() {
    // The regex blob holds the serialized sparse DFA the loader deserializes
    // once into the module's cache; a corrupt blob must be rejected at load, not
    // `.expect()`ed at match time.
    let mut bytes = emit_bytes(r#"Q = (identifier =~ /x/)"#);
    let (blob_off, blob_len) = {
        let m = Module::load_compiler_output(&bytes).expect("module validates before tampering");
        (
            m.offsets().regex_blob as usize,
            m.header().regex_blob_size as usize,
        )
    };
    assert!(blob_len > 0, "query must emit a DFA blob");

    bytes[blob_off..blob_off + blob_len].fill(0xFF);
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes).expect_err("forged regex DFA must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidRegexDfa(_)),
        "expected InvalidRegexDfa, got {err:?}"
    );
}

#[test]
fn forged_entry_point_into_instruction_interior_is_rejected() {
    // Issue #457: an entry-point `target` that lands inside a multi-word
    // instruction (not on a recorded instruction start) makes the VM begin
    // decoding mid-instruction. `target < word_count` is not enough — the load-time
    // check holds entry points to the same instruction-start rule as successors.
    let mut bytes = emit_bytes(RECORD_QUERY);
    let (ep_off, interior) = {
        let m = Module::load_compiler_output(&bytes).expect("module validates before tampering");
        (
            m.offsets().entry_points as usize,
            first_multiword_interior_addr(&bytes).get(),
        )
    };

    // `target` is the second u16 of the 8-byte entry point, after the name.
    bytes[ep_off + 2..ep_off + 4].copy_from_slice(&interior.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("forged interior entry point must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidEntryPoint(_)),
        "expected InvalidEntryPoint, got {err:?}"
    );
}

#[test]
fn forged_record_set_to_array_push_is_rejected() {
    // Swap an executed `RecordSet` for `ArrayPush`. A validated representation
    // would accept it, then the materializer would panic because the builder on
    // top is a Record, not a List. The effect-stack verifier rejects it at load.
    let mut bytes = emit_bytes(RECORD_QUERY);
    let slot = first_effect_op(&bytes, |op| op == EffectKind::RecordSet as u16);
    bytes[slot..slot + 2].copy_from_slice(&effect_word(EffectKind::ArrayPush));
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("forged RecordSet->ArrayPush must be rejected");
    assert!(
        matches!(err, ModuleError::EffectStackImbalance(_)),
        "expected EffectStackImbalance, got {err:?}"
    );
}

#[test]
fn forged_scalar_capture_record_set_to_array_push_is_rejected() {
    // The minimal case: a scalar record whose only effect is a `RecordSet` into
    // the entry-point wrapper's root record. Forged to `ArrayPush`, the body now
    // demands a List top while the wrapper hands it a Record — caught when the entry point
    // wrapper is checked as a root.
    let mut bytes = emit_bytes(r#"Q = (identifier) @id"#);
    let slot = first_effect_op(&bytes, |op| op == EffectKind::RecordSet as u16);
    bytes[slot..slot + 2].copy_from_slice(&effect_word(EffectKind::ArrayPush));
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("forged scalar RecordSet->ArrayPush must be rejected");
    assert!(
        matches!(err, ModuleError::EffectStackImbalance(_)),
        "expected EffectStackImbalance, got {err:?}"
    );
}

#[test]
fn forged_record_set_without_producer_is_rejected() {
    // Replace the producer in `[Node RecordSet]` with a frame opener. The
    // following `RecordSet` has a valid Record target but no pending value.
    let mut bytes = emit_bytes(r#"Q = (identifier) @id"#);
    let slot = first_effect_op(&bytes, |op| op == EffectKind::Node as u16);
    bytes[slot..slot + 2].copy_from_slice(&effect_word(EffectKind::RecordOpen));
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("forged producerless RecordSet must be rejected");
    assert!(
        matches!(err, ModuleError::EffectStackImbalance(_)),
        "expected EffectStackImbalance, got {err:?}"
    );
}

#[test]
fn forged_no_payload_variant_case_with_data_is_rejected() {
    // The alternative emits direct fields for a structured variant. Lie that the
    // variant case has no payload: `VariantClose` must reject the data-bearing
    // payload instead of letting materialization silently drop or mis-shape it.
    let mut bytes = emit_bytes(r#"Q = [A: (identifier) @a B: (number)]"#);
    let variant_open = first_effect_op(&bytes, |op| op == EffectKind::VariantOpen as u16);
    let variant_member = effect_payload(&bytes, variant_open);
    let type_id_off = type_member_type_id_off(&bytes, variant_member);

    bytes[type_id_off..type_id_off + 2].copy_from_slice(&0u16.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("forged data on a no-payload variant case must be rejected");
    assert!(
        matches!(err, ModuleError::EffectStackImbalance(_)),
        "expected EffectStackImbalance, got {err:?}"
    );
}

#[test]
fn forged_data_variant_case_without_data_is_rejected() {
    // A tag-only case emits `VariantOpen`/`VariantClose` and has no payload effects.
    // Lie that the case has a payload by pointing it at the variant type itself.
    let mut bytes = emit_bytes(r#"Q = [A: (identifier)]"#);
    let type_count = Module::load_compiler_output(&bytes)
        .expect("module validates before tampering")
        .header()
        .type_defs_count;
    assert!(type_count > 1, "query must emit a value type to point at");

    let variant_open = first_effect_op(&bytes, |op| op == EffectKind::VariantOpen as u16);
    let variant_member = effect_payload(&bytes, variant_open);
    let type_id_off = type_member_type_id_off(&bytes, variant_member);

    bytes[type_id_off..type_id_off + 2].copy_from_slice(&1u16.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("forged missing variant data must be rejected");
    assert!(
        matches!(err, ModuleError::EffectStackImbalance(_)),
        "expected EffectStackImbalance, got {err:?}"
    );
}

#[test]
fn forged_dropped_scope_close_is_rejected() {
    // Turn a `RecordClose` into a no-op `Node`: the record's `RecordOpen` is
    // never closed, so the body returns with an open frame — the materializer
    // would leave the builder stack unbalanced. Rejected as a non-neutral body.
    let mut bytes = emit_bytes(RECORD_QUERY);
    let slot = first_effect_op(&bytes, |op| op == EffectKind::RecordClose as u16);
    bytes[slot..slot + 2].copy_from_slice(&effect_word(EffectKind::Node));
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("forged dropped RecordClose must be rejected");
    assert!(
        matches!(err, ModuleError::EffectStackImbalance(_)),
        "expected EffectStackImbalance, got {err:?}"
    );
}

#[test]
fn forged_mismatched_scalar_frame_is_rejected() {
    let mut bytes = emit_bytes(indoc! {r#"
        Q = (program
          {
            (comment) @comment
            (expression_statement (identifier) @id)
          } @chunk :: str
        )
    "#});
    let slot = first_effect_op(&bytes, |op| op == EffectKind::ScalarOpen as u16);
    bytes[slot..slot + 2].copy_from_slice(&effect_word(EffectKind::RecordOpen));
    reseal(&mut bytes);

    let err =
        Module::load_compiler_output(&bytes).expect_err("StrClose must close a ScalarOpen frame");
    assert!(matches!(err, ModuleError::EffectStackImbalance(_)));
}

#[test]
fn forged_suppress_underflow_is_rejected() {
    // Replace a data effect with a bare `SuppressEnd`. With no matching
    // `SuppressBegin` on the path, the VM's suppression counter would underflow
    // and `.expect()` panic; the verifier rejects it at load.
    let mut bytes = emit_bytes(RECORD_QUERY);
    let slot = first_effect_op(&bytes, |op| {
        op == EffectKind::RecordOpen as u16 || op == EffectKind::RecordSet as u16
    });
    bytes[slot..slot + 2].copy_from_slice(&effect_word(EffectKind::SuppressEnd));
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("forged SuppressEnd underflow must be rejected");
    assert!(matches!(err, ModuleError::EffectStackImbalance(_)));
}

/// Address of the last `Return` instruction in the instruction stream.
fn last_return_addr(bytes: &[u8]) -> CodeAddr {
    let (base, word_count) = instruction_section(bytes);
    let mut found = None;
    let mut addr = CodeAddr::ZERO;
    while addr.get() < word_count {
        let off = base + addr.as_usize() * BYTECODE_WORD_SIZE;
        let opcode = bytes[off] & 0x0F;
        if opcode == 0x7 {
            found = Some(addr);
        }
        addr = addr
            .checked_add((instr_size(opcode) / BYTECODE_WORD_SIZE) as u16)
            .expect("instruction address fits in u16");
    }
    found.expect("no Return in the instruction stream")
}

/// Byte offset of the `n`th (0-based) effect slot whose opcode satisfies `want`.
fn nth_effect_op(bytes: &[u8], n: usize, want: impl Fn(u16) -> bool) -> usize {
    effect_slots(bytes)
        .into_iter()
        .filter(|&off| {
            want(u16::from_le_bytes([bytes[off], bytes[off + 1]]) >> EFFECT_PAYLOAD_BITS)
        })
        .nth(n)
        .expect("no matching effect slot in the instruction stream")
}

#[test]
fn forged_accept_inside_called_def_is_rejected() {
    // Zero the `next` of the Match8 that flows into the definition body's
    // `Return`, turning it into a terminal (accepting) match. A successor-less
    // match accepts the whole run from any call depth, so the wrapper's root
    // `RecordOpen` would still be open in the committed log and the
    // materializer's end-of-log balance assert would panic. The body is locally
    // balanced at that point — only the rule that called bodies must not
    // contain accepts catches it.
    let mut bytes = emit_bytes(r#"Q = (program (expression_statement (identifier) @name))"#);

    let def_return = last_return_addr(&bytes);
    let (base, word_count) = instruction_section(&bytes);
    let mut addr = CodeAddr::ZERO;
    let mut patched = false;
    while addr.get() < word_count {
        let off = base + addr.as_usize() * BYTECODE_WORD_SIZE;
        let opcode = bytes[off] & 0x0F;
        if opcode == 0x0 && u16::from_le_bytes([bytes[off + 6], bytes[off + 7]]) == def_return.get()
        {
            bytes[off + 6..off + 8].copy_from_slice(&0u16.to_le_bytes());
            patched = true;
            break;
        }
        addr = addr
            .checked_add((instr_size(opcode) / BYTECODE_WORD_SIZE) as u16)
            .expect("instruction address fits in u16");
    }
    assert!(patched, "no Match8 flows into the def body's Return");
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("forged accept inside a called body must be rejected");
    // Return-route validation rejects the now-returnless callee before the
    // effect-stack pass reaches the open wrapper frame.
    assert!(matches!(err, ModuleError::MalformedInstructionStream));
}

#[test]
fn forged_variant_wrapper_hiding_callee_write_is_rejected() {
    // A definition body applies `RecordSet` to its capture below entry, into the frame its
    // wrapper opened. Retarget that write: swap the wrapper's root
    // `RecordOpen`/`RecordClose` for `VariantOpen`/`VariantClose` on a no-payload
    // (tag-only) member borrowed from the other entry point. The callee's
    // below-entry `RecordSet` then lands data on a no-payload case — invisible to the
    // wrapper's own walk, because the write happens inside the callee. Only a
    // call site that forks on the callee's may-write (`record_sets_caller_top`)
    // rejects it; stale payload-field state would let it load and mis-materialize.
    let mut bytes = emit_bytes(indoc! {r#"
        A = (program [T: (comment)] @e)
        Z = (program (function_declaration) @fn)
    "#});

    let no_payload_member = {
        let m = Module::load_compiler_output(&bytes).expect("module validates before tampering");
        let types = m.types();
        (0..m.header().type_members_count)
            .find(|&i| {
                types
                    .get(types.member_type_id(i as usize))
                    .is_some_and(|def| {
                        matches!(def.decode(), TypeDefKind::Primitive(TypeKind::NoValue))
                    })
            })
            .expect("query must emit a no-payload variant member")
    };

    // Both wrappers precede the bodies and only wrappers open frames, so slot
    // order is A's pair then Z's pair; forge Z's.
    let open_slot = nth_effect_op(&bytes, 1, |op| op == EffectKind::RecordOpen as u16);
    let close_slot = nth_effect_op(&bytes, 1, |op| op == EffectKind::RecordClose as u16);
    bytes[open_slot..open_slot + 2].copy_from_slice(&effect_word_with_payload(
        EffectKind::VariantOpen,
        no_payload_member,
    ));
    bytes[close_slot..close_slot + 2].copy_from_slice(&effect_word(EffectKind::VariantClose));
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("forged no-payload case callee write must be rejected");
    assert!(
        matches!(err, ModuleError::EffectStackImbalance(_)),
        "expected EffectStackImbalance, got {err:?}"
    );
}

#[test]
fn forged_record_wrapper_without_root_frame_is_rejected() {
    // A record-producing entry-point wrapper opens a root `RecordOpen` before calling the
    // body, so the body always has a Record to apply `RecordSet` to. Neutralize
    // `RecordOpen` and its matching `RecordClose` (turn both into no-op `Absent`s)
    // and lie that the result type is scalar: the entry's `RecordSet` would then hit
    // the materializer's scalar root frame and panic. The wrapper has no caller,
    // so a requirement bubbling out of it must be rejected, not silently dropped.
    let mut bytes = emit_bytes(r#"Q = (_) @x"#);
    let ep_off = Module::load_compiler_output(&bytes)
        .expect("module validates before tampering")
        .offsets()
        .entry_points as usize;
    let record_open_slot = first_effect_op(&bytes, |op| op == EffectKind::RecordOpen as u16);
    let record_close_slot = first_effect_op(&bytes, |op| op == EffectKind::RecordClose as u16);

    let absent = effect_word(EffectKind::Absent);
    bytes[record_open_slot..record_open_slot + 2].copy_from_slice(&absent);
    bytes[record_close_slot..record_close_slot + 2].copy_from_slice(&absent);
    // Result type T1 (record) -> T0 (scalar <Node>): the root frame is now a Scalar.
    bytes[ep_off + 4..ep_off + 6].copy_from_slice(&0u16.to_le_bytes());
    reseal(&mut bytes);

    let err =
        Module::load_compiler_output(&bytes).expect_err("forged rootless wrapper must be rejected");
    assert!(
        matches!(err, ModuleError::EffectStackImbalance(_)),
        "expected EffectStackImbalance, got {err:?}"
    );
}

#[test]
fn forged_set_extended_match_reserved_count_bit_is_rejected() {
    // Bit 0 of an extended-Match counts word (low bit of byte 6) is reserved-zero
    // (docs/binary-format/06-instructions.md); the decoder never reads it, so a
    // forged set bit must be rejected at load.
    let mut bytes = emit_bytes(RECORD_QUERY);
    let off = first_instr(&bytes, |o| (1..=5).contains(&o)); // extended Match
    bytes[off + 6] |= 0x01;
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("forged reserved count bit must be rejected");
    assert!(
        matches!(err, ModuleError::MalformedInstructionStream),
        "expected MalformedInstructionStream, got {err:?}"
    );
}

#[test]
fn forged_nonzero_return_pad_is_rejected() {
    // Bytes 1-2 are the outcome and entry contract; bytes 3-7 are padding.
    for byte in 3usize..8 {
        let mut bytes = emit_bytes(RECORD_QUERY);
        let off = first_instr(&bytes, |o| o == 7); // Return
        bytes[off + byte] = 1;
        reseal(&mut bytes);

        let err =
            Module::load_compiler_output(&bytes).expect_err("forged return pad must be rejected");
        assert!(
            matches!(err, ModuleError::MalformedInstructionStream),
            "forged byte {byte}: expected MalformedInstructionStream, got {err:?}"
        );
    }
}

#[test]
fn forged_invalid_return_entry_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let off = first_instr(&bytes, |opcode| opcode == 7);
    bytes[off + 2] = 2;
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("unknown return entry contract must be rejected");
    assert!(matches!(err, ModuleError::MalformedInstructionStream));
}

#[test]
fn forged_invalid_return_outcome_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let off = first_instr(&bytes, |opcode| opcode == 7);
    bytes[off + 1] = 2;
    reseal(&mut bytes);

    let err =
        Module::load_compiler_output(&bytes).expect_err("unknown return outcome must be rejected");
    assert!(matches!(err, ModuleError::MalformedInstructionStream));
}

#[test]
fn forged_split_call_invalid_nav_and_zero_targets_are_rejected() {
    for byte in [1usize, 2, 4, 6] {
        let mut bytes = emit_bytes(SPLIT_CALL_QUERY);
        let off = first_instr(&bytes, |opcode| opcode == 8);
        if byte == 1 {
            bytes[off + byte] = 0x80;
        } else {
            bytes[off + byte..off + byte + 2].copy_from_slice(&0u16.to_le_bytes());
        }
        reseal(&mut bytes);

        let err = Module::load_compiler_output(&bytes)
            .expect_err("malformed split call must be rejected");
        assert!(matches!(err, ModuleError::MalformedInstructionStream));
    }
}

#[test]
fn forged_routed_call_invalid_metadata_and_targets_are_rejected() {
    for byte in [1usize, 2, 3, 4, 6] {
        let mut bytes = emit_bytes(ROUTED_CALL_QUERY);
        let off = first_instr(&bytes, |opcode| opcode == 9);
        match byte {
            1 => bytes[off + byte] = 0x80,
            2 | 3 => bytes[off + byte] = 1,
            4 | 6 => bytes[off + byte..off + byte + 2].copy_from_slice(&0u16.to_le_bytes()),
            _ => unreachable!("test enumerates every RoutedCall field"),
        }
        reseal(&mut bytes);

        let err = Module::load_compiler_output(&bytes)
            .expect_err("malformed routed call must be rejected");
        assert!(matches!(err, ModuleError::MalformedInstructionStream));
    }
}

#[test]
fn forged_ordinary_and_routed_call_target_mismatches_are_rejected() {
    let mut bytes = emit_bytes(ROUTED_CALL_QUERY);
    let ordinary = first_instr(&bytes, |opcode| opcode == 6);
    let routed = first_instr(&bytes, |opcode| opcode == 9);
    let routed_target = [bytes[routed + 6], bytes[routed + 7]];
    bytes[ordinary + 6..ordinary + 8].copy_from_slice(&routed_target);
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("ordinary call cannot target a routed body");
    assert!(matches!(err, ModuleError::MalformedInstructionStream));

    let mut bytes = emit_bytes(ROUTED_CALL_QUERY);
    let ordinary = first_instr(&bytes, |opcode| opcode == 6);
    let routed = first_instr(&bytes, |opcode| opcode == 9);
    let ordinary_target = [bytes[ordinary + 6], bytes[ordinary + 7]];
    bytes[routed + 6..routed + 8].copy_from_slice(&ordinary_target);
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("routed call cannot target an ordinary body");
    assert!(matches!(err, ModuleError::MalformedInstructionStream));
}

#[test]
fn forged_call_and_callee_return_contract_mismatch_is_rejected() {
    let mut bytes = emit_bytes(SPLIT_CALL_QUERY);
    let split = first_instr(&bytes, |opcode| opcode == 8);
    let ordinary = first_instr(&bytes, |opcode| opcode == 6);
    let split_target = [bytes[split + 6], bytes[split + 7]];
    bytes[ordinary + 6..ordinary + 8].copy_from_slice(&split_target);
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("ordinary call cannot target a split-return body");
    assert!(matches!(err, ModuleError::MalformedInstructionStream));

    let mut bytes = emit_bytes(SPLIT_CALL_QUERY);
    let split = first_instr(&bytes, |opcode| opcode == 8);
    let ordinary = first_instr(&bytes, |opcode| opcode == 6);
    let ordinary_target = [bytes[ordinary + 6], bytes[ordinary + 7]];
    bytes[split + 6..split + 8].copy_from_slice(&ordinary_target);
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("split call requires a body with both return outcomes");
    assert!(matches!(err, ModuleError::MalformedInstructionStream));
}

#[test]
fn forged_nonzero_predicate_reserved_bits_is_rejected() {
    // Only the op/flags word's low byte (operator) and bit 8 (regex flag) are
    // decoded; bits 9-15 are reserved-zero. A forged set bit there must be
    // rejected at load. Bit 9 is the low bit of the word's high byte.
    let mut bytes = emit_bytes(r#"Q = (identifier == "needle")"#);
    let pred_off = find_predicate_off(&bytes);
    bytes[pred_off + 1] |= 0x02;
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("forged predicate reserved bit must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidPredicateOperand(_)),
        "expected InvalidPredicateOperand, got {err:?}"
    );
}

#[test]
fn forged_regex_predicate_sentinel_operand_is_rejected() {
    // Regex value_ref `0` is the reserved sentinel: `load_regex_dfas` skips it,
    // so its DFA slot is `None`, and the VM expects a populated slot. The loader
    // must reject the sentinel operand — real regexes start at index 1 — instead
    // of letting it reach that expect. (The string side is safe at `0`: index 0
    // there is the validated easter-egg string.)
    let mut bytes = emit_bytes(r#"Q = (identifier =~ /needle/)"#);
    let pred_off = find_predicate_off(&bytes);
    bytes[pred_off + 2..pred_off + 4].copy_from_slice(&0u16.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("forged regex sentinel operand must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidPredicateOperand(_)),
        "expected InvalidPredicateOperand, got {err:?}"
    );
}

#[test]
fn forged_predicate_regex_flag_mismatch_is_rejected() {
    // The is_regex flag must agree with the op's class. Set the regex flag (bit 8
    // of `op_and_flags`) on a string op (`==`): the VM would resolve a string
    // operand as a regex and hit its op/flag `unreachable!`. The op nibble is left
    // intact, so this isolates the `op_is_regex != is_regex` branch.
    let mut bytes = emit_bytes(r#"Q = (identifier == "needle")"#);
    let pred_off = find_predicate_off(&bytes);
    bytes[pred_off + 1] |= 0x01;
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("forged regex-flag mismatch must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidPredicateOperand(_)),
        "expected InvalidPredicateOperand, got {err:?}"
    );
}

#[test]
fn forged_unknown_type_def_kind_is_rejected() {
    // A TypeDef's kind byte (byte 3 of the 4-byte entry) must be a known TypeKind;
    // an unknown kind would panic the materializer's `def`/`TypeDefKind` decode.
    let mut bytes = emit_bytes(RECORD_QUERY);
    let type_defs_off = Module::load_compiler_output(&bytes)
        .expect("module validates before tampering")
        .offsets()
        .type_defs as usize;
    bytes[type_defs_off + 3] = 0xFF;
    reseal(&mut bytes);

    let err =
        Module::load_compiler_output(&bytes).expect_err("forged type-def kind must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidTypeDef(_)),
        "expected InvalidTypeDef, got {err:?}"
    );
}

#[test]
fn forged_out_of_range_entry_point_target_is_rejected() {
    // The plain out-of-range case (vs the interior-target case above): `target >=
    // word_count` must be rejected before `is_start` is indexed.
    let mut bytes = emit_bytes(RECORD_QUERY);
    let ep_off = Module::load_compiler_output(&bytes)
        .expect("module validates before tampering")
        .offsets()
        .entry_points as usize;
    bytes[ep_off + 2..ep_off + 4].copy_from_slice(&u16::MAX.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("forged out-of-range target must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidEntryPoint(_)),
        "expected InvalidEntryPoint, got {err:?}"
    );
}

#[test]
fn forged_nonzero_entry_point_pad_is_rejected() {
    // Bytes 6-7 of the 8-byte entry point are reserved `_pad`; `from_bytes` drops
    // them, so a forged non-zero pad must be rejected at load, not ignored.
    let mut bytes = emit_bytes(RECORD_QUERY);
    let ep_off = Module::load_compiler_output(&bytes)
        .expect("module validates before tampering")
        .offsets()
        .entry_points as usize;
    bytes[ep_off + 6..ep_off + 8].copy_from_slice(&1u16.to_le_bytes());
    reseal(&mut bytes);

    let err =
        Module::load_compiler_output(&bytes).expect_err("forged entry-point pad must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidEntryPoint(_)),
        "expected InvalidEntryPoint, got {err:?}"
    );
}

#[test]
fn forged_out_of_range_entry_point_result_type_is_rejected() {
    // `result_type` (u16 at entry+4) must address a real TypeDef, or the
    // materializer's root-frame TypeId lookup reads out of bounds. `type_defs_count`
    // is one past the last valid index.
    let mut bytes = emit_bytes(RECORD_QUERY);
    let (ep_off, type_defs) = {
        let m = Module::load_compiler_output(&bytes).expect("module validates before tampering");
        (
            m.offsets().entry_points as usize,
            m.header().type_defs_count,
        )
    };
    bytes[ep_off + 4..ep_off + 6].copy_from_slice(&type_defs.to_le_bytes());
    reseal(&mut bytes);

    let err =
        Module::load_compiler_output(&bytes).expect_err("forged result_type must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidEntryPoint(_)),
        "expected InvalidEntryPoint, got {err:?}"
    );
}

#[test]
fn forged_type_member_name_string_id_is_rejected() {
    // `validate_string_ids` runs one closure over six sections with distinct
    // (base, stride, name_off) tuples; this locks the type-member arithmetic
    // (stride 4, name at offset 0) the materializer's record-field keys rely on.
    let mut bytes = emit_bytes(RECORD_QUERY);
    let members_off = Module::load_compiler_output(&bytes)
        .expect("module validates before tampering")
        .offsets()
        .type_members as usize;
    bytes[members_off..members_off + 2].copy_from_slice(&0u16.to_le_bytes());
    reseal(&mut bytes);

    let err =
        Module::load_compiler_output(&bytes).expect_err("forged type-member name must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidStringId(_)),
        "expected InvalidStringId, got {err:?}"
    );
}

#[test]
fn forged_oob_member_type_id_is_rejected() {
    // A TypeMember's `type_id` (bytes 2-3 of the 4-byte entry) must address a real
    // TypeDef, or the materializer resolves a record field to a type out of range.
    let mut bytes = emit_bytes(RECORD_QUERY);
    let (members_off, type_defs) = {
        let m = Module::load_compiler_output(&bytes).expect("module validates before tampering");
        (
            m.offsets().type_members as usize,
            m.header().type_defs_count,
        )
    };

    // `type_defs_count` is one past the last valid TypeId.
    bytes[members_off + 2..members_off + 4].copy_from_slice(&type_defs.to_le_bytes());
    reseal(&mut bytes);

    let err =
        Module::load_compiler_output(&bytes).expect_err("forged member type id must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidTypeDef(_)),
        "expected InvalidTypeDef, got {err:?}"
    );
}

#[test]
fn forged_oob_wrapper_inner_type_id_is_rejected() {
    // A wrapper/alias TypeDef holds its inner TypeId in `data` (bytes 0-1 of the
    // 4-byte entry); it must address a real def or `option_inner` / the list
    // element lookup resolves a type out of range.
    let mut bytes = emit_bytes(r#"Top = (program (expression_statement)* @stmts)"#);
    let (defs_off, type_defs, wrapper_idx) = {
        let m = Module::load_compiler_output(&bytes).expect("module validates before tampering");
        let types = m.types();
        let idx = (0..types.defs_count())
            .find(|&i| matches!(types.def(i).decode(), TypeDefKind::Wrapper { .. }))
            .expect("list query must emit a wrapper type def");
        (
            m.offsets().type_defs as usize,
            m.header().type_defs_count,
            idx,
        )
    };

    let off = defs_off + wrapper_idx * 4;
    bytes[off..off + 2].copy_from_slice(&type_defs.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("forged wrapper inner type id must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidTypeDef(_)),
        "expected InvalidTypeDef, got {err:?}"
    );
}

#[test]
fn forged_nonzero_primitive_typedef_reserved_is_rejected() {
    // NoValue/Node/Text/Bool carry no metadata: both `data` (bytes 0-1) and `count`
    // (byte 2) are reserved-zero (docs/binary-format/04-types.md). Smuggled state
    // in either must be rejected, not silently ignored by the typed view.
    for byte in [0usize, 2] {
        let mut bytes = emit_bytes(RECORD_QUERY);
        let (defs_off, prim_idx) = {
            let m =
                Module::load_compiler_output(&bytes).expect("module validates before tampering");
            let types = m.types();
            let idx = (0..types.defs_count())
                .find(|&i| matches!(types.def(i).decode(), TypeDefKind::Primitive(_)))
                .expect("record query must emit a primitive type def");
            (m.offsets().type_defs as usize, idx)
        };

        bytes[defs_off + prim_idx * 4 + byte] = 1;
        reseal(&mut bytes);

        let err = Module::load_compiler_output(&bytes)
            .expect_err("forged primitive reserved field must be rejected");
        assert!(
            matches!(err, ModuleError::InvalidTypeDef(_)),
            "forged byte {byte}: expected InvalidTypeDef, got {err:?}"
        );
    }
}

#[test]
fn scalar_primitive_typedefs_use_reserved_zero_metadata() {
    for (query, expected) in [
        (r#"Q = (identifier) @id :: str"#, TypeKind::Text),
        (
            r#"Q = (program (identifier)? @present :: bool)"#,
            TypeKind::Bool,
        ),
    ] {
        for byte in [0usize, 2] {
            let mut bytes = emit_bytes(query);
            let (defs_off, primitive_idx) = {
                let module = Module::load_compiler_output(&bytes)
                    .expect("module validates before tampering");
                let types = module.types();
                let index = (0..types.defs_count())
                    .find(|&index| {
                        matches!(types.def(index).decode(), TypeDefKind::Primitive(kind) if kind == expected)
                    })
                    .expect("query must emit the requested scalar primitive");
                (module.offsets().type_defs as usize, index)
            };

            bytes[defs_off + primitive_idx * 4 + byte] = 1;
            reseal(&mut bytes);

            let err = Module::load_compiler_output(&bytes)
                .expect_err("scalar primitive metadata must remain reserved-zero");
            assert!(
                matches!(err, ModuleError::InvalidTypeDef(_)),
                "{expected:?}, byte {byte}: expected InvalidTypeDef, got {err:?}"
            );
        }
    }
}

#[test]
fn version_ten_module_is_rejected_without_compatibility_mode() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    bytes[4..8].copy_from_slice(&10_u32.to_le_bytes());

    let err = Module::load_compiler_output(&bytes)
        .expect_err("v10 modules must be regenerated for the scalar vocabulary");
    assert!(
        matches!(err, ModuleError::UnsupportedVersion(10)),
        "expected UnsupportedVersion(10), got {err:?}"
    );
}

#[test]
fn forged_nonzero_wrapper_typedef_count_is_rejected() {
    // A wrapper/alias TypeDef uses `data` for its inner id but reserves `count`
    // (byte 2) as zero. A non-zero count must be rejected.
    let mut bytes = emit_bytes(r#"Top = (program (expression_statement)* @stmts)"#);
    let (defs_off, wrapper_idx) = {
        let m = Module::load_compiler_output(&bytes).expect("module validates before tampering");
        let types = m.types();
        let idx = (0..types.defs_count())
            .find(|&i| matches!(types.def(i).decode(), TypeDefKind::Wrapper { .. }))
            .expect("list query must emit a wrapper type def");
        (m.offsets().type_defs as usize, idx)
    };

    bytes[defs_off + wrapper_idx * 4 + 2] = 1;
    reseal(&mut bytes);

    let err =
        Module::load_compiler_output(&bytes).expect_err("forged wrapper count must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidTypeDef(_)),
        "expected InvalidTypeDef, got {err:?}"
    );
}

#[test]
fn forged_oob_type_name_type_id_is_rejected() {
    // A TypeNameEntry's target `type_id` (bytes 2-3 of the 4-byte entry) must address a
    // real TypeDef; a named definition emits at least one entry.
    let mut bytes = emit_bytes(RECORD_QUERY);
    let (names_off, type_defs) = {
        let m = Module::load_compiler_output(&bytes).expect("module validates before tampering");
        assert!(
            m.types().names_count() > 0,
            "named def must emit a type name"
        );
        (m.offsets().type_names as usize, m.header().type_defs_count)
    };

    bytes[names_off + 2..names_off + 4].copy_from_slice(&type_defs.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::load_compiler_output(&bytes)
        .expect_err("forged type name type id must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidTypeName(_)),
        "expected InvalidTypeName, got {err:?}"
    );
}

/// Build a minimal-but-valid module whose only populated section is Instructions,
/// carrying `instruction_word_count` bytecode words drawn from `instructions`.
///
/// Everything else (strings, regex, types, entry points) is empty, which the
/// other `validate_*` passes accept: the empty string/regex tables collapse to a
/// single zero sentinel that already lives in the zero-filled body. The checksum
/// is recomputed the same way `Module::validate` checks it (CRC32 over the
/// post-header bytes), so the only thing under test is `validate_instructions`.
fn module_with_instructions(instructions: &[u8], instruction_word_count: u16) -> Vec<u8> {
    let mut header = Header {
        instruction_word_count,
        ..Default::default()
    };

    let offsets = header.compute_offsets();
    let base = offsets.instructions as usize;
    let total = offsets.spans as usize;

    let mut bytes = vec![0u8; total];
    bytes[base..base + instructions.len()].copy_from_slice(instructions);

    header.total_size = total as u32;
    header.checksum = crc32fast::hash(&bytes[64..]);
    bytes[..64].copy_from_slice(&header.to_bytes());

    bytes
}

/// Header byte for a Match instruction with node_class_bits = Any (0).
fn match_header(opcode: u8) -> u8 {
    opcode & 0xF
}

#[test]
fn byte_storage_copy_from_slice() {
    let data = [1u8, 2, 3, 4, 5];
    let storage = ByteStorage::from_emitted_bytes(&data);

    assert_eq!(&*storage, &data[..]);
    assert_eq!(storage.len(), 5);
    assert_eq!(storage[2], 3);
}

#[test]
fn module_error_display() {
    let err = ModuleError::InvalidMagic;
    assert_eq!(err.to_string(), "invalid magic: expected PTKQ");

    let err = ModuleError::UnsupportedVersion(99);
    assert!(err.to_string().contains("99"));

    let err = ModuleError::BufferTooSmall(32);
    assert!(err.to_string().contains("32"));

    let err = ModuleError::SizeMismatch {
        header: 100,
        actual: 50,
    };
    assert!(err.to_string().contains("100"));
    assert!(err.to_string().contains("50"));
}

#[test]
fn load_accepts_single_terminal_match8() {
    // Sanity baseline: one Match8 terminal (opcode 0x0, no successor) loads.
    let mut word = [0u8; BYTECODE_WORD_SIZE];
    word[0] = match_header(0x0);
    let bytes = module_with_instructions(&word, 1);

    let module =
        Module::load_compiler_output(&bytes).expect("valid representation should validate");

    assert_eq!(module.header().instruction_word_count, 1);
}

#[test]
fn load_rejects_invalid_opcode_at_reachable_address() {
    // Address 0 carries an unknown opcode nibble (0xF); the linear walk lands on it
    // immediately and rejects.
    let mut word = [0u8; BYTECODE_WORD_SIZE];
    word[0] = 0xF;
    let bytes = module_with_instructions(&word, 1);

    let err = Module::load_compiler_output(&bytes).expect_err("unknown opcode must be rejected");

    assert!(matches!(
        err,
        ModuleError::InvalidOpcode {
            addr: CodeAddr::ZERO,
            opcode: 0xF
        }
    ));
}

#[test]
fn load_walks_past_extended_match_payload() {
    // A Match16 (opcode 0x1) occupies two words. Its interior payload word
    // (address 1) is poisoned with an invalid opcode nibble: a correct
    // `addr += word_count` walk advances from address 0 straight to address 2 and
    // never inspects it, so the representation still validates. A buggy walk that advanced
    // one word at a time would land on the poison and false-reject.
    let mut instructions = [0u8; BYTECODE_WORD_SIZE * 2];
    instructions[0] = match_header(0x1);
    instructions[BYTECODE_WORD_SIZE] = 0xF; // interior payload, not an instruction boundary
    let bytes = module_with_instructions(&instructions, 2);

    let module =
        Module::load_compiler_output(&bytes).expect("extended match payload must not false-reject");

    assert_eq!(module.header().instruction_word_count, 2);
}

/// Byte offset of the first negated-field slot in the instruction stream.
fn first_neg_slot(bytes: &[u8]) -> usize {
    let (base, word_count) = instruction_section(bytes);
    let mut addr = CodeAddr::ZERO;
    while addr.get() < word_count {
        let off = base + addr.as_usize() * BYTECODE_WORD_SIZE;
        let opcode = bytes[off] & 0x0F;
        if (1..=5).contains(&opcode) {
            let counts = u16::from_le_bytes([bytes[off + 6], bytes[off + 7]]);
            let effects = ((counts >> 12) & 0xF) as usize;
            let neg = ((counts >> 9) & 0x7) as usize;
            if neg > 0 {
                return off + 8 + effects * 2;
            }
        }
        addr = addr
            .checked_add((instr_size(opcode) / BYTECODE_WORD_SIZE) as u16)
            .expect("instruction address fits in u16");
    }
    panic!("query must emit a negated-field slot");
}

#[test]
fn forged_zero_neg_field_is_rejected() {
    // Neg-field slots decode through `NodeFieldId::try_from(raw)` (`NonZeroU16`);
    // a forged zero would panic `neg_fields()` in the VM or `dump`.
    let mut bytes = emit_bytes(r#"Q = (variable_declarator -value)"#);

    let off = first_neg_slot(&bytes);
    bytes[off..off + 2].copy_from_slice(&0u16.to_le_bytes());
    reseal(&mut bytes);

    let err =
        Module::load_compiler_output(&bytes).expect_err("forged zero neg field must be rejected");
    assert!(
        matches!(err, ModuleError::MalformedInstructionStream),
        "expected MalformedInstructionStream, got {err:?}"
    );
}

#[test]
fn forged_zero_node_symbol_is_rejected() {
    // The `symbol` half of a node-kind entry decodes as `NodeKindId`
    // (`NonZeroU16`) in the renderers; a forged zero would panic `dump`/`trace`.
    let mut bytes = emit_bytes(r#"Top = (identifier) @id"#);
    let off = Module::load_compiler_output(&bytes)
        .expect("module validates before tampering")
        .offsets()
        .node_kinds as usize;

    bytes[off..off + 2].copy_from_slice(&0u16.to_le_bytes());
    reseal(&mut bytes);

    let err =
        Module::load_compiler_output(&bytes).expect_err("forged node symbol must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidNodeSymbol(0)),
        "expected InvalidNodeSymbol(0), got {err:?}"
    );
}
