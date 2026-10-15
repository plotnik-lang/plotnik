use std::fmt::Write as _;

use crate::compiler::test_utils::synthetic_grammar as grammar;
use crate::compiler::{BytecodeConfig, BytecodeInspection, QueryBuilder, SourceMap, SourcePath};
use indoc::indoc;

use super::verify::{
    body_analyses as loader_body_analyses, reset_body_analyses as reset_loader_body_analyses,
};
use super::{Module, ModuleError, Opcode};
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
    Q = (program (B) @x :: text)
"#};

#[test]
fn serialized_expression_root_is_verified_before_execution() {
    let mut sources = SourceMap::new();
    let saved = sources.add_file(SourcePath::new("query.ptk"), "Named = (program)");
    let expression = sources.add_stdin("(program) @root");
    let mut collection = crate::compiler::SourceCollection::new(sources);
    collection.include(saved);
    let prepared = QueryBuilder::from_sources(collection)
        .with_expression(expression)
        .compile_dynamic(grammar())
        .unwrap();
    let module = prepared
        .emit(BytecodeConfig::new())
        .unwrap()
        .into_artifact()
        .unwrap();
    assert_eq!(module.entrypoint_count(), 2);
    let expression = module.expression_entrypoint().unwrap();
    let named = module.entrypoint("Named").unwrap();
    let offset = module.offsets().entrypoints as usize;

    let mut reordered = module.bytes().to_vec();
    reordered[offset..offset + 16].rotate_left(8);
    reseal(&mut reordered);
    let reloaded = Module::validate_and_load(&reordered).unwrap();
    assert_eq!(reloaded.entrypoint_at(0), Some(expression));
    assert_eq!(reloaded.expression_entrypoint(), Some(expression));
    assert_eq!(reloaded.entrypoint("Named"), Some(named));
    assert_eq!(reloaded.entrypoint_names().collect::<Vec<_>>(), ["Named"]);

    let mut duplicate = reordered.clone();
    duplicate[offset + 8..offset + 10].copy_from_slice(&0u16.to_le_bytes());
    reseal(&mut duplicate);
    assert!(matches!(
        Module::validate_and_load(&duplicate),
        Err(ModuleError::InvalidEntrypoint(1))
    ));

    let mut broken = reordered;
    let boundary_offset = offset + 6;
    broken[boundary_offset..boundary_offset + 2].copy_from_slice(&0u16.to_le_bytes());
    reseal(&mut broken);
    assert!(matches!(
        Module::validate_and_load(&broken),
        Err(ModuleError::EffectStackImbalance(_))
    ));
}

