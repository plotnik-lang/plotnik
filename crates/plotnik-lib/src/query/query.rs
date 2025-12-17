#![allow(unused)]
use crate::parser::{ParseResult, Parser, lexer::lex};
use crate::query::alt_kinds::validate_alt_kinds;
use crate::query::dependencies;
use crate::query::symbol_table::{SymbolTable, resolve_names};
use crate::{Diagnostics, parser::Root};

const DEFAULT_QUERY_PARSE_FUEL: u32 = 1_000_000;
const DEFAULT_QUERY_PARSE_MAX_DEPTH: u32 = 4096;

pub struct QueryConfig {
    pub query_parse_fuel: u32,
    pub query_parse_max_depth: u32,
}

pub struct QueryBuilder<'q> {
    pub src: &'q str,
    config: QueryConfig,
}

impl<'q> QueryBuilder<'q> {
    pub fn new(src: &'q str) -> Self {
        let config = QueryConfig {
            query_parse_fuel: DEFAULT_QUERY_PARSE_FUEL,
            query_parse_max_depth: DEFAULT_QUERY_PARSE_MAX_DEPTH,
        };

        Self { src, config }
    }

    pub fn with_query_parse_fuel(mut self, fuel: u32) -> Self {
        self.config.query_parse_fuel = fuel;
        self
    }

    pub fn with_query_parse_recursion_limit(mut self, limit: u32) -> Self {
        self.config.query_parse_max_depth = limit;
        self
    }

    pub fn parse(self) -> crate::Result<QueryParsed<'q>> {
        let src = self.src;
        let tokens = lex(src);
        let parser = Parser::new(
            self.src,
            tokens,
            self.config.query_parse_fuel,
            self.config.query_parse_max_depth,
        );

        let ParseResult {
            ast,
            mut diag,
            fuel_consumed,
        } = parser.parse()?;

        validate_alt_kinds(&ast, &mut diag);

        Ok(QueryParsed {
            src,
            diag,
            ast,
            fuel_consumed,
        })
    }
}

pub struct QueryParsed<'q> {
    src: &'q str,
    diag: Diagnostics,
    ast: Root,
    pub fuel_consumed: u32,
}

impl<'q> QueryParsed<'q> {
    pub fn analyze(mut self) -> QueryAnalyzed<'q> {
        let symbol_table = resolve_names(&self.ast, self.src, &mut self.diag);

        let dependency_analysis = dependencies::analyze_dependencies(&symbol_table);
        dependencies::validate_recursion(
            &dependency_analysis,
            &self.ast,
            &symbol_table,
            &mut self.diag,
        );

        QueryAnalyzed {
            query_parsed: self,
            symbol_table,
        }
    }
}

pub struct QueryAnalyzed<'q> {
    query_parsed: QueryParsed<'q>,
    symbol_table: SymbolTable<'q>,
}
