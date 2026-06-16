//! Loader hardening: forged-module rejection.
//!
//! Each test emits a real module through the compiler, deliberately corrupts one
//! field, and recomputes the CRC so the checksum gate still passes — then asserts
//! [`Module::load`] rejects it with a clean [`ModuleError`] rather than letting a
//! later view/decode, VM, or materializer access panic. Together they guard the
//! load-time structural validators (`validate_string_ids`, `validate_transitions`,
//! `validate_entrypoints`, `load_regex_dfas`, `validate_effect_stack`) that
//! uphold the format's "a loaded module never panics on later access" guarantee.
//!
//! These live in-crate rather than under `tests/`: forging exact bytes needs the
//! `pub(crate)` section offsets from [`Module::offsets`], which an external
//! integration test cannot reach. Minting a real module needs the compiler, which
//! depends on this crate — a cycle that is fine through a `[dev-dependencies]`
//! edge, since it never enters the build graph. `build.rs` exposes the JavaScript
//! `grammar.json` the fixtures link against.

use std::sync::LazyLock;

use plotnik_compiler::{QueryBuilder, SourceMap};
use plotnik_core::grammar::{Grammar, raw::RawGrammar};

use super::{Module, ModuleError};
use crate::bytecode::type_meta::TypeData;

fn javascript() -> &'static Grammar {
    static GRAMMAR: LazyLock<Grammar> = LazyLock::new(|| {
        let raw = RawGrammar::from_json(include_str!(env!(
            "PLOTNIK_BYTECODE_JAVASCRIPT_GRAMMAR_JSON"
        )))
        .expect("javascript grammar fixture");
        Grammar::from_raw(&raw).expect("javascript grammar metadata")
    });
    &GRAMMAR
}

/// Emit bytecode for a query that must link.
fn emit_bytes(query_src: &str) -> Vec<u8> {
    let mut source_map = SourceMap::new();
    source_map.add_file("query.ptk", query_src);
    let query = QueryBuilder::new(source_map)
        .parse()
        .expect("query parsing should not exhaust fuel")
        .analyze()
        .link(javascript());
    assert!(query.is_valid(), "query should link: {query_src}");
    query.emit().expect("bytecode emission should succeed")
}

/// Recompute the CRC32 the loader checks (over everything after the 64-byte
/// header) so a tampered body is accepted by the checksum gate and reaches the
/// structural validators we are exercising.
fn reseal(bytes: &mut [u8]) {
    let crc = crc32fast::hash(&bytes[64..]);
    bytes[8..12].copy_from_slice(&crc.to_le_bytes());
}

/// Byte offset of the first predicated Match's 4-byte predicate
/// (`op_and_flags` u16 || `value_ref` u16) in the transitions stream.
fn find_predicate_off(bytes: &[u8]) -> usize {
    let (base, steps) = {
        let m = Module::load(bytes).expect("module loads before tampering");
        (
            m.offsets().transitions as usize,
            m.header().transitions_count,
        )
    };
    let mut step = 0u16;
    while step < steps {
        let instr = base + step as usize * 8;
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
            if (counts >> 1) & 1 != 0 {
                let pre = ((counts >> 13) & 0x7) as usize;
                let neg = ((counts >> 10) & 0x7) as usize;
                let post = ((counts >> 7) & 0x7) as usize;
                return instr + 8 + (pre + neg + post) * 2;
            }
        }
        step += (size / 8) as u16;
    }
    panic!("query must emit a string predicate");
}