const ROUTED_CALL_QUERY: &str = indoc! {r#"
    B = (expression_statement)
    A = (statement_block (A)+ (identifier))?
    Q = (program (B) (A) (expression_statement) @e)
"#};

const CALLER_OWNED_CALL_QUERY: &str = indoc! {r#"
    Body = (expression_statement)
    Q = (program (Body))
"#};

const BOUNDARY_NAV_CALL_QUERY: &str = indoc! {r#"
    B = (expression_statement)
    Q = (program (comment) . (B))
"#};

const BOUNDARY_FIELD_CALL_QUERY: &str = indoc! {r#"
    B = (identifier)
    Q = (program
      (lexical_declaration
        (variable_declarator
          .
          name: (B)
        )
      )
    )
"#};

fn reseal(bytes: &mut [u8]) {
    let crc = crc32fast::hash(&bytes[64..]);
    bytes[8..12].copy_from_slice(&crc.to_le_bytes());
}

const MANY_DEFINITIONS: usize = 4_096;
const MAX_BODY_ANALYSES_PER_DEFINITION: usize = 8;

fn assert_linear_body_analyses(stage: &str, analyses: usize) {
    assert!(
        analyses <= MANY_DEFINITIONS * MAX_BODY_ANALYSES_PER_DEFINITION,
        "{stage} performed {analyses} body analyses for {MANY_DEFINITIONS} definitions"
    );
}

#[test]
fn many_callable_definitions_load_without_global_fixpoint_rescans() {
    let mut query = String::new();
    for index in 0..MANY_DEFINITIONS {
        writeln!(query, "Query{index} = (identifier)").expect("writing to a string succeeds");
    }

    reset_loader_body_analyses();
    let bytes = emit_bytes(&query);
    let emission_load_analyses = loader_body_analyses();
    assert!(
        bytes.len() > 63 * 1_024,
        "regression module must stay large"
    );

    assert_linear_body_analyses("emission loader", emission_load_analyses);

    reset_loader_body_analyses();
    let module = Module::validate_and_load(&bytes).expect("compiler output validates");
    let reload_analyses = loader_body_analyses();
    assert_linear_body_analyses("module reload", reload_analyses);

    assert_eq!(module.entrypoints().len(), MANY_DEFINITIONS);
}

fn find_predicate_off(bytes: &[u8]) -> usize {
    let (base, word_count) = {
        let m = Module::validate_and_load(bytes).expect("module validates before tampering");
        (
            m.offsets().instructions as usize,
            m.header().instruction_word_count,
        )
    };
    let mut addr = CodeAddr::ZERO;
    while addr.get() < word_count {
        let instr = base + addr.as_usize() * BYTECODE_WORD_SIZE;
        let opcode = bytes[instr] & 0x0F;
        let size = instr_size(opcode);
        if (1..=5).contains(&opcode) {
            let counts = u16::from_le_bytes([bytes[instr + 6], bytes[instr + 7]]);
            if counts & 7 != 0 {
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
fn forged_invalid_entrypoint_name_is_rejected() {
    let mut bytes = emit_bytes(r#"Top = (identifier) @id"#);
    let ep_off = Module::validate_and_load(&bytes)
        .expect("module validates before tampering")
        .offsets()
        .entrypoints as usize;

    bytes[ep_off..ep_off + 2].copy_from_slice(&u16::MAX.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes)
        .expect_err("out-of-range named identity must be rejected");
    assert!(matches!(err, ModuleError::InvalidStringId(_)));
}

#[test]
fn forged_out_of_range_predicate_operand_is_rejected() {
    let mut bytes = emit_bytes(r#"Q = (identifier == "needle")"#);
    let str_count = Module::validate_and_load(&bytes)
        .expect("module validates before tampering")
        .header()
        .str_table_count;

    let pred_off = find_predicate_off(&bytes);
    bytes[pred_off..pred_off + 2].copy_from_slice(&str_count.to_le_bytes());
    reseal(&mut bytes);

    let err =
        Module::validate_and_load(&bytes).expect_err("forged predicate operand must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidPredicateOperand(_)),
        "expected InvalidPredicateOperand, got {err:?}"
    );
}

#[test]
fn forged_out_of_range_regex_predicate_operand_is_rejected() {
    let mut bytes = emit_bytes(r#"Q = (identifier !~ /needle/)"#);
    let regex_count = Module::validate_and_load(&bytes)
        .expect("module validates before tampering")
        .header()
        .regex_table_count;

    let pred_off = find_predicate_off(&bytes);
    bytes[pred_off..pred_off + 2].copy_from_slice(&regex_count.to_le_bytes());
    reseal(&mut bytes);
    let err = Module::validate_and_load(&bytes).expect_err("regex index is outside its table");

    assert!(matches!(err, ModuleError::InvalidPredicateOperand(_)));
}

const RECORD_QUERY: &str =
    r#"Top = (binary_expression left: (identifier) @l right: (identifier) @r)"#;

fn instruction_section(bytes: &[u8]) -> (usize, u16) {
    let m = Module::validate_and_load(bytes).expect("module validates before tampering");
    (
        m.offsets().instructions as usize,
        m.header().instruction_word_count,
    )
}

fn instr_size(opcode: u8) -> usize {
    Opcode::from_u8(opcode)
        .unwrap_or_else(|| panic!("unexpected opcode {opcode}"))
        .size()
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

fn first_call(bytes: &[u8], callee_owned: bool) -> usize {
    let (base, word_count) = instruction_section(bytes);
    let mut addr = CodeAddr::ZERO;
    while addr.get() < word_count {
        let off = base + addr.as_usize() * BYTECODE_WORD_SIZE;
        let opcode = bytes[off] & 0x0F;
        let owns_entry = (bytes[off] >> 4) & 1 != 0;
        if matches!(Opcode::from_u8(opcode), Some(Opcode::Call1 | Opcode::CallN))
            && owns_entry == callee_owned
        {
            return off;
        }
        addr = addr
            .checked_add((instr_size(opcode) / BYTECODE_WORD_SIZE) as u16)
            .expect("instruction address fits in u16");
    }
    panic!("no matching call in the instruction stream");
}

fn raw_call_target(bytes: &[u8], call_off: usize) -> u16 {
    match bytes[call_off] & 0x0F {
        opcode if opcode == Opcode::Call1 as u8 => {
            u16::from_le_bytes([bytes[call_off + 6], bytes[call_off + 7]])
        }
        opcode if opcode == Opcode::CallN as u8 => {
            u16::from_le_bytes([bytes[call_off + 4], bytes[call_off + 5]])
        }
        opcode => panic!("expected Call1 or CallN, got opcode {opcode}"),
    }
}

fn calls_to_target(bytes: &[u8], target: u16) -> usize {
    let (base, word_count) = instruction_section(bytes);
    let mut count = 0;
    let mut addr = CodeAddr::ZERO;
    while addr.get() < word_count {
        let off = base + addr.as_usize() * BYTECODE_WORD_SIZE;
        let opcode = bytes[off] & 0x0F;
        if matches!(Opcode::from_u8(opcode), Some(Opcode::Call1 | Opcode::CallN))
            && raw_call_target(bytes, off) == target
        {
            count += 1;
        }
        addr = addr
            .checked_add((instr_size(opcode) / BYTECODE_WORD_SIZE) as u16)
            .expect("instruction address fits in u16");
    }
    count
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
            let has_pred = counts & 7 != 0;
            if succ > 0 {
                return off + 8 + (effects + neg) * 2 + if has_pred { 2 } else { 0 };
            }
        }
        addr = addr
            .checked_add((instr_size(opcode) / BYTECODE_WORD_SIZE) as u16)
            .expect("instruction address fits in u16");
    }
    panic!("no extended-match successor in the instruction stream");
}

fn first_effect_op(bytes: &[u8], want: impl Fn(u16) -> bool) -> usize {
    effect_slots(bytes)
        .into_iter()
        .find(|&off| want(u16::from_le_bytes([bytes[off], bytes[off + 1]]) >> EFFECT_PAYLOAD_BITS))
        .expect("no matching effect slot in the instruction stream")
}

fn first_instruction_with_effect(bytes: &[u8], want: impl Fn(u16) -> bool) -> usize {
    let (base, word_count) = instruction_section(bytes);
    let mut addr = CodeAddr::ZERO;
    while addr.get() < word_count {
        let off = base + addr.as_usize() * BYTECODE_WORD_SIZE;
        let opcode = bytes[off] & 0x0F;
        if (1..=5).contains(&opcode) {
            let counts = u16::from_le_bytes([bytes[off + 6], bytes[off + 7]]);
            let effects = ((counts >> 12) & 0xF) as usize;
            for index in 0..effects {
                let effect = off + BYTECODE_WORD_SIZE + index * 2;
                let op =
                    u16::from_le_bytes([bytes[effect], bytes[effect + 1]]) >> EFFECT_PAYLOAD_BITS;
                if want(op) {
                    return off;
                }
            }
        }
        addr = addr
            .checked_add((instr_size(opcode) / BYTECODE_WORD_SIZE) as u16)
            .expect("instruction address fits in u16");
    }
    panic!("no instruction with a matching effect");
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
    let m = Module::validate_and_load(bytes).expect("module validates before tampering");
    m.offsets().type_members as usize + member as usize * 4 + 2
}

fn first_section_gap(bytes: &[u8]) -> usize {
    let m = Module::validate_and_load(bytes).expect("module validates before tampering");
    let o = m.offsets();
    let h = m.header();
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
        (o.entrypoints, h.entrypoints_count as u32 * 8),
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
    let mut bytes = emit_bytes(RECORD_QUERY);
    let pad_off = first_section_gap(&bytes);
    bytes[pad_off] = 1;
    reseal(&mut bytes);

    let err =
        Module::validate_and_load(&bytes).expect_err("forged section padding must be rejected");
    assert!(
        matches!(err, ModuleError::NonZeroSectionPadding),
        "expected NonZeroSectionPadding, got {err:?}"
    );
}

#[test]
fn forged_unknown_opcode_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let off = first_instr(&bytes, |_| true);
    bytes[off] = (bytes[off] & 0xF0) | 0x0A;
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes).expect_err("forged opcode must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidOpcode { opcode: 0x0A, .. }),
        "expected InvalidOpcode, got {err:?}"
    );
}

#[test]
fn forged_nonzero_segment_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let off = first_instr(&bytes, |_| true);
    bytes[off] |= 0x40;
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes).expect_err("forged segment must be rejected");
    assert!(
        matches!(err, ModuleError::MalformedInstructionStream),
        "expected MalformedInstructionStream, got {err:?}"
    );
}

#[test]
fn forged_reserved_return_header_flag_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let off = first_instr(&bytes, |opcode| opcode == Opcode::Return as u8);
    bytes[off] |= 0x20;
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes).expect_err("forged return flag must be rejected");
    assert!(matches!(err, ModuleError::MalformedInstructionStream));
}

#[test]
fn forged_reserved_node_kind_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let off = first_instr(&bytes, |o| o <= 5);
    bytes[off] |= 0x30;
    reseal(&mut bytes);

    let err =
        Module::validate_and_load(&bytes).expect_err("forged node_class_bits must be rejected");
    assert!(
        matches!(err, ModuleError::MalformedInstructionStream),
        "expected MalformedInstructionStream, got {err:?}"
    );
}

#[test]
fn forged_invalid_nav_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let off = first_instr(&bytes, |o| o <= 5);
    bytes[off + 1] = 0x80;
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes).expect_err("forged nav must be rejected");
    assert!(
        matches!(err, ModuleError::MalformedInstructionStream),
        "expected MalformedInstructionStream, got {err:?}"
    );
}

#[test]
fn forged_invalid_effect_opcode_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let slot = effect_slots(&bytes)[0];
    let existing = u16::from_le_bytes([bytes[slot], bytes[slot + 1]]);
    let invalid_op = EffectKind::BoolValue as u16 + 1;
    let forged = (invalid_op << EFFECT_PAYLOAD_BITS) | (existing & EFFECT_PAYLOAD_MAX as u16);
    bytes[slot..slot + 2].copy_from_slice(&forged.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes).expect_err("forged effect opcode must be rejected");
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

    let err =
        Module::validate_and_load(&bytes).expect_err("unit effects must reject a nonzero payload");
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

    let err = Module::validate_and_load(&bytes)
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

    let err = Module::validate_and_load(&bytes).expect_err("span effects need a spans section");
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

    let err =
        Module::validate_and_load(&bytes).expect_err("span payload must address an existing span");
    assert!(
        matches!(err, ModuleError::InvalidSpanPayload(_)),
        "expected InvalidSpanPayload, got {err:?}"
    );
}

