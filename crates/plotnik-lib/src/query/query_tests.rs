use plotnik_langs::{Lang, from_name};

use crate::{
    SourceMap,
    bytecode::Module,
    query::query::{LinkedQuery, QueryAnalyzed, QueryBuilder},
};

fn javascript() -> Lang {
    from_name("javascript").expect("javascript lang")
}

macro_rules! expect_invalid {
    ($($name:literal: $content:literal),+ $(,)?) => {{
        let mut source_map = SourceMap::new();
        $(source_map.add_file($name, $content);)+
        let query = QueryBuilder::new(source_map).parse().unwrap().analyze();
        if query.is_valid() {
            panic!("Expected invalid query, got valid");
        }
        query.dump_diagnostics()
    }};
}

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
    pub fn expect_valid_linking(src: &str) -> LinkedQuery {
        let query = Self::parse_and_validate(src).link(&javascript());
        if !query.is_valid() {
            panic!(
                "Expected valid linking, got error:\n{}",
                query.dump_diagnostics()
            );
        }
        query
    }

    #[track_caller]
    pub fn expect_invalid_linking(src: &str) -> String {
        let query = Self::parse_and_validate(src).link(&javascript());
        if query.is_valid() {
            panic!("Expected failed linking, got valid");
        }
        query.dump_diagnostics()
    }

    #[track_caller]
    pub fn expect_valid_types(src: &str) -> String {
        let query = Self::parse_and_validate(src);
        if !query.is_valid() {
            panic!(
                "Expected valid types, got error:\n{}",
                query.dump_diagnostics()
            );
        }

        // Emit to bytecode and then emit TypeScript from the bytecode module
        let bytecode = query.emit().expect("bytecode emission should succeed");
        let module = Module::from_bytes(bytecode).expect("module loading should succeed");
        crate::typegen::typescript::emit(&module)
    }

    #[track_caller]
    pub fn expect_valid_bytecode(src: &str) -> String {
        let query = Self::parse_and_validate(src);
        let bytecode = query.emit().expect("bytecode emission should succeed");
        let module = Module::from_bytes(bytecode).expect("module loading should succeed");
        crate::bytecode::dump(&module, crate::Colors::OFF)
    }

    #[track_caller]
    pub fn expect_valid_linked_bytecode(src: &str) -> String {
        let query = Self::parse_and_validate(src).link(&javascript());
        if !query.is_valid() {
            panic!(
                "Expected valid linking, got error:\n{}",
                query.dump_diagnostics()
            );
        }
        let bytecode = query.emit().expect("bytecode emission should succeed");
        let module = Module::from_bytes(bytecode).expect("module loading should succeed");
        crate::bytecode::dump(&module, crate::Colors::OFF)
    }

    #[track_caller]
    pub fn expect_valid_bytes(src: &str) -> Vec<u8> {
        let query = Self::parse_and_validate(src);
        query.emit().expect("bytecode emission should succeed")
    }

    #[track_caller]
    pub fn expect_valid_linked_bytes(src: &str) -> Vec<u8> {
        let query = Self::parse_and_validate(src).link(&javascript());
        if !query.is_valid() {
            panic!(
                "Expected valid linking, got error:\n{}",
                query.dump_diagnostics()
            );
        }
        query.emit().expect("bytecode emission should succeed")
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

    #[track_caller]
    pub fn expect_warning(src: &str) -> String {
        let source_map = SourceMap::one_liner(src);
        let query = QueryBuilder::new(source_map).parse().unwrap().analyze();

        if !query.is_valid() {
            panic!(
                "Expected valid query with warning, got error:\n{}",
                query.dump_diagnostics()
            );
        }

        if !query.diagnostics().has_warnings() {
            panic!("Expected warning, got none:\n{}", query.dump_cst());
        }

        query.dump_diagnostics()
    }

    #[track_caller]
    pub fn expect_cst_with_warnings(src: &str) -> String {
        let source_map = SourceMap::one_liner(src);
        let query = QueryBuilder::new(source_map).parse().unwrap().analyze();

        if !query.is_valid() {
            panic!(
                "Expected valid query (warnings ok), got error:\n{}",
                query.dump_diagnostics()
            );
        }

        query.dump_cst()
    }
}

#[test]
fn invalid_three_way_mutual_recursion_across_files() {
    let res = expect_invalid! {
        "a.ptk": "A = (a (B))",
        "b.ptk": "B = (b (C))",
        "c.ptk": "C = (c (A))",
    };

    insta::assert_snapshot!(res, @r"
    error: infinite recursion: no escape path
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
      |
    help: add a non-recursive branch to terminate: `[Base: ... Rec: (Self)]`
    ");
}

#[test]
fn multifile_field_with_ref_to_seq_error() {
    let res = expect_invalid! {
        "defs.ptk": "X = {(a) (b)}",
        "main.ptk": "Q = (call name: (X))",
    };

    insta::assert_snapshot!(res, @r"
    error: field `name` cannot match a sequence
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
