#![allow(unused)]
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

use indexmap::IndexMap;

use plotnik_core::{NodeFieldId, NodeTypeId};
use plotnik_langs::Lang;

use crate::Diagnostics;
use crate::parser::{ParseResult, Parser, Root, SyntaxNode, lexer::lex};
use crate::query::alt_kinds::validate_alt_kinds;
use crate::query::dependencies::{self, DependencyAnalysisOwned};
use crate::query::expr_arity::{ExprArity, ExprArityTable, infer_arities, resolve_arity};
use crate::query::link;
use crate::query::source_map::{SourceId, SourceMap};
use crate::query::symbol_table::{SymbolTableOwned, resolve_names};

const DEFAULT_QUERY_PARSE_FUEL: u32 = 1_000_000;
const DEFAULT_QUERY_PARSE_MAX_DEPTH: u32 = 4096;

pub type AstMap = IndexMap<SourceId, Root>;

pub struct QueryConfig {
    pub query_parse_fuel: u32,
    pub query_parse_max_depth: u32,
}

pub struct QueryBuilder {
    source_map: SourceMap,
    config: QueryConfig,
}

impl QueryBuilder {
    pub fn new(source_map: SourceMap) -> Self {
        let config = QueryConfig {
            query_parse_fuel: DEFAULT_QUERY_PARSE_FUEL,
            query_parse_max_depth: DEFAULT_QUERY_PARSE_MAX_DEPTH,
        };

        Self { source_map, config }
    }

    pub fn from_str(src: &str) -> Self {
        let source_map = SourceMap::one_liner(src);
        Self::new(source_map)
    }

    pub fn with_query_parse_fuel(mut self, fuel: u32) -> Self {
        self.config.query_parse_fuel = fuel;
        self
    }

    pub fn with_query_parse_recursion_limit(mut self, limit: u32) -> Self {
        self.config.query_parse_max_depth = limit;
        self
    }

    pub fn parse(self) -> crate::Result<QueryParsed> {
        let mut ast = IndexMap::new();
        let mut diag = Diagnostics::new();
        let mut total_fuel_consumed = 0u32;

        for source in self.source_map.iter() {
            let tokens = lex(source.content);
            let parser = Parser::new(
                source.content,
                source.id,
                tokens,
                &mut diag,
                self.config.query_parse_fuel,
                self.config.query_parse_max_depth,
            );

            let res = parser.parse()?;

            validate_alt_kinds(source.id, &res.ast, &mut diag);
            total_fuel_consumed = total_fuel_consumed.saturating_add(res.fuel_consumed);
            ast.insert(source.id, res.ast);
        }

        Ok(QueryParsed {
            source_map: self.source_map,
            diag,
            ast_map: ast,
            fuel_consumed: total_fuel_consumed,
        })
    }
}

#[derive(Debug)]
pub struct QueryParsed {
    source_map: SourceMap,
    ast_map: AstMap,
    diag: Diagnostics,
    fuel_consumed: u32,
}

impl QueryParsed {
    pub fn query_parser_fuel_consumed(&self) -> u32 {
        self.fuel_consumed
    }
}

impl QueryParsed {
    pub fn analyze(mut self) -> QueryAnalyzed {
        // Use reference-based structures for processing
        let symbol_table = resolve_names(&self.source_map, &self.ast_map, &mut self.diag);

        let dependency_analysis = dependencies::analyze_dependencies(&symbol_table);
        dependencies::validate_recursion(
            &dependency_analysis,
            &self.ast_map,
            &symbol_table,
            &mut self.diag,
        );

        let arity_table = infer_arities(&self.ast_map, &symbol_table, &mut self.diag);

        // Convert to owned for storage
        let symbol_table_owned = crate::query::symbol_table::to_owned(symbol_table);
        let dependency_analysis_owned = dependency_analysis.to_owned();

        QueryAnalyzed {
            query_parsed: self,
            symbol_table: symbol_table_owned,
            dependency_analysis: dependency_analysis_owned,
            arity_table,
        }
    }

    pub fn source_map(&self) -> &SourceMap {
        &self.source_map
    }

    pub fn diagnostics(&self) -> Diagnostics {
        self.diag.clone()
    }

    pub fn asts(&self) -> &AstMap {
        &self.ast_map
    }
}

pub type Query = QueryAnalyzed;

pub struct QueryAnalyzed {
    query_parsed: QueryParsed,
    pub symbol_table: SymbolTableOwned,
    dependency_analysis: DependencyAnalysisOwned,
    arity_table: ExprArityTable,
}

impl QueryAnalyzed {
    pub fn is_valid(&self) -> bool {
        !self.diag.has_errors()
    }

    pub fn get_arity(&self, node: &SyntaxNode) -> Option<ExprArity> {
        resolve_arity(node, &self.arity_table)
    }

    pub fn link(mut self, lang: &Lang) -> LinkedQuery {
        // Use reference-based hash maps during processing
        let mut type_ids: HashMap<&str, Option<NodeTypeId>> = HashMap::new();
        let mut field_ids: HashMap<&str, Option<NodeFieldId>> = HashMap::new();

        link::link(
            &self.query_parsed.ast_map,
            &self.query_parsed.source_map,
            lang,
            &self.symbol_table,
            &mut type_ids,
            &mut field_ids,
            &mut self.query_parsed.diag,
        );

        // Convert to owned for storage
        let type_ids_owned = type_ids
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect();
        let field_ids_owned = field_ids
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect();

        LinkedQuery {
            inner: self,
            type_ids: type_ids_owned,
            field_ids: field_ids_owned,
        }
    }
}

impl Deref for QueryAnalyzed {
    type Target = QueryParsed;

    fn deref(&self) -> &Self::Target {
        &self.query_parsed
    }
}

impl DerefMut for QueryAnalyzed {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.query_parsed
    }
}

impl TryFrom<&str> for QueryAnalyzed {
    type Error = crate::Error;

    fn try_from(src: &str) -> crate::Result<Self> {
        Ok(QueryBuilder::new(SourceMap::one_liner(src))
            .parse()?
            .analyze())
    }
}

type NodeTypeIdTableOwned = HashMap<String, Option<NodeTypeId>>;
type NodeFieldIdTableOwned = HashMap<String, Option<NodeFieldId>>;

pub struct LinkedQuery {
    inner: QueryAnalyzed,
    type_ids: NodeTypeIdTableOwned,
    field_ids: NodeFieldIdTableOwned,
}

impl Deref for LinkedQuery {
    type Target = QueryAnalyzed;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for LinkedQuery {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