#[test]
fn forged_oob_member_operand_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let members = Module::validate_and_load(&bytes)
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
        Module::validate_and_load(&bytes).expect_err("forged member operand must be rejected");
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

    let err = Module::validate_and_load(&bytes).expect_err("forged span kind must be rejected");
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

    let err = Module::validate_and_load(&bytes).expect_err("forged span range must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidSpanEntry(0)),
        "expected InvalidSpanEntry, got {err:?}"
    );
}

#[test]
fn forged_member_binding_without_type_is_rejected() {
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

    let err = Module::validate_and_load(&bytes)
        .expect_err("member binding without type must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidSpanEntry(0)),
        "expected InvalidSpanEntry, got {err:?}"
    );
}

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
    let mut bytes = emit_inspection_bytes(RECORD_QUERY);
    let slot = *effect_slots_of(&bytes, &[EffectKind::SpanEnd])
        .first()
        .expect("inspection module must emit a SpanEnd");
    let payload = u16::from_le_bytes([bytes[slot], bytes[slot + 1]]) & EFFECT_PAYLOAD_MAX as u16;
    bytes[slot..slot + 2]
        .copy_from_slice(&effect_word_with_payload(EffectKind::SpanStart, payload));
    reseal(&mut bytes);

    let err =
        Module::validate_and_load(&bytes).expect_err("unclosed span bracket must be rejected");
    assert!(
        matches!(err, ModuleError::SpanImbalance(_)),
        "expected SpanImbalance, got {err:?}"
    );
}

#[test]
fn forged_unopened_span_close_is_rejected() {
    let mut bytes = emit_inspection_bytes(RECORD_QUERY);
    let slot = *effect_slots_of(&bytes, &[EffectKind::SpanStart, EffectKind::SpanStartAt])
        .first()
        .expect("inspection module must emit a span open");
    let payload = u16::from_le_bytes([bytes[slot], bytes[slot + 1]]) & EFFECT_PAYLOAD_MAX as u16;
    bytes[slot..slot + 2].copy_from_slice(&effect_word_with_payload(EffectKind::SpanEnd, payload));
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes).expect_err("unopened span close must be rejected");
    assert!(
        matches!(err, ModuleError::SpanImbalance(_)),
        "expected SpanImbalance, got {err:?}"
    );
}

#[test]
fn forged_mispaired_span_ids_are_rejected() {
    let mut bytes = emit_inspection_bytes(RECORD_QUERY);
    let spans_count = Module::validate_and_load(&bytes)
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

    let err = Module::validate_and_load(&bytes).expect_err("mis-paired span ids must be rejected");
    assert!(
        matches!(err, ModuleError::SpanImbalance(_)),
        "expected SpanImbalance, got {err:?}"
    );
}

#[test]
fn forged_invalid_span_binding_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let module = Module::validate_and_load(&bytes).expect("module validates before tampering");
    let span = SpanEntry {
        source_id: 0,
        kind: SpanKind::Capture,
        start: 3,
        end: 8,
        type_id: module.header().type_defs_count,
        member: SPAN_NO_BINDING,
    };
    add_single_span(&mut bytes, span.to_bytes());

    let err = Module::validate_and_load(&bytes).expect_err("forged span binding must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidSpanEntry(0)),
        "expected InvalidSpanEntry, got {err:?}"
    );
}

