use std::collections::HashSet;
use std::ops::{Deref, DerefMut};

use indexmap::IndexMap;
use rowan::TextRange;

use plotnik_core::grammar::Grammar;
use plotnik_core::{Interner, NodeFieldId, NodeKind, NodeKindId, Symbol};

use super::{SourceId, SourceMap};
use crate::Diagnostics;
use crate::diagnostics::DiagnosticKind;
use crate::analyze::link;
use crate::analyze::symbol_table::{SymbolTable, resolve_names};
use crate::analyze::type_check::{self, Arity, TypeContext};
use crate::analyze::validation::{
    PredicateInput, ValidationInput, validate_alt_kinds, validate_anchors, validate_empty_constructs,
    validate_predicates,
};
use crate::analyze::{dependencies, validate_recursion};
use crate::parser::{DEFAULT_FUEL, DEFAULT_MAX_DEPTH, ParseConfig, Parser, Root, SyntaxNode, lex};

pub type AstMap = IndexMap<SourceId, Root>;

pub struct QueryConfig {
    pub parse_fuel: u32,
    pub parse_max_depth: u32,
}

pub struct QueryBuilder {
    source_map: SourceMap,
    config: QueryConfig,
}

impl QueryBuilder {
    pub fn new(source_map: SourceMap) -> Self {
        let config = QueryConfig {
            parse_fuel: DEFAULT_FUEL,
            parse_max_depth: DEFAULT_MAX_DEPTH,
        };

        Self { source_map, config }
    }

    pub fn from_inline(src: &str) -> Self {
        let source_map = SourceMap::from_inline(src);
        Self::new(source_map)
    }

    pub fn with_parse_fuel(mut self, fuel: u32) -> Self {
        self.config.parse_fuel = fuel;
        self
    }

