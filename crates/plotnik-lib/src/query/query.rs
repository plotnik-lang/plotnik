#![allow(unused)]
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

use indexmap::IndexMap;

use plotnik_core::{Interner, NodeFieldId, NodeTypeId, Symbol};
use plotnik_langs::Lang;

use crate::Diagnostics;
use crate::parser::{ParseResult, Parser, Root, SyntaxNode, lexer::lex};
use crate::query::alt_kinds::validate_alt_kinds;
use crate::query::anchors::validate_anchors;
use crate::query::dependencies;
use crate::query::link;
use crate::query::source_map::{SourceId, SourceMap};
use crate::query::symbol_table::{SymbolTable, resolve_names};
use crate::query::type_check::{self, Arity, TypeContext};

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

    pub fn one_liner(src: &str) -> Self {
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
            validate_anchors(source.id, &res.ast, &mut diag);
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
        // Create shared interner for all phases
        let mut interner = Interner::new();

        // Use reference-based structures for processing
        let symbol_table = resolve_names(&self.source_map, &self.ast_map, &mut self.diag);

        let dependency_analysis = dependencies::analyze_dependencies(&symbol_table, &mut interner);
        dependencies::validate_recursion(
            &dependency_analysis,
            &self.ast_map,
            &symbol_table,
            &mut self.diag,
        );

        // Unified type checking pass
        let type_context = type_check::infer_types(
            &mut interner,
            &self.ast_map,
            &symbol_table,
            &dependency_analysis,
            &mut self.diag,
        );

        QueryAnalyzed {
            query_parsed: self,
            interner,
            symbol_table,
            type_context,
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
    interner: Interner,
    pub symbol_table: SymbolTable,
    type_context: TypeContext,
}

impl QueryAnalyzed {
    pub fn is_valid(&self) -> bool {
        !self.diag.has_errors()
    }

    pub fn get_arity(&self, node: &SyntaxNode) -> Option<Arity> {
        use crate::parser::ast::{self, Expr};

        // Try casting to Expr first as it's the most common query
        if let Some(expr) = ast::Expr::cast(node.clone()) {
            return self.type_context.get_arity(&expr);
        }

        // Root: arity based on definition count
        if let Some(root) = ast::Root::cast(node.clone()) {
            return Some(if root.defs().nth(1).is_some() {
                Arity::Many
            } else {
                Arity::One
            });
        }

        // Def: delegate to body's arity
        if let Some(def) = ast::Def::cast(node.clone()) {
            return def.body().and_then(|b| self.type_context.get_arity(&b));
        }

        // Branch: delegate to body's arity
        if let Some(branch) = ast::Branch::cast(node.clone()) {
            return branch.body().and_then(|b| self.type_context.get_arity(&b));
        }

        None
    }

    pub fn type_context(&self) -> &TypeContext {
        &self.type_context
    }

    pub fn interner(&self) -> &Interner {
        &self.interner
    }

    /// Emit bytecode (types only, no node validation).
    pub fn emit(&self) -> Result<Vec<u8>, super::emit::EmitError> {
        super::emit::emit(&self.type_context, &self.interner)
    }

    pub fn link(mut self, lang: &Lang) -> LinkedQuery {
        let mut output = link::LinkOutput::default();

        link::link(
            &mut self.interner,
            lang,
            &self.query_parsed.source_map,
            &self.query_parsed.ast_map,
            &self.symbol_table,
            &mut output,
            &mut self.query_parsed.diag,
        );

        LinkedQuery {
            inner: self,
            node_type_ids: output.node_type_ids,
            node_field_ids: output.node_field_ids,
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

pub struct LinkedQuery {
    inner: QueryAnalyzed,
    node_type_ids: HashMap<Symbol, NodeTypeId>,
    node_field_ids: HashMap<Symbol, NodeFieldId>,
}

impl LinkedQuery {
    pub fn interner(&self) -> &Interner {
        &self.inner.interner
    }

    pub fn node_type_ids(&self) -> &HashMap<Symbol, NodeTypeId> {
        &self.node_type_ids
    }

    pub fn node_field_ids(&self) -> &HashMap<Symbol, NodeFieldId> {
        &self.node_field_ids
    }

    /// Emit bytecode (includes node type/field validation info).
    pub fn emit(&self) -> Result<Vec<u8>, super::emit::EmitError> {
        super::emit::emit_linked(self)
    }
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
