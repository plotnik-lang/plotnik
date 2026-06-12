#[cfg(feature = "lang-javascript")]
mod javascript {
    use plotnik_lib::bytecode::Module;
    use plotnik_lib::{QueryBuilder, RuntimeError, SourceMap, VM};

    use plotnik::language_registry;

    fn query_matches(query_src: &str, source_src: &str) -> bool {
        let lang = language_registry::javascript();
        let source_map = SourceMap::one_liner(query_src);
        let query = QueryBuilder::new(source_map)
            .parse()
            .unwrap()
            .analyze()
            .link(lang.grammar());
        assert!(query.is_valid(), "query should link successfully");

        let bytes = query.emit().expect("bytecode emission should succeed");
        let module = Module::load(&bytes).expect("module loading should succeed");
        let entrypoint = module
            .entrypoints()
            .find_by_name("Q", &module.strings())
            .expect("Q entrypoint should exist");
        let tree = lang.parse(source_src);
        let vm = VM::builder(source_src, &tree).build();

        match vm.execute(&module, 0, &entrypoint) {
            Ok(_) => true,
            Err(RuntimeError::NoMatch) => false,
            Err(err) => panic!("unexpected runtime error: {err}"),
        }
    }

    #[test]
    fn soft_anchor_with_anonymous_operand_skips_extras_only() {
        let query = r#"Q = (program (expression_statement (array "," . (number) @n)))"#;

        assert!(query_matches(query, "[1, /* c */ 2]"));
        assert!(!query_matches(query, "[1,,2]"));
    }
}