#[test]
fn forged_zero_successor_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let succ_off = first_ext_successor(&bytes);
    bytes[succ_off..succ_off + 2].copy_from_slice(&0u16.to_le_bytes());
    reseal(&mut bytes);

    let err =
        Module::validate_and_load(&bytes).expect_err("forged zero successor must be rejected");
    assert!(
        matches!(err, ModuleError::MalformedInstructionStream),
        "expected MalformedInstructionStream, got {err:?}"
    );
}

#[test]
fn forged_out_of_range_successor_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let succ_off = first_ext_successor(&bytes);
    bytes[succ_off..succ_off + 2].copy_from_slice(&u16::MAX.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes)
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

    let err = Module::validate_and_load(&bytes)
        .expect_err("forged cursor-depth imbalance must be rejected");
    assert!(
        matches!(err, ModuleError::DepthImbalance(_)),
        "expected DepthImbalance, got {err:?}"
    );
}

#[test]
fn forged_regex_pattern_string_id_is_rejected() {
    for forged in [0u16, u16::MAX] {
        let mut bytes = emit_bytes(r#"Q = (identifier =~ /x/)"#);
        let (regex_off, regex_count, str_count) = {
            let m = Module::validate_and_load(&bytes).expect("module validates before tampering");
            (
                m.offsets().regex_table as usize,
                m.header().regex_table_count,
                m.header().str_table_count,
            )
        };
        assert!(regex_count > 1, "query must emit a regex entry");

        let value = if forged == u16::MAX {
            str_count
        } else {
            forged
        };
        let sid_off = regex_off + 8;
        bytes[sid_off..sid_off + 2].copy_from_slice(&value.to_le_bytes());
        reseal(&mut bytes);

        let err =
            Module::validate_and_load(&bytes).expect_err("forged regex string_id must be rejected");
        assert!(
            matches!(err, ModuleError::InvalidStringId(_)),
            "forged regex string_id {value}: expected InvalidStringId, got {err:?}"
        );
    }
}

#[test]
fn forged_nonzero_regex_table_reserved_is_rejected() {
    let mut bytes = emit_bytes(r#"Q = (identifier =~ /x/)"#);
    let (regex_off, regex_count) = {
        let m = Module::validate_and_load(&bytes).expect("module validates before tampering");
        (
            m.offsets().regex_table as usize,
            m.header().regex_table_count,
        )
    };
    assert!(regex_count > 1, "query must emit a regex entry");

    let reserved_off = regex_off + 8 + 2;
    bytes[reserved_off..reserved_off + 2].copy_from_slice(&1u16.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes)
        .expect_err("forged regex reserved field must be rejected");
    assert!(
        matches!(err, ModuleError::MalformedRegexTable),
        "expected MalformedRegexTable, got {err:?}"
    );
}

#[test]
fn forged_corrupt_regex_dfa_is_rejected() {
    let mut bytes = emit_bytes(r#"Q = (identifier =~ /x/)"#);
    let (blob_off, blob_len) = {
        let m = Module::validate_and_load(&bytes).expect("module validates before tampering");
        (
            m.offsets().regex_blob as usize,
            m.header().regex_blob_size as usize,
        )
    };
    assert!(blob_len > 0, "query must emit a DFA blob");

    bytes[blob_off..blob_off + blob_len].fill(0xFF);
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes).expect_err("forged regex DFA must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidRegexDfa(_)),
        "expected InvalidRegexDfa, got {err:?}"
    );
}

#[test]
fn forged_entrypoint_into_instruction_interior_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let (ep_off, interior) = {
        let m = Module::validate_and_load(&bytes).expect("module validates before tampering");
        (
            m.offsets().entrypoints as usize,
            first_multiword_interior_addr(&bytes).get(),
        )
    };

    bytes[ep_off + 2..ep_off + 4].copy_from_slice(&interior.to_le_bytes());
    reseal(&mut bytes);

    let err =
        Module::validate_and_load(&bytes).expect_err("forged interior entrypoint must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidEntrypoint(_)),
        "expected InvalidEntrypoint, got {err:?}"
    );
}

#[test]
fn entry_and_call_targets_may_address_word_zero() {
    let bytes = emit_bytes(CALLER_OWNED_CALL_QUERY);
    let module = Module::validate_and_load(&bytes).expect("compiler output validates");
    let first_entry = module.entrypoints().get(0);
    let call = first_call(&bytes, false);

    assert_eq!(first_entry.target(), CodeAddr::ZERO);
    assert_eq!(raw_call_target(&bytes, call), 0);
}

#[test]
fn forged_record_set_to_array_push_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let slot = first_effect_op(&bytes, |op| op == EffectKind::RecordSet as u16);
    bytes[slot..slot + 2].copy_from_slice(&effect_word(EffectKind::ArrayPush));
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes)
        .expect_err("forged RecordSet->ArrayPush must be rejected");
    assert!(
        matches!(err, ModuleError::EffectStackImbalance(_)),
        "expected EffectStackImbalance, got {err:?}"
    );
}

#[test]
fn forged_scalar_capture_record_set_to_array_push_is_rejected() {
    let mut bytes = emit_bytes(r#"Q = (identifier) @id"#);
    let slot = first_effect_op(&bytes, |op| op == EffectKind::RecordSet as u16);
    bytes[slot..slot + 2].copy_from_slice(&effect_word(EffectKind::ArrayPush));
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes)
        .expect_err("forged scalar RecordSet->ArrayPush must be rejected");
    assert!(
        matches!(err, ModuleError::EffectStackImbalance(_)),
        "expected EffectStackImbalance, got {err:?}"
    );
}