#[test]
fn forged_invalid_entrypoint_name_is_rejected() {
    // `0` is the reserved easter-egg index (never a real reference) and `u16::MAX`
    // is past the table; both must yield a clean error, not panic in StringId::new.
    for forged in [0u16, u16::MAX] {
        let mut bytes = emit_bytes(r#"Top = (identifier) @id"#);
        let ep_off = Module::load(&bytes)
            .expect("module loads before tampering")
            .offsets()
            .entrypoints as usize;

        bytes[ep_off..ep_off + 2].copy_from_slice(&forged.to_le_bytes());
        reseal(&mut bytes);

        let err = Module::load(&bytes).expect_err("forged entrypoint name must be rejected");
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
    let str_count = Module::load(&bytes)
        .expect("module loads before tampering")
        .header()
        .str_table_count;

    let pred_off = find_predicate_off(&bytes);
    bytes[pred_off + 2..pred_off + 4].copy_from_slice(&str_count.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::load(&bytes).expect_err("forged predicate operand must be rejected");
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

    let err = Module::load(&bytes).expect_err("forged predicate op must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidPredicateOperand(_)),
        "expected InvalidPredicateOperand, got {err:?}"
    );
}

/// A struct query that emits extended Matches carrying `Node`/`Set` effects and
/// successors — the shapes the per-instruction forging tests below target.
const STRUCT_QUERY: &str =
    r#"Top = (binary_expression left: (identifier) @l right: (identifier) @r)"#;

/// `(base, steps)` of a freshly-emitted (still valid) module's transitions.
fn transitions(bytes: &[u8]) -> (usize, u16) {
    let m = Module::load(bytes).expect("module loads before tampering");
    (
        m.offsets().transitions as usize,
        m.header().transitions_count,
    )
}

/// Byte size of an instruction from its opcode nibble (mirrors `Opcode::size`).
fn instr_size(opcode: u8) -> usize {
    match opcode {
        0 | 6 | 7 | 8 => 8,
        1 => 16,
        2 => 24,
        3 => 32,
        4 => 48,
        5 => 64,
        other => panic!("unexpected opcode {other}"),
    }
}

/// Byte offset of the first instruction whose opcode nibble satisfies `want`.
fn first_instr(bytes: &[u8], want: impl Fn(u8) -> bool) -> usize {
    let (base, steps) = transitions(bytes);
    let mut step = 0u16;
    while step < steps {
        let off = base + step as usize * 8;
        let opcode = bytes[off] & 0x0F;
        if want(opcode) {
            return off;
        }
        step += (instr_size(opcode) / 8) as u16;
    }
    panic!("no matching instruction in transitions");
}

/// Byte offsets of every pre/post effect slot in the stream (the negated-field
/// slots are skipped: those are plain field ids, not decoded effects).
fn effect_slots(bytes: &[u8]) -> Vec<usize> {
    let (base, steps) = transitions(bytes);
    let mut slots = Vec::new();
    let mut step = 0u16;
    while step < steps {
        let off = base + step as usize * 8;
        let opcode = bytes[off] & 0x0F;
        if (1..=5).contains(&opcode) {
            let counts = u16::from_le_bytes([bytes[off + 6], bytes[off + 7]]);
            let pre = ((counts >> 13) & 0x7) as usize;
            let neg = ((counts >> 10) & 0x7) as usize;
            let post = ((counts >> 7) & 0x7) as usize;
            slots.extend((0..pre).map(|i| off + 8 + i * 2));
            slots.extend((0..post).map(|i| off + 8 + (pre + neg + i) * 2));
        }
        step += (instr_size(opcode) / 8) as u16;
    }
    slots
}

/// Step index of an interior (non-start) step of the first multi-step
/// instruction — a byte region a multi-step opcode spans beyond its header step.
fn first_multistep_interior_step(bytes: &[u8]) -> u16 {
    let (base, steps) = transitions(bytes);
    let mut step = 0u16;
    while step < steps {
        let off = base + step as usize * 8;
        let span = (instr_size(bytes[off] & 0x0F) / 8) as u16;
        if span > 1 {
            return step + 1;
        }
        step += span;
    }
    panic!("no multi-step instruction in transitions");
}

/// Byte offset of the first extended-match successor slot in the stream.
fn first_ext_successor(bytes: &[u8]) -> usize {
    let (base, steps) = transitions(bytes);
    let mut step = 0u16;
    while step < steps {
        let off = base + step as usize * 8;
        let opcode = bytes[off] & 0x0F;
        if (1..=5).contains(&opcode) {
            let counts = u16::from_le_bytes([bytes[off + 6], bytes[off + 7]]);
            let pre = ((counts >> 13) & 0x7) as usize;
            let neg = ((counts >> 10) & 0x7) as usize;
            let post = ((counts >> 7) & 0x7) as usize;
            let succ = ((counts >> 2) & 0x1F) as usize;
            let has_pred = (counts >> 1) & 1 != 0;
            if succ > 0 {
                return off + 8 + (pre + neg + post) * 2 + if has_pred { 4 } else { 0 };
            }
        }
        step += (instr_size(opcode) / 8) as u16;
    }
    panic!("no extended-match successor in transitions");
}

/// Byte offset of the first pre/post effect slot whose opcode (`raw >> 10`)
/// satisfies `want`.
fn first_effect_op(bytes: &[u8], want: impl Fn(u16) -> bool) -> usize {
    effect_slots(bytes)
        .into_iter()
        .find(|&off| want(u16::from_le_bytes([bytes[off], bytes[off + 1]]) >> 10))
        .expect("no matching effect slot in transitions")
}

#[test]
fn forged_unknown_opcode_is_rejected() {
    // `9` is past the 0x0..=0x8 opcode range; the VM's `decode_step` would
    // `.expect()` on the `None` from `Opcode::from_u8` for this step.
    let mut bytes = emit_bytes(STRUCT_QUERY);
    let off = first_instr(&bytes, |_| true);
    bytes[off] = (bytes[off] & 0xF0) | 0x09;
    reseal(&mut bytes);

    let err = Module::load(&bytes).expect_err("forged opcode must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidOpcode { opcode: 0x09, .. }),
        "expected InvalidOpcode, got {err:?}"
    );
}

#[test]
fn forged_nonzero_segment_is_rejected() {
    // Segment bits (header bits 6-7) are reserved at zero; the call/return/
    // trampoline decoders `assert!` on a non-zero segment.
    let mut bytes = emit_bytes(STRUCT_QUERY);
    let off = first_instr(&bytes, |_| true);
    bytes[off] |= 0x40;
    reseal(&mut bytes);

    let err = Module::load(&bytes).expect_err("forged segment must be rejected");
    assert!(
        matches!(err, ModuleError::MalformedTransitions),
        "expected MalformedTransitions, got {err:?}"
    );
}

#[test]
fn forged_reserved_node_kind_is_rejected() {
    // node_kind `0b11` (header bits 4-5) is reserved; `NodeTypeIR::from_bytes`
    // would panic on it.
    let mut bytes = emit_bytes(STRUCT_QUERY);
    let off = first_instr(&bytes, |o| o <= 5);
    bytes[off] |= 0x30;
    reseal(&mut bytes);

    let err = Module::load(&bytes).expect_err("forged node_kind must be rejected");
    assert!(
        matches!(err, ModuleError::MalformedTransitions),
        "expected MalformedTransitions, got {err:?}"
    );
}

#[test]
fn forged_invalid_nav_is_rejected() {
    // `0x80` is an Up-family byte (bit 7 set) with a zero level; `Nav::from_byte`
    // would panic, so the loader must reject it.
    let mut bytes = emit_bytes(STRUCT_QUERY);
    let off = first_instr(&bytes, |o| o <= 5);
    bytes[off + 1] = 0x80;
    reseal(&mut bytes);

    let err = Module::load(&bytes).expect_err("forged nav must be rejected");
    assert!(
        matches!(err, ModuleError::MalformedTransitions),
        "expected MalformedTransitions, got {err:?}"
    );
}

#[test]
fn forged_invalid_effect_opcode_is_rejected() {
    // `14` is past the 0..=13 effect range; `EffectOpcode::from_u8` would panic
    // when the VM emits this effect.
    let mut bytes = emit_bytes(STRUCT_QUERY);
    let slot = effect_slots(&bytes)[0];
    let existing = u16::from_le_bytes([bytes[slot], bytes[slot + 1]]);
    let forged = (14u16 << 10) | (existing & 0x3FF);
    bytes[slot..slot + 2].copy_from_slice(&forged.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::load(&bytes).expect_err("forged effect opcode must be rejected");
    assert!(
        matches!(err, ModuleError::MalformedTransitions),
        "expected MalformedTransitions, got {err:?}"
    );
}

#[test]
fn forged_oob_member_operand_is_rejected() {
    // A `Set`/`Enum` payload indexes the type-member table via the materializer's
    // `get_member`, which asserts the index is in bounds.
    let mut bytes = emit_bytes(STRUCT_QUERY);
    let members = Module::load(&bytes)
        .expect("module loads before tampering")
        .header()
        .type_members_count;
    let slot = effect_slots(&bytes)
        .into_iter()
        .find(|&off| {
            let e = u16::from_le_bytes([bytes[off], bytes[off + 1]]);
            matches!(e >> 10, 6 | 7) // Set | Enum
        })
        .expect("struct query must emit a Set/Enum effect");
    let opcode_bits = u16::from_le_bytes([bytes[slot], bytes[slot + 1]]) & 0xFC00;
    let forged = opcode_bits | (members & 0x3FF);
    bytes[slot..slot + 2].copy_from_slice(&forged.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::load(&bytes).expect_err("forged member operand must be rejected");
    assert!(
        matches!(err, ModuleError::MalformedTransitions),
        "expected MalformedTransitions, got {err:?}"
    );
}

#[test]
fn forged_zero_successor_is_rejected() {
    // `0` decodes through `StepId::new`, which panics; `0` is the terminal marker
    // only for the `Match8` fast path, never an extended successor slot.
    let mut bytes = emit_bytes(STRUCT_QUERY);
    let succ_off = first_ext_successor(&bytes);
    bytes[succ_off..succ_off + 2].copy_from_slice(&0u16.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::load(&bytes).expect_err("forged zero successor must be rejected");
    assert!(
        matches!(err, ModuleError::MalformedTransitions),
        "expected MalformedTransitions, got {err:?}"
    );
}

#[test]
fn forged_out_of_range_successor_is_rejected() {
    // A successor past the step count would slice past the buffer in `decode_step`.
    let mut bytes = emit_bytes(STRUCT_QUERY);
    let succ_off = first_ext_successor(&bytes);
    bytes[succ_off..succ_off + 2].copy_from_slice(&u16::MAX.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::load(&bytes).expect_err("forged out-of-range successor must be rejected");
    assert!(
        matches!(err, ModuleError::MalformedTransitions),
        "expected MalformedTransitions, got {err:?}"
    );
}

#[test]
fn forged_regex_pattern_string_id_is_rejected() {
    // A regex entry's `string_id` is display metadata that `dump`/`trace` resolve
    // through the panicking `get_string_id` (StringId::new) and then index the
    // string blob; `0` (reserved) and an out-of-range id must be rejected at load.
    for forged in [0u16, u16::MAX] {
        let mut bytes = emit_bytes(r#"Q = (identifier =~ /x/)"#);
        let (regex_off, regex_count, str_count) = {
            let m = Module::load(&bytes).expect("module loads before tampering");
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

        let err = Module::load(&bytes).expect_err("forged regex string_id must be rejected");
        assert!(
            matches!(err, ModuleError::InvalidStringId(_)),
            "forged regex string_id {value}: expected InvalidStringId, got {err:?}"
        );
    }
}

#[test]
fn forged_corrupt_regex_dfa_is_rejected() {
    // The regex blob holds the serialized sparse DFA the loader deserializes
    // once into the module's cache; a corrupt blob must be rejected at load, not
    // `.expect()`ed at match time.
    let mut bytes = emit_bytes(r#"Q = (identifier =~ /x/)"#);
    let (blob_off, blob_len) = {
        let m = Module::load(&bytes).expect("module loads before tampering");
        (
            m.offsets().regex_blob as usize,
            m.header().regex_blob_size as usize,
        )
    };
    assert!(blob_len > 0, "query must emit a DFA blob");

    bytes[blob_off..blob_off + blob_len].fill(0xFF);
    reseal(&mut bytes);

    let err = Module::load(&bytes).expect_err("forged regex DFA must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidRegexDfa(_)),
        "expected InvalidRegexDfa, got {err:?}"
    );
}

#[test]
fn forged_entrypoint_into_instruction_interior_is_rejected() {
    // Issue #457: an entrypoint `target` that lands inside a multi-step
    // instruction (not on a recorded instruction start) makes the VM begin
    // decoding mid-instruction. `target < steps` is not enough — the load-time
    // check holds entrypoints to the same instruction-start rule as successors.
    let mut bytes = emit_bytes(STRUCT_QUERY);
    let (ep_off, interior) = {
        let m = Module::load(&bytes).expect("module loads before tampering");
        (
            m.offsets().entrypoints as usize,
            first_multistep_interior_step(&bytes),
        )
    };

    // `target` is the second u16 of the 8-byte entrypoint, after the name.
    bytes[ep_off + 2..ep_off + 4].copy_from_slice(&interior.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::load(&bytes).expect_err("forged interior entrypoint must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidEntrypoint(_)),
        "expected InvalidEntrypoint, got {err:?}"
    );
}

#[test]
fn forged_effect_set_to_push_is_rejected() {
    // Swap an executed `Set` (opcode 6) for `Push` (opcode 2). A loaded module
    // would accept it, then the materializer would panic because the builder on
    // top is an Object, not an Array. The effect-stack verifier rejects it at
    // load instead.
    let mut bytes = emit_bytes(STRUCT_QUERY);
    let slot = first_effect_op(&bytes, |op| op == 6);
    bytes[slot..slot + 2].copy_from_slice(&(2u16 << 10).to_le_bytes());
    reseal(&mut bytes);

    let err = Module::load(&bytes).expect_err("forged Set->Push must be rejected");
    assert!(
        matches!(err, ModuleError::EffectStackImbalance(_)),
        "expected EffectStackImbalance, got {err:?}"
    );
}

#[test]
fn forged_scalar_capture_set_to_push_is_rejected() {
    // The minimal case: a scalar struct whose only effect is a `Set` into the
    // preamble's root object. Forged to `Push`, the body now demands an Array
    // top while the preamble hands it an Object — caught when the entrypoint
    // summary is checked against the preamble.
    let mut bytes = emit_bytes(r#"Q = (identifier) @id"#);
    let slot = first_effect_op(&bytes, |op| op == 6);
    bytes[slot..slot + 2].copy_from_slice(&(2u16 << 10).to_le_bytes());
    reseal(&mut bytes);

    let err = Module::load(&bytes).expect_err("forged scalar Set->Push must be rejected");
    assert!(
        matches!(err, ModuleError::EffectStackImbalance(_)),
        "expected EffectStackImbalance, got {err:?}"
    );
}

#[test]
fn forged_dropped_scope_close_is_rejected() {
    // Turn an `EndObj` (opcode 5) into a no-op `Node` (opcode 0): the struct's
    // `Obj` is never closed, so the body returns with an open frame — the
    // materializer would leave the builder stack unbalanced. Rejected as a
    // non-neutral body.
    let mut bytes = emit_bytes(STRUCT_QUERY);
    let slot = first_effect_op(&bytes, |op| op == 5);
    bytes[slot..slot + 2].copy_from_slice(&0u16.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::load(&bytes).expect_err("forged dropped EndObj must be rejected");
    assert!(
        matches!(err, ModuleError::EffectStackImbalance(_)),
        "expected EffectStackImbalance, got {err:?}"
    );
}

#[test]
fn forged_suppress_underflow_is_rejected() {
    // Replace a data effect with a bare `SuppressEnd` (opcode 13). With no
    // matching `SuppressBegin` on the path, the VM's suppression counter would
    // underflow and `.expect()` panic; the verifier rejects it at load.
    let mut bytes = emit_bytes(STRUCT_QUERY);
    let slot = first_effect_op(&bytes, |op| op == 4 || op == 6);
    bytes[slot..slot + 2].copy_from_slice(&(13u16 << 10).to_le_bytes());
    reseal(&mut bytes);

    let err = Module::load(&bytes).expect_err("forged SuppressEnd underflow must be rejected");
    assert!(
        matches!(err, ModuleError::EffectStackImbalance(_)),
        "expected EffectStackImbalance, got {err:?}"
    );
}

#[test]
fn forged_preamble_without_root_object_is_rejected() {
    // The shared preamble opens a root `Obj` before trampolining into the entry
    // body, so the body always has an Object to `Set` into. Neutralize that `Obj`
    // and its matching `EndObj` (turn both into no-op `Clear`s) and lie that the
    // result type is scalar: the entry's `Set` would then hit the materializer's
    // scalar root frame and panic. The preamble has no caller, so a requirement
    // bubbling out of it must be rejected, not silently dropped.
    let mut bytes = emit_bytes(r#"Q = (_) @x"#);
    let ep_off = Module::load(&bytes)
        .expect("module loads before tampering")
        .offsets()
        .entrypoints as usize;
    let obj_slot = first_effect_op(&bytes, |op| op == 4); // preamble Obj
    let endobj_slot = first_effect_op(&bytes, |op| op == 5); // preamble EndObj

    bytes[obj_slot..obj_slot + 2].copy_from_slice(&(10u16 << 10).to_le_bytes());
    bytes[endobj_slot..endobj_slot + 2].copy_from_slice(&(10u16 << 10).to_le_bytes());
    // Result type T1 (struct) -> T0 (scalar <Node>): the root frame is now a Scalar.
    bytes[ep_off + 4..ep_off + 6].copy_from_slice(&0u16.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::load(&bytes).expect_err("forged rootless preamble must be rejected");
    assert!(
        matches!(err, ModuleError::EffectStackImbalance(_)),
        "expected EffectStackImbalance, got {err:?}"
    );
}

#[test]
fn forged_out_of_range_trampoline_target_is_rejected() {
    // The trampoline's `next` is a jump target too; an out-of-range value must be
    // caught by the pass-2 instruction-start check, not decoded.
    let mut bytes = emit_bytes(STRUCT_QUERY);
    let off = first_instr(&bytes, |o| o == 8); // Trampoline
    bytes[off + 2..off + 4].copy_from_slice(&u16::MAX.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::load(&bytes).expect_err("forged trampoline target must be rejected");
    assert!(
        matches!(err, ModuleError::MalformedTransitions),
        "expected MalformedTransitions, got {err:?}"
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

    let err = Module::load(&bytes).expect_err("forged regex sentinel operand must be rejected");
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

    let err = Module::load(&bytes).expect_err("forged regex-flag mismatch must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidPredicateOperand(_)),
        "expected InvalidPredicateOperand, got {err:?}"
    );
}

#[test]
fn forged_unknown_type_def_kind_is_rejected() {
    // A TypeDef's kind byte (byte 3 of the 4-byte entry) must be a known TypeKind;
    // an unknown kind would panic the materializer's `get_def`/`TypeData` decode.
    let mut bytes = emit_bytes(STRUCT_QUERY);
    let type_defs_off = Module::load(&bytes)
        .expect("module loads before tampering")
        .offsets()
        .type_defs as usize;
    bytes[type_defs_off + 3] = 0xFF;
    reseal(&mut bytes);

    let err = Module::load(&bytes).expect_err("forged type-def kind must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidTypeDef(_)),
        "expected InvalidTypeDef, got {err:?}"
    );
}

#[test]
fn forged_out_of_range_entrypoint_target_is_rejected() {
    // The plain out-of-range case (vs the interior-target case above): `target >=
    // steps` must be rejected before `is_start` is indexed.
    let mut bytes = emit_bytes(STRUCT_QUERY);
    let ep_off = Module::load(&bytes)
        .expect("module loads before tampering")
        .offsets()
        .entrypoints as usize;
    bytes[ep_off + 2..ep_off + 4].copy_from_slice(&u16::MAX.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::load(&bytes).expect_err("forged out-of-range target must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidEntrypoint(_)),
        "expected InvalidEntrypoint, got {err:?}"
    );
}

#[test]
fn forged_out_of_range_entrypoint_result_type_is_rejected() {
    // `result_type` (u16 at entry+4) must address a real TypeDef, or the
    // materializer's root-frame TypeId lookup reads out of bounds. `type_defs_count`
    // is one past the last valid index.
    let mut bytes = emit_bytes(STRUCT_QUERY);
    let (ep_off, type_defs) = {
        let m = Module::load(&bytes).expect("module loads before tampering");
        (m.offsets().entrypoints as usize, m.header().type_defs_count)
    };
    bytes[ep_off + 4..ep_off + 6].copy_from_slice(&type_defs.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::load(&bytes).expect_err("forged result_type must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidEntrypoint(_)),
        "expected InvalidEntrypoint, got {err:?}"
    );
}

#[test]
fn forged_type_member_name_string_id_is_rejected() {
    // `validate_string_ids` runs one closure over six sections with distinct
    // (base, stride, name_off) tuples; this locks the type-member arithmetic
    // (stride 4, name at offset 0) the materializer's struct-field keys rely on.
    let mut bytes = emit_bytes(STRUCT_QUERY);
    let members_off = Module::load(&bytes)
        .expect("module loads before tampering")
        .offsets()
        .type_members as usize;
    bytes[members_off..members_off + 2].copy_from_slice(&0u16.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::load(&bytes).expect_err("forged type-member name must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidStringId(_)),
        "expected InvalidStringId, got {err:?}"
    );
}

#[test]
fn forged_oob_member_type_id_is_rejected() {
    // A TypeMember's `type_id` (bytes 2-3 of the 4-byte entry) must address a real
    // TypeDef, or the materializer resolves a struct field to a type out of range.
    let mut bytes = emit_bytes(STRUCT_QUERY);
    let (members_off, type_defs) = {
        let m = Module::load(&bytes).expect("module loads before tampering");
        (
            m.offsets().type_members as usize,
            m.header().type_defs_count,
        )
    };

    // `type_defs_count` is one past the last valid TypeId.
    bytes[members_off + 2..members_off + 4].copy_from_slice(&type_defs.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::load(&bytes).expect_err("forged member type id must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidTypeDef(_)),
        "expected InvalidTypeDef, got {err:?}"
    );
}

#[test]
fn forged_oob_wrapper_inner_type_id_is_rejected() {
    // A wrapper/alias TypeDef holds its inner TypeId in `data` (bytes 0-1 of the
    // 4-byte entry); it must address a real def or `unwrap_optional` / the array
    // element lookup resolves a type out of range.
    let mut bytes = emit_bytes(r#"Top = (program (statement)* @stmts)"#);
    let (defs_off, type_defs, wrapper_idx) = {
        let m = Module::load(&bytes).expect("module loads before tampering");
        let types = m.types();
        let idx = (0..types.defs_count())
            .find(|&i| matches!(types.get_def(i).classify(), TypeData::Wrapper { .. }))
            .expect("array query must emit a wrapper type def");
        (
            m.offsets().type_defs as usize,
            m.header().type_defs_count,
            idx,
        )
    };

    let off = defs_off + wrapper_idx * 4;
    bytes[off..off + 2].copy_from_slice(&type_defs.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::load(&bytes).expect_err("forged wrapper inner type id must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidTypeDef(_)),
        "expected InvalidTypeDef, got {err:?}"
    );
}

#[test]
fn forged_oob_type_name_type_id_is_rejected() {
    // A TypeName's target `type_id` (bytes 2-3 of the 4-byte entry) must address a
    // real TypeDef; a named definition emits at least one entry.
    let mut bytes = emit_bytes(STRUCT_QUERY);
    let (names_off, type_defs) = {
        let m = Module::load(&bytes).expect("module loads before tampering");
        assert!(
            m.types().names_count() > 0,
            "named def must emit a type name"
        );
        (m.offsets().type_names as usize, m.header().type_defs_count)
    };

    bytes[names_off + 2..names_off + 4].copy_from_slice(&type_defs.to_le_bytes());
    reseal(&mut bytes);

    let err = Module::load(&bytes).expect_err("forged type name type id must be rejected");
    assert!(
        matches!(err, ModuleError::InvalidTypeName(_)),
        "expected InvalidTypeName, got {err:?}"
    );
}
