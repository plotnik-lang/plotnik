#[cfg(feature = "lang-javascript")]
mod javascript {
    use plotnik_lib::bytecode::Module;
    use plotnik_lib::{
        Colors, Materializer, QueryBuilder, RuntimeError, SourceMap, VM, ValueMaterializer,
    };

    use crate::language_registry;

    fn try_exec(query_src: &str, source_src: &str) -> Result<String, RuntimeError> {
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

        let effects = vm.execute(&module, 0, &entrypoint)?;
        let materializer = ValueMaterializer::new(source_src, module.types(), module.strings());
        let value = materializer.materialize(effects.as_slice(), entrypoint.result_type());
        Ok(value.format(false, Colors::new(false)))
    }

    fn query_matches(query_src: &str, source_src: &str) -> bool {
        match try_exec(query_src, source_src) {
            Ok(_) => true,
            Err(RuntimeError::NoMatch) => false,
            Err(err) => panic!("unexpected runtime error: {err}"),
        }
    }

    fn query_exec(query_src: &str, source_src: &str) -> String {
        try_exec(query_src, source_src).expect("query should match")
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
        // The first "," can't reach 2 (a non-extra "," intervenes), but the
        // anchored search retries: the second "," is adjacent to 2.
        assert!(query_matches(query, "[1,,2]"));
        // No "," is followed by a number anywhere.
        assert!(!query_matches(query, "[1,,]"));
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

    #[test]
    fn interior_anchor_retries_search_at_later_siblings() {
        let query = r#"Q = (program {(lexical_declaration) @a . (expression_statement) @b})"#;

        // `let x;` is not adjacent to an expression_statement; `let y;` is.
        let res = query_exec(query, "let x; let y; foo;");

        insta::assert_snapshot!(res, @r#"{"a":{"kind":"lexical_declaration","text":"let y;","span":[7, 13]},"b":{"kind":"expression_statement","text":"foo;","span":[14, 18]}}"#);
    }

    #[test]
    fn interior_anchor_rejects_when_no_adjacent_pair_exists() {
        let query = r#"Q = (program {(lexical_declaration) @a .! (expression_statement) @b})"#;

        assert!(!query_matches(query, "let x; /* c */ foo;"));
    }

    #[test]
    fn greedy_quantifier_exit_binds_following_pattern_leftmost() {
        let query = r#"Q = (program {(lexical_declaration)* @decls (_) @x})"#;

        // The quantifier's failed candidates (foo, bar) must not become exit
        // positions: `x` binds the leftmost sibling after the last match.
        let res = query_exec(query, "let a; foo; bar;");

        insta::assert_snapshot!(res, @r#"{"decls":[{"kind":"lexical_declaration","text":"let a;","span":[0, 6]}],"x":{"kind":"expression_statement","text":"foo;","span":[7, 11]}}"#);
    }

    #[test]
    fn non_greedy_plus_matches_leftmost_minimal() {
        let query = r#"Q = (program {(lexical_declaration)+? @d})"#;

        // Non-greediness means fewest iterations, not skipping leftmost candidates.
        let res = query_exec(query, "let a; let b;");

        insta::assert_snapshot!(res, @r#"{"d":[{"kind":"lexical_declaration","text":"let a;","span":[0, 6]}]}"#);
    }

    #[test]
    fn quantifier_skips_non_matching_siblings() {
        let query = r#"Q = (program (lexical_declaration)* @decls)"#;

        let res = query_exec(query, "let a; foo; let b;");

        insta::assert_snapshot!(res, @r#"{"decls":[{"kind":"lexical_declaration","text":"let a;","span":[0, 6]},{"kind":"lexical_declaration","text":"let b;","span":[12, 18]}]}"#);
    }
}