#[test]
fn forged_record_set_without_producer_is_rejected() {
    let mut bytes = emit_bytes(r#"Q = (identifier) @id"#);
    let slot = first_effect_op(&bytes, |op| op == EffectKind::Node as u16);
    bytes[slot..slot + 2].copy_from_slice(&effect_word(EffectKind::RecordOpen));
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes)
        .expect_err("forged producerless RecordSet must be rejected");
    assert!(
        matches!(err, ModuleError::EffectStackImbalance(_)),
        "expected EffectStackImbalance, got {err:?}"
    );
}

#[test]
fn forged_cursor_read_on_empty_path_is_rejected() {
    let mut bytes = emit_bytes(r#"Q = (program) @node"#);
    let instruction = first_instruction_with_effect(&bytes, |op| op == EffectKind::Node as u16);
    bytes[instruction + 1] = Nav::Epsilon.to_byte();
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes)
        .expect_err("cursor read on an empty path must be rejected");
    assert!(matches!(err, ModuleError::EmptyPathCursorRead(_)));
}

#[test]
fn forged_no_payload_variant_case_with_data_is_rejected() {
    let mut bytes = emit_bytes(r#"Q = [A: (identifier) @a B: (number)]"#);
    let variant_open = first_effect_op(&bytes, |op| op == EffectKind::VariantOpen as u16);
    let variant_member = effect_payload(&bytes, variant_open);
    let type_id_off = type_member_type_id_off(&bytes, variant_member);

    bytes[type_id_off..type_id_off + 2].copy_from_slice(&0u16.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes)
        .expect_err("forged data on a no-payload variant case must be rejected");
    assert!(
        matches!(err, ModuleError::EffectStackImbalance(_)),
        "expected EffectStackImbalance, got {err:?}"
    );
}

#[test]
fn forged_data_variant_case_without_data_is_rejected() {
    let mut bytes = emit_bytes(r#"Q = [A: (identifier)]"#);
    let type_count = Module::validate_and_load(&bytes)
        .expect("module validates before tampering")
        .header()
        .type_defs_count;
    assert!(type_count > 1, "query must emit a value type to point at");

    let variant_open = first_effect_op(&bytes, |op| op == EffectKind::VariantOpen as u16);
    let variant_member = effect_payload(&bytes, variant_open);
    let type_id_off = type_member_type_id_off(&bytes, variant_member);

    bytes[type_id_off..type_id_off + 2].copy_from_slice(&1u16.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes)
        .expect_err("forged missing variant data must be rejected");
    assert!(
        matches!(err, ModuleError::EffectStackImbalance(_)),
        "expected EffectStackImbalance, got {err:?}"
    );
}

#[test]
fn forged_dropped_scope_close_is_rejected() {
    let mut bytes = emit_bytes(indoc! {r#"
        Q = (program
          (function_declaration
            name: (identifier) @name
          )* @funcs
        )
    "#});
    let slot = first_effect_op(&bytes, |op| op == EffectKind::RecordClose as u16);
    bytes[slot..slot + 2].copy_from_slice(&effect_word(EffectKind::Node));
    reseal(&mut bytes);

    let err =
        Module::validate_and_load(&bytes).expect_err("forged dropped RecordClose must be rejected");
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
          } @chunk :: text
        )
    "#});
    let slot = first_effect_op(&bytes, |op| op == EffectKind::ScalarOpen as u16);
    bytes[slot..slot + 2].copy_from_slice(&effect_word(EffectKind::RecordOpen));
    reseal(&mut bytes);

    let err =
        Module::validate_and_load(&bytes).expect_err("TextClose must close a ScalarOpen frame");
    assert!(matches!(err, ModuleError::EffectStackImbalance(_)));
}

#[test]
fn forged_suppress_underflow_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let slot = first_effect_op(&bytes, |op| {
        op == EffectKind::RecordOpen as u16 || op == EffectKind::RecordSet as u16
    });
    bytes[slot..slot + 2].copy_from_slice(&effect_word(EffectKind::SuppressEnd));
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes)
        .expect_err("forged SuppressEnd underflow must be rejected");
    assert!(matches!(err, ModuleError::EffectStackImbalance(_)));
}

fn last_return_addr(bytes: &[u8]) -> CodeAddr {
    let (base, word_count) = instruction_section(bytes);
    let mut found = None;
    let mut addr = CodeAddr::ZERO;
    while addr.get() < word_count {
        let off = base + addr.as_usize() * BYTECODE_WORD_SIZE;
        let opcode = bytes[off] & 0x0F;
        if opcode == Opcode::Return as u8 {
            found = Some(addr);
        }
        addr = addr
            .checked_add((instr_size(opcode) / BYTECODE_WORD_SIZE) as u16)
            .expect("instruction address fits in u16");
    }
    found.expect("no Return in the instruction stream")
}

#[test]
fn forged_returnless_entry_body_is_rejected() {
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

    let err = Module::validate_and_load(&bytes)
        .expect_err("forged returnless entry body must be rejected");
    assert!(matches!(err, ModuleError::MalformedInstructionStream));
}

#[test]
fn forged_record_entry_without_root_boundary_is_rejected() {
    let mut bytes = emit_bytes(r#"Q = (_) @x"#);
    let ep_off = Module::validate_and_load(&bytes)
        .expect("module validates before tampering")
        .offsets()
        .entrypoints as usize;
    bytes[ep_off + 6..ep_off + 8].copy_from_slice(&0u16.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes)
        .expect_err("forged rootless entry boundary must be rejected");
    assert!(
        matches!(err, ModuleError::EffectStackImbalance(_)),
        "expected EffectStackImbalance, got {err:?}"
    );
}

#[test]
fn forged_nonzero_return_pad_is_rejected() {
    for byte in [3usize, 6, 7] {
        let mut bytes = emit_bytes(RECORD_QUERY);
        let off = first_instr(&bytes, |opcode| opcode == Opcode::Return as u8);
        bytes[off + byte] = 1;
        reseal(&mut bytes);

        let err =
            Module::validate_and_load(&bytes).expect_err("forged return pad must be rejected");
        assert!(
            matches!(err, ModuleError::MalformedInstructionStream),
            "forged byte {byte}: expected MalformedInstructionStream, got {err:?}"
        );
    }
}

#[test]
fn forged_invalid_return_port_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let off = first_instr(&bytes, |opcode| opcode == Opcode::Return as u8);
    bytes[off + 1] = 8;
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes)
        .expect_err("port outside the runtime universe must be rejected");
    assert!(matches!(err, ModuleError::MalformedInstructionStream));
}