    pub fn with_parse_max_depth(mut self, limit: u32) -> Self {
        self.config.parse_max_depth = limit;
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
                    fuel: self.config.parse_fuel,
                    max_depth: self.config.parse_max_depth,
                },
            );

            let res = parser.parse()?;

            validate_alt_kinds(ValidationInput {
                source_id: source.id,
                ast: res.ast(),
                diag: &mut diag,
            });
            validate_anchors(ValidationInput {
                source_id: source.id,
                ast: res.ast(),
                diag: &mut diag,
            });
            validate_empty_constructs(ValidationInput {
                source_id: source.id,
                ast: res.ast(),
                diag: &mut diag,
            });
            validate_predicates(PredicateInput {
                source_id: source.id,
                ast: res.ast(),
                source_content: source.content,
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
    pub fn fuel_consumed(&self) -> u32 {
        self.fuel_consumed
    }
}

impl QueryParsed {
    pub fn analyze(mut self) -> Query {
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

        Query {
            parsed: self,
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

    pub fn ast_map(&self) -> &AstMap {
        &self.ast_map
    }
}

pub struct Query {
    parsed: QueryParsed,
    interner: Interner,
    symbol_table: SymbolTable,
    type_context: TypeContext,
}

impl Query {
    pub fn is_valid(&self) -> bool {
        !self.diag.has_errors()
    }

    pub fn arity(&self, node: &SyntaxNode) -> Option<Arity> {
        use crate::parser::ast;

        if let Some(pattern) = ast::Pattern::cast(node.clone()) {
            return self.type_context.arity(&pattern);
        }

        if let Some(root) = ast::Root::cast(node.clone()) {
            return Some(if root.defs().nth(1).is_some() {
                Arity::Many
            } else {
                Arity::One
            });
        }

        if let Some(def) = ast::Def::cast(node.clone()) {
            return def.body().and_then(|b| self.type_context.arity(&b));
        }

        if let Some(branch) = ast::Branch::cast(node.clone()) {
            return branch.body().and_then(|b| self.type_context.arity(&b));
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

    pub fn link(mut self, grammar: &Grammar) -> GrammarBoundQuery {
        let mut output = link::GrammarBinding::default();

        link::GrammarLinkCtx {
            interner: &mut self.interner,
            grammar,
            source_map: &self.parsed.source_map,
            ast_map: &self.parsed.ast_map,
            symbol_table: &self.symbol_table,
        }
        .link(&mut output, &mut self.parsed.diag);

        GrammarBoundQuery {
            analyzed: self,
            grammar: output,
        }
    }
}

impl Deref for Query {
    type Target = QueryParsed;

    fn deref(&self) -> &Self::Target {
        &self.parsed
    }
}

impl DerefMut for Query {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.parsed
    }
}

impl TryFrom<&str> for Query {
    type Error = crate::Error;

    fn try_from(src: &str) -> crate::Result<Self> {
        Ok(QueryBuilder::new(SourceMap::from_inline(src))
            .parse()?
            .analyze())
    }
}

pub struct GrammarBoundQuery {
    analyzed: Query,
    grammar: link::GrammarBinding,
}

impl GrammarBoundQuery {
    pub fn interner(&self) -> &Interner {
        &self.analyzed.interner
    }

    pub fn node_kind_ids(&self) -> &IndexMap<NodeKind<Symbol>, NodeKindId> {
        self.grammar.node_kind_ids()
    }

    pub fn node_field_ids(&self) -> &IndexMap<Symbol, NodeFieldId> {
        self.grammar.node_field_ids()
    }

    /// Emit bytecode. Returns `Err(EmitError::InvalidQuery)` if the query has errors.
    pub fn emit(&self) -> Result<Vec<u8>, crate::emit::EmitError> {
        if !self.is_valid() {
            return Err(crate::emit::EmitError::InvalidQuery);
        }
        crate::emit::emit(self)
    }

    /// Like [`emit`](Self::emit), but without the emitter's debug load self-check.
    /// The caller must load the bytecode itself; used by [`check_compile`](Self::check_compile)
    /// so a malformed-bytecode case surfaces as a diagnostic instead of the debug panic.
    pub fn emit_unchecked(&self) -> Result<Vec<u8>, crate::emit::EmitError> {
        if !self.is_valid() {
            return Err(crate::emit::EmitError::InvalidQuery);
        }
        crate::emit::emit_unchecked(self)
    }

    /// Full-pipeline dry run for `check`: emit bytecode and load it, reporting any
    /// failure as a diagnostic instead of panicking. Returns the analyze/link
    /// diagnostics plus any emit/load failure; the caller inspects `has_errors()`.
    ///
    /// Uses [`emit_unchecked`](Self::emit_unchecked) and loads the bytecode itself,
    /// so it never reaches the emitter's debug self-check panic — in debug or release.
    pub fn check_compile(&self) -> Diagnostics {
        let mut diag = self.diagnostics();
        if diag.has_errors() {
            return diag;
        }

        let bytes = match self.emit_unchecked() {
            Ok(bytes) => bytes,
            Err(err) => {
                if let Some((source, range)) = self.fallback_span() {
                    diag.report(source, DiagnosticKind::EmitFailed, range)
                        .detail(err.to_string())
                        .emit();
                }
                return diag;
            }
        };

        match plotnik_bytecode::Module::load(&bytes) {
            Ok(_) => self.report_value_less_defs(&mut diag),
            Err(err) => {
                if let Some((source, range)) = self.fallback_span() {
                    diag.report(source, DiagnosticKind::BytecodeRejected, range)
                        .detail(err.to_string())
                        .emit();
                }
            }
        }

        diag
    }

    /// Report every definition that compiles to no entrypoint. A value-less body
    /// (`.`, `-field`, `.!`) yields no type, so the emitter silently omits it;
    /// `iter_def_types` is exactly the set of defs that do emit an entrypoint, so
    /// any def outside it is dropped — including when other defs compile fine.
    fn report_value_less_defs(&self, diag: &mut Diagnostics) {
        let typed: HashSet<Symbol> = self
            .type_context()
            .iter_def_types()
            .map(|(def_id, _)| self.type_context().def_name_sym(def_id))
            .collect();

        for (source_id, root) in self.ast_map() {
            for def in root.defs() {
                let Some(name) = def.name() else { continue };
                let has_entrypoint = self
                    .interner()
                    .get(name.text())
                    .is_some_and(|sym| typed.contains(&sym));
                if !has_entrypoint {
                    diag.report(*source_id, DiagnosticKind::NoEntrypoints, name.text_range())
                        .emit();
                }
            }
        }
    }

    /// A coarse fallback span for emit/load failures, none of which carry a
    /// source mapping. Points at the whole first source; the diagnostic's detail
    /// carries the specifics. `None` when the query has no sources at all, so the
    /// dry run is total even on an empty source map.
    fn fallback_span(&self) -> Option<(SourceId, TextRange)> {
        let source = self.source_map().iter().next()?;
        let len = u32::try_from(source.content.len()).unwrap_or(u32::MAX);
        Some((source.id, TextRange::up_to(len.into())))
    }
}

impl Deref for GrammarBoundQuery {
    type Target = Query;

    fn deref(&self) -> &Self::Target {
        &self.analyzed
    }
}

impl DerefMut for GrammarBoundQuery {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.analyzed
    }
}
