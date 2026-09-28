use super::*;
use crate::bytecode::EffectKind;

#[test]
fn predicate_choices_and_missing_have_independent_encodings() {
    for (op, plain, missing) in [
        (PredicateOp::Eq, 0x01, 0x09),
        (PredicateOp::Ne, 0x02, 0x0a),
        (PredicateOp::StartsWith, 0x03, 0x0b),
        (PredicateOp::EndsWith, 0x04, 0x0c),
        (PredicateOp::Contains, 0x05, 0x0d),
        (PredicateOp::RegexMatch, 0x06, 0x0e),
        (PredicateOp::RegexNoMatch, 0x07, 0x0f),
    ] {
        for (missing, counts) in [(false, plain), (true, missing)] {
            let instruction = MatchInstr {
                nav: Nav::StayExact,
                missing,
                predicate: Some(MatchPredicate {
                    op,
                    value_ref: 0x1234,
                }),
                ..Default::default()
            };
            let expected = [1, 2, 0, 0, 0, 0, counts, 0, 0x34, 0x12, 0, 0, 0, 0, 0, 0];

            let encoded = instruction.encode().expect("predicate fits Match16");
            let decoded = Match::from_bytes(&expected).to_instr();

            assert_eq!(encoded, expected);
            assert_eq!(decoded, instruction);
        }
    }
}

#[test]
fn missing_without_predicate_leaves_successor_in_first_payload_slot() {
    let instruction = MatchInstr {
        nav: Nav::StayExact,
        missing: true,
        successors: vec![SuccessorAddr::try_from(5).expect("nonzero successor")],
        ..Default::default()
    };
    let expected = [1, 2, 0, 0, 0, 0, 0x18, 0, 5, 0, 0, 0, 0, 0, 0, 0];

    let encoded = instruction.encode().expect("missing match fits Match16");
    let decoded = Match::from_bytes(&expected).to_instr();

    assert_eq!(encoded, expected);
    assert_eq!(decoded, instruction);
}

#[test]
fn captured_predicate_and_successor_fit_match16() {
    let instruction = MatchInstr {
        nav: Nav::StayExact,
        effects: vec![
            Effect::new(EffectKind::Node, 0),
            Effect::new(EffectKind::RecordSet, 0),
        ],
        predicate: Some(MatchPredicate {
            op: PredicateOp::Eq,
            value_ref: 0x1234,
        }),
        successors: vec![SuccessorAddr::try_from(5).expect("nonzero successor")],
        ..Default::default()
    };
    let expected = [
        1, 2, 0, 0, 0, 0, 0x11, 0x20, 0, 0, 0, 0x14, 0x34, 0x12, 5, 0,
    ];

    let encoded = instruction
        .encode()
        .expect("four payload slots fit Match16");
    let decoded = Match::from_bytes(&expected).to_instr();

    assert_eq!(encoded, expected);
    assert_eq!(decoded, instruction);
}

#[test]
fn predicate_operand_fits_last_available_match64_slot() {
    let mut instruction = MatchInstr {
        nav: Nav::StayExact,
        effects: vec![Effect::new(EffectKind::Node, 0); 15],
        neg_fields: vec![NodeFieldId::try_from(1).expect("nonzero field"); 7],
        predicate: Some(MatchPredicate {
            op: PredicateOp::RegexNoMatch,
            value_ref: 0x1234,
        }),
        successors: vec![SuccessorAddr::try_from(5).expect("nonzero successor"); 5],
        ..Default::default()
    };

    let encoded = instruction.encode().expect("28 slots fit Match64");
    instruction
        .successors
        .push(SuccessorAddr::try_from(6).expect("nonzero successor"));
    let overflow = instruction.encode();

    assert_eq!(encoded.len(), 64);
    assert_eq!(&encoded[..8], &[5, 2, 0, 0, 0, 0, 0x57, 0xfe]);
    assert_eq!(overflow, Err(EncodeError::PayloadTooLarge(29)));
}
