use indoc::indoc;

use crate::bytecode::{CodeAddr, PredicateOp};
use crate::compiler::test_utils::synthetic_grammar as grammar;
use crate::compiler::{BytecodeConfig, QueryBuilder, SourceMap, SourcePath};

use super::{DecodedInstr, Instruction, Module};

fn compile_module(query_src: &str) -> Module {
    let mut source_map = SourceMap::new();
    source_map.add_file(SourcePath::new("query.ptk"), query_src);
    let compiled = QueryBuilder::new(source_map)
        .compile(grammar())
        .expect("query parsing should not exhaust fuel");
    assert!(compiled.is_valid(), "query should compile: {query_src}");
    compiled
        .emit(BytecodeConfig::new())
        .expect("bytecode emission answers")
        .into_artifact()
        .expect("compiled query has bytecode")
}

#[test]
fn decoded_program_matches_byte_decoder() {
    let module = compile_module(indoc! {r#"
        Top = (program [
          Expr: (expression_statement (identifier == "needle") @id)
          Decl: (lexical_declaration
            (variable_declarator -value name: (identifier) @name) @decl)
        ])
    "#});

    let mut saw_effects = false;
    let mut saw_extended_match = false;
    let mut saw_multiple_successors = false;
    let mut saw_neg_fields = false;
    let mut saw_predicate = false;

    let mut addr = CodeAddr::ZERO;
    while addr.get() < module.header().instruction_word_count {
        match (
            module.decode_instruction(addr),
            module.decoded().instruction_at(addr),
        ) {
            (Instruction::Match(m), DecodedInstr::Match(decoded)) => {
                assert_eq!(decoded.nav, m.nav);
                assert_eq!(decoded.node_kind, m.node_kind);
                assert_eq!(decoded.node_field, m.node_field);

                let effects = m.effects().collect::<Vec<_>>();
                assert_eq!(module.decoded().effects(&decoded), effects.as_slice());

                let neg_fields = m.neg_fields().collect::<Vec<_>>();
                assert_eq!(module.decoded().neg_fields(&decoded), neg_fields.as_slice());

                let successors = m.successors().collect::<Vec<_>>();
                assert_eq!(module.decoded().successors(&decoded), successors.as_slice());

                let predicate = m
                    .predicate()
                    .map(|p| (PredicateOp::from_byte(p.op), p.is_regex, p.value_ref));
                let decoded_predicate = decoded.predicate.map(|p| (p.op, p.is_regex, p.value_ref));
                assert_eq!(decoded_predicate, predicate);

                saw_effects |= !effects.is_empty();
                saw_extended_match |= m.word_count() > 1;
                saw_multiple_successors |= successors.len() > 1;
                saw_neg_fields |= !neg_fields.is_empty();
                saw_predicate |= predicate.is_some();

                for interior in addr.get() + 1..addr.get() + m.word_count() {
                    assert!(
                        matches!(
                            module.decoded().instruction_at(CodeAddr::from(interior)),
                            DecodedInstr::Return(_)
                        ),
                        "interior word {interior} should be a placeholder"
                    );
                }

                addr = addr
                    .checked_add(m.word_count())
                    .expect("decoded instruction address fits in u16");
            }
            (Instruction::Call(c), DecodedInstr::Call(decoded)) => {
                assert_eq!(decoded.ownership, c.ownership);
                assert_eq!(decoded.nav, c.nav);
                assert_eq!(decoded.node_field, c.node_field);
                assert_eq!(decoded.target, c.target);
                let returns = c.returns().collect::<Vec<_>>();
                assert_eq!(module.decoded().call_returns(&decoded), returns.as_slice());
                let words = if c.arity() == 1 { 1 } else { 3 };
                for interior in addr.get() + 1..addr.get() + words {
                    assert!(
                        matches!(
                            module.decoded().instruction_at(CodeAddr::from(interior)),
                            DecodedInstr::Return(_)
                        ),
                        "interior word {interior} should be a placeholder"
                    );
                }
                addr = addr
                    .checked_add(words)
                    .expect("instruction address fits in u16");
            }
            (Instruction::Return(return_), DecodedInstr::Return(port)) => {
                assert_eq!(port, return_.port);
                addr = addr
                    .checked_add(1)
                    .expect("instruction address fits in u16");
            }
            (byte, decoded) => {
                panic!("decoded instruction mismatch at address {addr}: {byte:?} vs {decoded:?}");
            }
        }
    }

    assert!(saw_effects, "query should exercise effects");
    assert!(
        saw_extended_match,
        "query should exercise an extended Match"
    );
    assert!(
        saw_multiple_successors,
        "query should exercise multiple successors"
    );
    assert!(saw_neg_fields, "query should exercise negated fields");
    assert!(saw_predicate, "query should exercise a predicate");
}
