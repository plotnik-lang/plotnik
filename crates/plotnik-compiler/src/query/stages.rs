use std::ops::{Deref, DerefMut};

use indexmap::IndexMap;

use plotnik_core::grammar::Grammar;
use plotnik_core::{Interner, NodeFieldId, NodeType, NodeTypeId, Symbol};

use super::{SourceId, SourceMap};
use crate::Diagnostics;
use crate::analyze::link;
use crate::analyze::symbol_table::{SymbolTable, resolve_names};
use crate::analyze::type_check::{self, Arity, TypeContext};
use crate::analyze::validation::{
    ValidateInput, validate_alt_kinds, validate_anchors, validate_empty_constructs,
    validate_predicates,
};
use crate::analyze::{dependencies, validate_recursion};
use crate::parser::{DEFAULT_FUEL, DEFAULT_MAX_DEPTH, ParseConfig, Parser, Root, SyntaxNode, lex};

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
            query_parse_fuel: DEFAULT_FUEL,
            query_parse_max_depth: DEFAULT_MAX_DEPTH,
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
                ParseConfig {
                    fuel: self.config.query_parse_fuel,
                    max_depth: self.config.query_parse_max_depth,
                },
            );

            let res = parser.parse()?;

            validate_alt_kinds(ValidateInput {
                source_id: source.id,
                ast: res.ast(),
                source_content: None,
                diag: &mut diag,
            });
            validate_anchors(ValidateInput {
                source_id: source.id,
                ast: res.ast(),
                source_content: None,
                diag: &mut diag,
            });
            validate_empty_constructs(ValidateInput {
                source_id: source.id,
                ast: res.ast(),
                source_content: None,
                diag: &mut diag,
            });
            validate_predicates(ValidateInput {
                source_id: source.id,
                ast: res.ast(),
                source_content: Some(source.content),
                diag: &mut diag,
            });
            total_fuel_consumed = total_fuel_consumed.saturating_add(res.fuel_consumed());
            ast.insert(source.id, res.into_ast());
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
        let mut interner = Interner::new();

        let symbol_table = resolve_names(&self.source_map, &self.ast_map, &mut self.diag);

        let dependency_analysis = dependencies::analyze_dependencies(&symbol_table, &mut interner);
        validate_recursion(
            &dependency_analysis,
            &self.ast_map,
            &symbol_table,
            &mut self.diag,
        );

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

/// A unified view of the core analysis context.
///
/// Bundles references to the three main analysis artifacts that downstream
/// modules (compile, emit) commonly need together.
#[derive(Clone, Copy)]
pub struct QueryContext<'q> {
    pub interner: &'q Interner,
    pub type_ctx: &'q TypeContext,
    pub symbol_table: &'q SymbolTable,
}

pub struct QueryAnalyzed {
    query_parsed: QueryParsed,
    interner: Interner,
    symbol_table: SymbolTable,
    type_context: TypeContext,
}

impl QueryAnalyzed {
    pub fn is_valid(&self) -> bool {
        !self.diag.has_errors()
    }

    pub fn context(&self) -> QueryContext<'_> {
        QueryContext {
            interner: &self.interner,
            type_ctx: &self.type_context,
            symbol_table: &self.symbol_table,
        }
    }

    pub fn get_arity(&self, node: &SyntaxNode) -> Option<Arity> {
        use crate::parser::ast;

        if let Some(expr) = ast::Expr::cast(node.clone()) {
            return self.type_context.get_arity(&expr);
        }

        if let Some(root) = ast::Root::cast(node.clone()) {
            return Some(if root.defs().nth(1).is_some() {
                Arity::Many
            } else {
                Arity::One
            });
        }

        if let Some(def) = ast::Def::cast(node.clone()) {
            return def.body().and_then(|b| self.type_context.get_arity(&b));
        }

        if let Some(branch) = ast::Branch::cast(node.clone()) {
            return branch.body().and_then(|b| self.type_context.get_arity(&b));
        }

        None
    }

    pub fn type_context(&self) -> &TypeContext {
        &self.type_context
    }

    pub fn symbol_table(&self) -> &SymbolTable {
        &self.symbol_table
    }

    pub fn interner(&self) -> &Interner {
        &self.interner
    }

    pub fn link(mut self, grammar: &Grammar) -> LinkedQuery {
        let mut output = link::LinkOutput::default();

        link::link(
            &mut self.interner,
            grammar,
            &self.query_parsed.source_map,
            &self.query_parsed.ast_map,
            &self.symbol_table,
            &mut output,
            &mut self.query_parsed.diag,
        );

        LinkedQuery {
            inner: self,
            linking: output,
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
    linking: link::LinkOutput,
}

impl LinkedQuery {
    pub fn interner(&self) -> &Interner {
        &self.inner.interner
    }

    pub fn node_type_ids(&self) -> &IndexMap<NodeType<Symbol>, NodeTypeId> {
        self.linking.node_type_ids()
    }

    pub fn node_field_ids(&self) -> &IndexMap<Symbol, NodeFieldId> {
        self.linking.node_field_ids()
    }

    /// Emit bytecode. Returns `Err(EmitError::InvalidQuery)` if the query has errors.
    pub fn emit(&self) -> Result<Vec<u8>, crate::emit::EmitError> {
        if !self.is_valid() {
            return Err(crate::emit::EmitError::InvalidQuery);
        }
        crate::emit::emit(self)
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