#[test]
fn forged_invalid_return_entry_metadata_is_rejected() {
    for mutation in 0..3 {
        let mut bytes = emit_bytes(RECORD_QUERY);
        let off = first_instr(&bytes, |opcode| opcode == Opcode::Return as u8);
        match mutation {
            0 => bytes[off + 2] = 0x80,
            1 => bytes[off + 2] = Nav::Next.to_byte(),
            2 => bytes[off + 4..off + 6].copy_from_slice(&1u16.to_le_bytes()),
            _ => unreachable!("test enumerates every return metadata mutation"),
        }
        reseal(&mut bytes);

        let err = Module::validate_and_load(&bytes)
            .expect_err("malformed return entry metadata must be rejected");
        assert!(matches!(err, ModuleError::MalformedInstructionStream));
    }
}

#[test]
fn forged_calln_invalid_nav_target_and_return_are_rejected() {
    for byte in [1usize, 4, 8] {
        let mut bytes = emit_bytes(SPLIT_CALL_QUERY);
        let off = first_instr(&bytes, |opcode| opcode == Opcode::CallN as u8);
        if byte == 1 {
            bytes[off + byte] = 0x80;
        } else if byte == 4 {
            bytes[off + byte..off + byte + 2].copy_from_slice(&u16::MAX.to_le_bytes());
        } else {
            bytes[off + byte..off + byte + 2].copy_from_slice(&0u16.to_le_bytes());
        }
        reseal(&mut bytes);

        let err =
            Module::validate_and_load(&bytes).expect_err("malformed split call must be rejected");
        assert!(matches!(err, ModuleError::MalformedInstructionStream));
    }
}

#[test]
fn forged_calln_invalid_arity_mask_flags_and_padding_are_rejected() {
    for mutation in 0..6 {
        let mut bytes = emit_bytes(SPLIT_CALL_QUERY);
        let off = first_instr(&bytes, |opcode| opcode == Opcode::CallN as u8);
        let arity = usize::from(bytes[off + 6]);
        match mutation {
            0 => bytes[off + 6] = 0,
            1 => bytes[off + 6] = 1,
            2 => bytes[off + 6] = 9,
            3 => bytes[off + 7] |= 1 << arity,
            4 => bytes[off] |= 0b10 << 4,
            5 => {
                let unused = off + BYTECODE_WORD_SIZE + arity * 2;
                bytes[unused..unused + 2].copy_from_slice(&1u16.to_le_bytes());
            }
            _ => unreachable!("test enumerates every CallN mutation"),
        }
        reseal(&mut bytes);

        let err = Module::validate_and_load(&bytes)
            .expect_err("malformed CallN metadata must be rejected");
        assert!(matches!(err, ModuleError::MalformedInstructionStream));
    }
}

#[test]
fn forged_callee_owned_call1_invalid_nav_and_targets_are_rejected() {
    for byte in [1usize, 4, 6] {
        let mut bytes = emit_bytes(ROUTED_CALL_QUERY);
        let off = first_call(&bytes, true);
        if byte == 1 {
            bytes[off + byte] = 0x80;
        } else {
            bytes[off + byte..off + byte + 2].copy_from_slice(&0u16.to_le_bytes());
        }
        reseal(&mut bytes);

        let err = Module::validate_and_load(&bytes)
            .expect_err("malformed callee-owned Call1 must be rejected");
        assert!(matches!(err, ModuleError::MalformedInstructionStream));
    }
}

#[test]
fn forged_caller_owned_empty_call1_is_rejected() {
    let mut bytes = emit_bytes(CALLER_OWNED_CALL_QUERY);
    let off = first_call(&bytes, false);
    bytes[off] &= !(0b10 << 4);
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes)
        .expect_err("caller-owned calls cannot expose an empty port");
    assert!(matches!(err, ModuleError::MalformedInstructionStream));
}

#[test]
fn forged_caller_and_callee_owned_call_target_mismatches_are_rejected() {
    let mut bytes = emit_bytes(ROUTED_CALL_QUERY);
    let caller = first_call(&bytes, false);
    let callee = first_call(&bytes, true);
    let callee_target = [bytes[callee + 6], bytes[callee + 7]];
    bytes[caller + 6..caller + 8].copy_from_slice(&callee_target);
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes)
        .expect_err("caller-owned call cannot target a callee-owned body");
    assert!(matches!(err, ModuleError::MalformedInstructionStream));

    let mut bytes = emit_bytes(ROUTED_CALL_QUERY);
    let caller = first_call(&bytes, false);
    let callee = first_call(&bytes, true);
    let caller_target = [bytes[caller + 6], bytes[caller + 7]];
    bytes[callee + 6..callee + 8].copy_from_slice(&caller_target);
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes)
        .expect_err("callee-owned call cannot target a caller-owned body");
    assert!(matches!(err, ModuleError::MalformedInstructionStream));
}

#[test]
fn forged_single_call_ownership_mismatch_is_rejected() {
    let mut bytes = emit_bytes(CALLER_OWNED_CALL_QUERY);
    let call = first_call(&bytes, false);
    let target = raw_call_target(&bytes, call);
    assert_eq!(calls_to_target(&bytes, target), 1);

    bytes[call] |= 1 << 4;
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes)
        .expect_err("callee ownership must match the target return contract");
    assert!(matches!(err, ModuleError::MalformedInstructionStream));
}

#[test]
fn forged_single_call_horizontal_navigation_mismatch_is_rejected() {
    let mut bytes = emit_bytes(BOUNDARY_NAV_CALL_QUERY);
    let call = first_call(&bytes, true);
    let target = raw_call_target(&bytes, call);
    assert_eq!(calls_to_target(&bytes, target), 1);

    let original = Nav::from_byte(bytes[call + 1]);
    assert!(matches!(
        original,
        Nav::Next | Nav::NextSkip | Nav::NextSkipExtras | Nav::NextExact
    ));
    bytes[call + 1] = if original == Nav::NextExact {
        Nav::Next.to_byte()
    } else {
        Nav::NextExact.to_byte()
    };
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes)
        .expect_err("same-depth horizontal navigation must match the target return contract");
    assert!(matches!(err, ModuleError::MalformedInstructionStream));
}

