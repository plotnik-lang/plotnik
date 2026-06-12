#[cfg(feature = "lang-javascript")]
mod javascript {
    use plotnik_lib::bytecode::Module;
    use plotnik_lib::{QueryBuilder, RuntimeError, SourceMap, VM};

    use crate::language_registry;

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
    fn skip_trivia_accepts_comment_and_anonymous_token() {
        let query = r#"Q = (program (expression_statement (binary_expression (identifier) @left . (identifier) @right)))"#;

        assert!(query_matches(query, "a + /* c */ b"));
        assert!(query_matches(query, "a + b"));
    }

    #[test]
    fn skip_trivia_rejects_named_node_between_matches() {
        let query = r#"Q = (program (expression_statement (binary_expression (identifier) @left . (identifier) @right)))"#;

        assert!(!query_matches(query, "a + f() + b"));
    }

    #[test]
    fn skip_extras_accepts_extra_rejects_anonymous_token() {
        let query = r#"Q = (program (expression_statement (array "," . (number) @n)))"#;

        assert!(query_matches(query, "[1, /* c */ 2]"));
        assert!(!query_matches(query, "[1,,2]"));
    }

    #[test]
    fn skip_exact_requires_true_adjacency() {
        let query = r#"Q = (program (expression_statement (array "," .! (number) @n)))"#;

        assert!(query_matches(query, "[1, 2]"));
        assert!(!query_matches(query, "[1, /* c */ 2]"));
    }

    #[test]
    fn up_skip_trivia_accepts_trailing_comment() {
        let query = r#"Q = (program (expression_statement (identifier == "a")) .)"#;

        assert!(query_matches(query, "a; /* c */"));
    }

    #[test]
    fn up_skip_trivia_rejects_trailing_named_node() {
        let query = r#"Q = (program (expression_statement (identifier == "a")) .)"#;

        assert!(!query_matches(query, "a; b;"));
    }

    #[test]
    fn up_skip_extras_accepts_anonymous_operand_as_last_child() {
        let query = r#"Q = (program (debugger_statement "debugger" .))"#;

        assert!(query_matches(query, "debugger /* c */"));
    }

    #[test]
    fn up_skip_extras_rejects_trailing_anonymous_token() {
        let query = r#"Q = (program (debugger_statement "debugger" .))"#;

        assert!(!query_matches(query, "debugger;"));
    }

    #[test]
    fn up_exact_accepts_literal_last_child() {
        let query = r#"Q = (program (expression_statement (identifier == "a") .!))"#;

        assert!(query_matches(query, "a"));
    }

    #[test]
    fn up_exact_rejects_trailing_comment() {
        let query = r#"Q = (program (expression_statement (identifier == "a") .!))"#;

        assert!(!query_matches(query, "a /* c */;"));
    }

    #[test]
    fn explicit_comment_pattern_matches_before_skip_policy_runs() {
        let query = r#"Q = (program {(comment) @doc . (function_declaration) @fn})"#;

        assert!(query_matches(query, "// doc\nfunction f() {}"));
    }
}
