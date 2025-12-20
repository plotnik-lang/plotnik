use plotnik_langs::Lang;

use crate::query::query::{LinkedQuery, QueryAnalyzed, QueryBuilder};

impl<'q> QueryAnalyzed<'q> {
    #[track_caller]
    fn parse_and_validate(src: &'q str) -> Self {
        let query = QueryBuilder::new(src).parse().unwrap().analyze();
        if !query.is_valid() {
            panic!(
                "Expected valid query, got error:\n{}",
                query.dump_diagnostics()
            );
        }
        query
    }

    #[track_caller]
    pub fn expect(src: &'q str) -> Self {
        QueryBuilder::new(src).parse().unwrap().analyze()
    }

    #[track_caller]
    pub fn expect_valid(src: &'q str) -> Self {
        Self::parse_and_validate(src)
    }

    #[track_caller]
    pub fn expect_valid_cst(src: &'q str) -> String {
        Self::parse_and_validate(src).dump_cst()
    }

    #[track_caller]
    pub fn expect_valid_cst_full(src: &'q str) -> String {
        Self::parse_and_validate(src).dump_cst_full()
    }

    #[track_caller]
    pub fn expect_valid_ast(src: &'q str) -> String {
        Self::parse_and_validate(src).dump_ast()
    }

    #[track_caller]
    pub fn expect_valid_arities(src: &'q str) -> String {
        Self::parse_and_validate(src).dump_with_arities()
    }

    #[track_caller]
    pub fn expect_valid_symbols(src: &'q str) -> String {
        Self::parse_and_validate(src).dump_symbols()
    }

    #[track_caller]
    pub fn expect_valid_linking(src: &'q str, lang: &Lang) -> LinkedQuery<'q> {
        let query = Self::parse_and_validate(src).link(lang);
        if !query.is_valid() {
            panic!(
                "Expected valid linking, got error:\n{}",
                query.dump_diagnostics()
            );
        }
        query
    }

    #[track_caller]
    pub fn expect_invalid_linking(src: &'q str, lang: &Lang) -> String {
        let query = Self::parse_and_validate(src).link(lang);
        if query.is_valid() {
            panic!("Expected failed linking, got valid");
        }
        query.dump_diagnostics()
    }

    #[track_caller]
    pub fn expect_invalid(src: &'q str) -> String {
        let query = QueryBuilder::new(src).parse().unwrap().analyze();
        if query.is_valid() {
            panic!("Expected invalid query, got valid:\n{}", query.dump_cst());
        }
        query.dump_diagnostics()
    }
}