#[test]
fn forged_single_call_field_mismatch_is_rejected() {
    let mut bytes = emit_bytes(BOUNDARY_FIELD_CALL_QUERY);
    let call = first_call(&bytes, true);
    let target = raw_call_target(&bytes, call);
    assert_eq!(calls_to_target(&bytes, target), 1);
    assert_ne!(
        u16::from_le_bytes([bytes[call + 2], bytes[call + 3]]),
        0,
        "boundary call must carry the grammar field into its specialization"
    );

    bytes[call + 2..call + 4].copy_from_slice(&0u16.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes)
        .expect_err("grammar field must match the target return contract");
    assert!(matches!(err, ModuleError::MalformedInstructionStream));
}

#[test]
fn forged_call_and_callee_return_contract_mismatch_is_rejected() {
    let mut bytes = emit_bytes(SPLIT_CALL_QUERY);
    let calln = first_instr(&bytes, |opcode| opcode == Opcode::CallN as u8);
    let call1 = first_instr(&bytes, |opcode| opcode == Opcode::Call1 as u8);
    let calln_target = [bytes[calln + 4], bytes[calln + 5]];
    bytes[call1 + 6..call1 + 8].copy_from_slice(&calln_target);
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes)
        .expect_err("ordinary call cannot target a split-return body");
    assert!(matches!(err, ModuleError::MalformedInstructionStream));

    let mut bytes = emit_bytes(SPLIT_CALL_QUERY);
    let calln = first_instr(&bytes, |opcode| opcode == Opcode::CallN as u8);
    let call1 = first_instr(&bytes, |opcode| opcode == Opcode::Call1 as u8);
    let call1_target = [bytes[call1 + 6], bytes[call1 + 7]];
    bytes[calln + 4..calln + 6].copy_from_slice(&call1_target);
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes)
        .expect_err("split call requires a body with both return outcomes");
    assert!(matches!(err, ModuleError::MalformedInstructionStream));
}

#[test]
fn forged_regex_predicate_sentinel_operand_is_rejected() {
    let mut bytes = emit_bytes(r#"Q = (identifier =~ /needle/)"#);
    let pred_off = find_predicate_off(&bytes);
    bytes[pred_off..pred_off + 2].copy_from_slice(&0u16.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes)
        .expect_err("forged regex sentinel operand must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidPredicateOperand(_)),
        "expected InvalidPredicateOperand, got {err:?}"
    );
}

#[test]
fn forged_unknown_type_def_kind_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let type_defs_off = Module::validate_and_load(&bytes)
        .expect("module validates before tampering")
        .offsets()
        .type_defs as usize;
    bytes[type_defs_off + 3] = 0xFF;
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes).expect_err("forged type-def kind must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidTypeDef(_)),
        "expected InvalidTypeDef, got {err:?}"
    );
}

#[test]
fn forged_out_of_range_entrypoint_target_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let ep_off = Module::validate_and_load(&bytes)
        .expect("module validates before tampering")
        .offsets()
        .entrypoints as usize;
    bytes[ep_off + 2..ep_off + 4].copy_from_slice(&u16::MAX.to_le_bytes());
    reseal(&mut bytes);

    let err =
        Module::validate_and_load(&bytes).expect_err("forged out-of-range target must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidEntrypoint(_)),
        "expected InvalidEntrypoint, got {err:?}"
    );
}

#[test]
fn forged_invalid_entrypoint_boundary_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let ep_off = Module::validate_and_load(&bytes)
        .expect("module validates before tampering")
        .offsets()
        .entrypoints as usize;
    bytes[ep_off + 6..ep_off + 8].copy_from_slice(&u16::MAX.to_le_bytes());
    reseal(&mut bytes);

    let err =
        Module::validate_and_load(&bytes).expect_err("forged entrypoint boundary must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidEntrypoint(_)),
        "expected InvalidEntrypoint, got {err:?}"
    );
}

#[test]
fn forged_out_of_range_entrypoint_result_type_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let (ep_off, type_defs) = {
        let m = Module::validate_and_load(&bytes).expect("module validates before tampering");
        (m.offsets().entrypoints as usize, m.header().type_defs_count)
    };
    bytes[ep_off + 4..ep_off + 6].copy_from_slice(&type_defs.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes).expect_err("forged result_type must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidEntrypoint(_)),
        "expected InvalidEntrypoint, got {err:?}"
    );
}

#[test]
fn forged_type_member_name_string_id_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let members_off = Module::validate_and_load(&bytes)
        .expect("module validates before tampering")
        .offsets()
        .type_members as usize;
    bytes[members_off..members_off + 2].copy_from_slice(&0u16.to_le_bytes());
    reseal(&mut bytes);

    let err =
        Module::validate_and_load(&bytes).expect_err("forged type-member name must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidStringId(_)),
        "expected InvalidStringId, got {err:?}"
    );
}

#[test]
fn forged_oob_member_type_id_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let (members_off, type_defs) = {
        let m = Module::validate_and_load(&bytes).expect("module validates before tampering");
        (
            m.offsets().type_members as usize,
            m.header().type_defs_count,
        )
    };

    bytes[members_off + 2..members_off + 4].copy_from_slice(&type_defs.to_le_bytes());
    reseal(&mut bytes);

    let err =
        Module::validate_and_load(&bytes).expect_err("forged member type id must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidTypeDef(_)),
        "expected InvalidTypeDef, got {err:?}"
    );
}

