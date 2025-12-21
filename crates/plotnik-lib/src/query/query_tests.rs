use plotnik_langs::Lang;

use crate::{
    SourceMap,
    query::query::{LinkedQuery, QueryAnalyzed, QueryBuilder},
};

impl QueryAnalyzed {
    #[track_caller]
    fn parse_and_validate(src: &str) -> Self {
        let source_map = SourceMap::one_liner(src);
        let query = QueryBuilder::new(source_map).parse().unwrap().analyze();
        if !query.is_valid() {
            panic!(
                "Expected valid query, got error:\n{}",
                query.dump_diagnostics()
            );
        }
        query
    }

    #[track_caller]
    pub fn expect(src: &str) -> Self {
        let source_map = SourceMap::one_liner(src);
        QueryBuilder::new(source_map).parse().unwrap().analyze()
    }

    #[track_caller]
    pub fn expect_valid(src: &str) -> Self {
        Self::parse_and_validate(src)
    }

    #[track_caller]
    pub fn expect_valid_cst(src: &str) -> String {
        Self::parse_and_validate(src).dump_cst()
    }

    #[track_caller]
    pub fn expect_valid_cst_full(src: &str) -> String {
        Self::parse_and_validate(src).dump_cst_full()
    }

    #[track_caller]
    pub fn expect_valid_ast(src: &str) -> String {
        Self::parse_and_validate(src).dump_ast()
    }

    #[track_caller]
    pub fn expect_valid_arities(src: &str) -> String {
        Self::parse_and_validate(src).dump_with_arities()
    }

    #[track_caller]
    pub fn expect_valid_symbols(src: &str) -> String {
        Self::parse_and_validate(src).dump_symbols()
    }

    #[track_caller]
    pub fn expect_valid_linking(src: &str, lang: &Lang) -> LinkedQuery {
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
    pub fn expect_invalid_linking(src: &str, lang: &Lang) -> String {
        let query = Self::parse_and_validate(src).link(lang);
        if query.is_valid() {
            panic!("Expected failed linking, got valid");
        }
        query.dump_diagnostics()
    }

    #[track_caller]
    pub fn expect_invalid(src: &str) -> String {
        let source_map = SourceMap::one_liner(src);
        let query = QueryBuilder::new(source_map).parse().unwrap().analyze();
        if query.is_valid() {
            panic!("Expected invalid query, got valid:\n{}", query.dump_cst());
        }
        query.dump_diagnostics()
    }
}

#[test]
fn invalid_three_way_mutual_recursion_across_files() {
    let mut source_map = SourceMap::new();
    source_map.add_file("a.ptk", "A = (a (B))");
    source_map.add_file("b.ptk", "B = (b (C))");
    source_map.add_file("c.ptk", "C = (c (A))");

    let query = QueryBuilder::new(source_map).parse().unwrap().analyze();

    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: infinite recursion: cycle has no escape path
     --> c.ptk:1:9
      |
    1 | C = (c (A))
      | -       ^
      | |       |
      | |       references A
      | C is defined here
      |
     ::: a.ptk:1:9
      |
    1 | A = (a (B))
      |         - references B
      |
     ::: b.ptk:1:9
      |
    1 | B = (b (C))
      |         - references C (completing cycle)
    ");
}

#[test]
fn multifile_field_with_ref_to_seq_error() {
    let mut source_map = SourceMap::new();
    source_map.add_file("defs.ptk", "X = {(a) (b)}");
    source_map.add_file("main.ptk", "Q = (call name: (X))");

    let query = QueryBuilder::new(source_map).parse().unwrap().analyze();

    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: field `name` must match exactly one node, not a sequence
     --> main.ptk:1:17
      |
    1 | Q = (call name: (X))
      |                 ^^^
      |
     ::: defs.ptk:1:5
      |
    1 | X = {(a) (b)}
      |     --------- defined here
    ");
}
