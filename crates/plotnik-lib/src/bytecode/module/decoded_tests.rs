use indoc::indoc;

use crate::bytecode::PredicateOp;
use crate::compiler::test_utils::javascript_grammar as javascript;
use crate::compiler::{QueryBuilder, SourceMap, SourcePath};

use super::{DecodedInstr, Instruction, Module};

fn compile_module(query_src: &str) -> Module {
    let mut source_map = SourceMap::new();
    source_map.add_file(SourcePath::new("query.ptk"), query_src);
    let compiled = QueryBuilder::new(source_map)
        .compile(javascript())
        .expect("query parsing should not exhaust fuel");
    assert!(compiled.is_valid(), "query should compile: {query_src}");
    Module::load(compiled.bytecode().expect("compiled query has bytecode"))
        .expect("compiled bytecode should load")
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

    let mut step = 0u16;
    while step < module.header().transitions_count {
        match (module.decode_step(step), module.decoded().step(step)) {
            (Instruction::Match(m), DecodedInstr::Match(decoded)) => {
                assert_eq!(decoded.nav, m.nav);
                assert_eq!(decoded.node_kind, m.node_kind);
                assert_eq!(decoded.node_field, m.node_field);

                let effects = m.effects().collect::<Vec<_>>();
                assert_eq!(module.decoded().effects(&decoded), effects.as_slice());

                let neg_fields = m.neg_fields().collect::<Vec<_>>();
                assert_eq!(module.decoded().neg_fields(&decoded), neg_fields.as_slice());

                let successors = m.successors().map(u16::from).collect::<Vec<_>>();
                assert_eq!(module.decoded().successors(&decoded), successors.as_slice());

                let predicate = m
                    .predicate()
                    .map(|p| (PredicateOp::from_byte(p.op), p.is_regex, p.value_ref));
                let decoded_predicate = decoded.predicate.map(|p| (p.op, p.is_regex, p.value_ref));
                assert_eq!(decoded_predicate, predicate);

                saw_effects |= !effects.is_empty();
                saw_extended_match |= m.step_count() > 1;
                saw_multiple_successors |= successors.len() > 1;
                saw_neg_fields |= !neg_fields.is_empty();
                saw_predicate |= predicate.is_some();

                for interior in step + 1..step + m.step_count() {
                    assert!(
                        matches!(module.decoded().step(interior), DecodedInstr::Return),
                        "interior step {interior} should be a placeholder"
                    );
                }

                step += m.step_count();
            }
            (Instruction::Call(c), DecodedInstr::Call(decoded)) => {
                assert_eq!(decoded.nav, c.nav);
                assert_eq!(decoded.node_field, c.node_field);
                assert_eq!(decoded.next, u16::from(c.next));
                assert_eq!(decoded.target, u16::from(c.target));
                step += 1;
            }
            (Instruction::Return(_), DecodedInstr::Return) => {
                step += 1;
            }
            (byte, decoded) => {
                panic!("decoded instruction mismatch at step {step}: {byte:?} vs {decoded:?}");
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
