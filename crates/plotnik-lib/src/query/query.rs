#![allow(unused)]
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

use plotnik_core::{NodeFieldId, NodeTypeId};
use plotnik_langs::Lang;

use crate::parser::{ParseResult, Parser, lexer::lex};
use crate::query::alt_kinds::validate_alt_kinds;
use crate::query::dependencies::{self, DependencyAnalysis};
use crate::query::expr_arity::{ExprArityTable, infer_arities};
use crate::query::graph_qis::{QisContext, detect_capture_scopes};
use crate::query::link;
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

        if diag.has_errors() {
            return Err(crate::Error::QueryParseError(diag));
        }

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
    pub fn analyze(mut self) -> crate::Result<QueryAnalyzed<'q>> {
        let symbol_table = resolve_names(&self.ast, self.src, &mut self.diag);

        let dependency_analysis = dependencies::analyze_dependencies(&symbol_table);
        dependencies::validate_recursion(
            &dependency_analysis,
            &self.ast,
            &symbol_table,
            &mut self.diag,
        );

        let arity_table = infer_arities(&self.ast, &symbol_table, &mut self.diag);

        if self.diag.has_errors() {
            return Err(crate::Error::QueryAnalyzeError(self.diag));
        }

        let qis_ctx = detect_capture_scopes(self.src, &symbol_table);

        Ok(QueryAnalyzed {
            query_parsed: self,
            symbol_table,
            dependency_analysis,
            arity_table,
            qis_ctx,
        })
    }
}

pub struct QueryAnalyzed<'q> {
    query_parsed: QueryParsed<'q>,
    symbol_table: SymbolTable<'q>,
    dependency_analysis: DependencyAnalysis<'q>,
    arity_table: ExprArityTable,
    qis_ctx: QisContext<'q>,
}

impl<'q> QueryAnalyzed<'q> {
    pub fn link(mut self, lang: &Lang) -> LinkedQuery<'q> {
        let mut type_ids: HashMap<&'q str, Option<NodeTypeId>> = HashMap::new();
        let mut field_ids: HashMap<&'q str, Option<NodeFieldId>> = HashMap::new();

        link::link(
            &self.query_parsed.ast,
            self.query_parsed.src,
            lang,
            &self.symbol_table,
            &mut type_ids,
            &mut field_ids,
            &mut self.query_parsed.diag,
        );

        LinkedQuery {
            inner: self,
            type_ids,
            field_ids,
        }
    }
}

impl<'q> Deref for QueryAnalyzed<'q> {
    type Target = QueryParsed<'q>;

    fn deref(&self) -> &Self::Target {
        &self.query_parsed
    }
}

impl<'q> DerefMut for QueryAnalyzed<'q> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.query_parsed
    }
}

type NodeTypeIdTable<'q> = HashMap<&'q str, Option<NodeTypeId>>;
type NodeFieldIdTable<'q> = HashMap<&'q str, Option<NodeFieldId>>;

pub struct LinkedQuery<'q> {
    inner: QueryAnalyzed<'q>,
    type_ids: NodeTypeIdTable<'q>,
    field_ids: NodeFieldIdTable<'q>,
}