#[test]
fn forged_oob_wrapper_inner_type_id_is_rejected() {
    let mut bytes = emit_bytes(r#"Top = (program (expression_statement)* @stmts)"#);
    let (defs_off, type_defs, wrapper_idx) = {
        let m = Module::validate_and_load(&bytes).expect("module validates before tampering");
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

    let err = Module::validate_and_load(&bytes)
        .expect_err("forged wrapper inner type id must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidTypeDef(_)),
        "expected InvalidTypeDef, got {err:?}"
    );
}

#[test]
fn forged_nonzero_primitive_typedef_reserved_is_rejected() {
    for byte in [0usize, 2] {
        let mut bytes = emit_bytes(RECORD_QUERY);
        let (defs_off, prim_idx) = {
            let m = Module::validate_and_load(&bytes).expect("module validates before tampering");
            let types = m.types();
            let idx = (0..types.defs_count())
                .find(|&i| matches!(types.def(i).decode(), TypeDefKind::Primitive(_)))
                .expect("record query must emit a primitive type def");
            (m.offsets().type_defs as usize, idx)
        };

        bytes[defs_off + prim_idx * 4 + byte] = 1;
        reseal(&mut bytes);

        let err = Module::validate_and_load(&bytes)
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
        (r#"Q = (identifier) @id :: text"#, TypeKind::Text),
        (
            r#"Q = (program (identifier)? @present :: bool)"#,
            TypeKind::Bool,
        ),
    ] {
        for byte in [0usize, 2] {
            let mut bytes = emit_bytes(query);
            let (defs_off, primitive_idx) = {
                let module =
                    Module::validate_and_load(&bytes).expect("module validates before tampering");
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

            let err = Module::validate_and_load(&bytes)
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

    let err = Module::validate_and_load(&bytes)
        .expect_err("v10 modules must be regenerated for the scalar vocabulary");
    assert!(
        matches!(err, ModuleError::UnsupportedVersion(10)),
        "expected UnsupportedVersion(10), got {err:?}"
    );
}

#[test]
fn forged_nonzero_wrapper_typedef_count_is_rejected() {
    let mut bytes = emit_bytes(r#"Top = (program (expression_statement)* @stmts)"#);
    let (defs_off, wrapper_idx) = {
        let m = Module::validate_and_load(&bytes).expect("module validates before tampering");
        let types = m.types();
        let idx = (0..types.defs_count())
            .find(|&i| matches!(types.def(i).decode(), TypeDefKind::Wrapper { .. }))
            .expect("list query must emit a wrapper type def");
        (m.offsets().type_defs as usize, idx)
    };

    bytes[defs_off + wrapper_idx * 4 + 2] = 1;
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes).expect_err("forged wrapper count must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidTypeDef(_)),
        "expected InvalidTypeDef, got {err:?}"
    );
}

#[test]
fn forged_oob_type_name_type_id_is_rejected() {
    let mut bytes = emit_bytes(RECORD_QUERY);
    let (names_off, type_defs) = {
        let m = Module::validate_and_load(&bytes).expect("module validates before tampering");
        assert!(
            m.types().names_count() > 0,
            "named def must emit a type name"
        );
        (m.offsets().type_names as usize, m.header().type_defs_count)
    };

    bytes[names_off + 2..names_off + 4].copy_from_slice(&type_defs.to_le_bytes());
    reseal(&mut bytes);

    let err =
        Module::validate_and_load(&bytes).expect_err("forged type name type id must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidTypeName(_)),
        "expected InvalidTypeName, got {err:?}"
    );
}

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

fn match_header(opcode: u8) -> u8 {
    opcode & 0xF
}

#[test]
fn load_walks_past_extended_match_payload() {
    let mut instructions = [0u8; BYTECODE_WORD_SIZE * 2];
    instructions[0] = match_header(0x1);
    instructions[BYTECODE_WORD_SIZE] = 0xF;
    let bytes = module_with_instructions(&instructions, 2);

    let module =
        Module::validate_and_load(&bytes).expect("extended match payload must not false-reject");

    assert_eq!(module.header().instruction_word_count, 2);
}

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
    let mut bytes = emit_bytes(r#"Q = (variable_declarator -value)"#);

    let off = first_neg_slot(&bytes);
    bytes[off..off + 2].copy_from_slice(&0u16.to_le_bytes());
    reseal(&mut bytes);

    let err =
        Module::validate_and_load(&bytes).expect_err("forged zero neg field must be rejected");
    assert!(
        matches!(err, ModuleError::MalformedInstructionStream),
        "expected MalformedInstructionStream, got {err:?}"
    );
}

#[test]
fn forged_zero_node_symbol_is_rejected() {
    let mut bytes = emit_bytes(r#"Top = (identifier) @id"#);
    let off = Module::validate_and_load(&bytes)
        .expect("module validates before tampering")
        .offsets()
        .node_kinds as usize;

    bytes[off..off + 2].copy_from_slice(&0u16.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::validate_and_load(&bytes).expect_err("forged node symbol must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidNodeSymbol(0)),
        "expected InvalidNodeSymbol(0), got {err:?}"
    );
}

#[test]
fn forged_reserved_node_kind_symbols_are_rejected() {
    for symbol in [u16::MAX - 1, u16::MAX] {
        let mut bytes = emit_bytes(r#"Top = (identifier) @id"#);
        let off = Module::validate_and_load(&bytes)
            .expect("module validates before tampering")
            .offsets()
            .node_kinds as usize;

        bytes[off..off + 2].copy_from_slice(&symbol.to_le_bytes());
        reseal(&mut bytes);

        let err =
            Module::validate_and_load(&bytes).expect_err("reserved node symbol must be rejected");
        assert!(
            matches!(err, ModuleError::InvalidNodeSymbol(0)),
            "expected InvalidNodeSymbol(0) for {symbol:#06x}, got {err:?}"
        );
    }
}

#[test]
fn forged_noncanonical_node_kind_operands_are_rejected() {
    for (query, operand) in [(r#"Q = _"#, 1), (r#"Q = (ERROR)"#, u16::MAX - 1)] {
        let mut bytes = emit_bytes(query);
        Module::validate_and_load(&bytes).expect("compiler output is canonical");
        let off = first_instr(&bytes, |opcode| opcode <= 5);

        bytes[off + 2..off + 4].copy_from_slice(&operand.to_le_bytes());
        reseal(&mut bytes);

        let err = Module::validate_and_load(&bytes)
            .expect_err("forged node-kind operand must be rejected");
        assert!(
            matches!(err, ModuleError::MalformedInstructionStream),
            "expected MalformedInstructionStream for {operand:#06x}, got {err:?}"
        );
    }
}
