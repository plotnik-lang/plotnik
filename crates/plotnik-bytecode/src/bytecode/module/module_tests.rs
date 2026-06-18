use super::{ByteStorage, Module, ModuleError};
use crate::bytecode::Header;
use crate::bytecode::{AlignedVec, STEP_SIZE};

/// Build a minimal-but-valid module whose only populated section is Transitions,
/// carrying `transitions_count` steps drawn from `transitions`.
///
/// Everything else (strings, regex, types, entrypoints) is empty, which the
/// other `validate_*` passes accept: the empty string/regex tables collapse to a
/// single zero sentinel that already lives in the zero-filled body. The checksum
/// is recomputed the same way `Module::validate` checks it (CRC32 over the
/// post-header bytes), so the only thing under test is `validate_transitions`.
fn module_with_transitions(transitions: &[u8], transitions_count: u16) -> Vec<u8> {
    let mut header = Header {
        transitions_count,
        ..Default::default()
    };

    let offsets = header.compute_offsets();
    let base = offsets.transitions as usize;
    let total = base + transitions_count as usize * STEP_SIZE;

    let mut bytes = vec![0u8; total];
    bytes[base..base + transitions.len()].copy_from_slice(transitions);

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
    let storage = ByteStorage::copy_from_slice(&data);

    assert_eq!(&*storage, &data[..]);
    assert_eq!(storage.len(), 5);
    assert_eq!(storage[2], 3);
}

#[test]
fn byte_storage_from_aligned() {
    let vec = AlignedVec::copy_from_slice(&[1, 2, 3, 4, 5]);
    let storage = ByteStorage::from_aligned(vec);

    assert_eq!(&*storage, &[1, 2, 3, 4, 5]);
    assert_eq!(storage.len(), 5);
}

#[test]
fn module_error_display() {
    let err = ModuleError::InvalidMagic;
    assert_eq!(err.to_string(), "invalid magic: expected PTKQ");

    let err = ModuleError::UnsupportedVersion(99);
    assert!(err.to_string().contains("99"));

    let err = ModuleError::FileTooSmall(32);
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
    let mut step = [0u8; STEP_SIZE];
    step[0] = match_header(0x0);
    let bytes = module_with_transitions(&step, 1);

    let module = Module::load(&bytes).expect("valid module should load");

    assert_eq!(module.header().transitions_count, 1);
}

#[test]
fn load_rejects_invalid_opcode_at_reachable_step() {
    // Step 0 carries an unknown opcode nibble (0xF); the linear walk lands on it
    // immediately and rejects.
    let mut step = [0u8; STEP_SIZE];
    step[0] = 0xF;
    let bytes = module_with_transitions(&step, 1);

    let err = Module::load(&bytes).expect_err("unknown opcode must be rejected");

    assert!(matches!(
        err,
        ModuleError::InvalidOpcode {
            step: 0,
            opcode: 0xF
        }
    ));
}

#[test]
fn load_walks_past_extended_match_payload() {
    // A Match16 (opcode 0x1) occupies two steps. Its interior payload half
    // (step 1) is poisoned with an invalid opcode nibble: a correct
    // `step += step_count` walk advances from step 0 straight to step 2 and
    // never inspects it, so the module still loads. A buggy walk that advanced
    // one step at a time would land on the poison and false-reject.
    let mut transitions = [0u8; STEP_SIZE * 2];
    transitions[0] = match_header(0x1);
    transitions[STEP_SIZE] = 0xF; // interior payload, not an instruction boundary
    let bytes = module_with_transitions(&transitions, 2);

    let module = Module::load(&bytes).expect("extended match payload must not false-reject");

    assert_eq!(module.header().transitions_count, 2);
}
