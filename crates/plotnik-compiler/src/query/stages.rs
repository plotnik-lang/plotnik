use indexmap::IndexMap;
use rowan::TextRange;

use plotnik_core::grammar::Grammar;
use plotnik_core::Interner;

use super::{SourceId, SourceMap};
use crate::Diagnostics;
use crate::analyze::link;
use crate::analyze::symbol_table::{SymbolTable, resolve_names};
use crate::analyze::type_check::{self, Arity, TypeAnalysis};
use crate::analyze::validation::{AstValidationInput, validate_ast};
use crate::analyze::{dependencies, validate_entrypoints, validate_recursion};
use crate::compile::{
    CompileCtx, Compiler, collapse_up, eliminate_epsilons, lower, remove_unreachable,
};
use crate::diagnostics::DiagnosticKind;
use crate::emit::EmitInput;
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

        let (symbol_table, type_analysis, dependency_analysis) = {
            let validated = validate_ast(AstValidationInput {
                source_map: &self.source_map,
                ast_map: &self.ast_map,
                diag: &mut self.diag,
            });

            let symbol_table = resolve_names(&validated, &mut self.diag);

            let dependency_analysis =
                dependencies::analyze_dependencies(&symbol_table, &mut interner);
            validate_recursion(
                &dependency_analysis,
                validated.ast_map(),
                &symbol_table,
                &mut self.diag,
            );

            let type_analysis = type_check::infer_types(
                &mut interner,
                &symbol_table,
                &dependency_analysis,
                &mut self.diag,
            );
            if !self.diag.has_errors() {
                validate_entrypoints(
                    validated.ast_map(),
                    &interner,
                    &type_analysis,
                    &dependency_analysis,
                    &mut self.diag,
                );
            }

            (symbol_table, type_analysis, dependency_analysis)
        };

        Query {
            parsed: self,
            interner,
            symbol_table,
            type_analysis,
            dependency_analysis,
        }
    }

    pub fn source_map(&self) -> &SourceMap {
        &self.source_map
    }

    pub fn diagnostics(&self) -> &Diagnostics {
        &self.diag
    }

    pub fn ast_map(&self) -> &AstMap {
        &self.ast_map
    }

}

pub struct Query {
    parsed: QueryParsed,
    interner: Interner,
    symbol_table: SymbolTable,
    type_analysis: TypeAnalysis,
    dependency_analysis: dependencies::DependencyAnalysis,
}

impl Query {
    pub fn is_valid(&self) -> bool {
        !self.parsed.diag.has_errors()
    }

    pub fn arity(&self, node: &SyntaxNode) -> Option<Arity> {
        use crate::parser::ast;

        if let Some(pattern) = ast::Pattern::cast(node.clone()) {
            return self.type_analysis.arity(&pattern);
        }

        if let Some(root) = ast::Root::cast(node.clone()) {
            return Some(if root.defs().nth(1).is_some() {
                Arity::Many
            } else {
                Arity::One
            });
        }

        if let Some(def) = ast::Def::cast(node.clone()) {
            return def.body().and_then(|b| self.type_analysis.arity(&b));
        }

        if let Some(branch) = ast::Branch::cast(node.clone()) {
            return branch.body().and_then(|b| self.type_analysis.arity(&b));
        }

        None
    }

    pub fn type_analysis(&self) -> &TypeAnalysis {
        &self.type_analysis
    }

    pub fn symbol_table(&self) -> &SymbolTable {
        &self.symbol_table
    }

    pub fn interner(&self) -> &Interner {
        &self.interner
    }

    pub fn dependency_analysis(&self) -> &dependencies::DependencyAnalysis {
        &self.dependency_analysis
    }

    pub fn source_map(&self) -> &SourceMap {
        self.parsed.source_map()
    }

    pub fn diagnostics(&self) -> &Diagnostics {
        self.parsed.diagnostics()
    }

    pub fn ast_map(&self) -> &AstMap {
        self.parsed.ast_map()
    }

    pub fn link(mut self, grammar: &Grammar) -> GrammarBoundQuery {
        let mut output = link::GrammarBindingBuilder::new();
        let parsed = &mut self.parsed;

        link::GrammarLinkCtx {
            interner: &mut self.interner,
            grammar,
            source_map: &parsed.source_map,
            ast_map: &parsed.ast_map,
            symbol_table: &self.symbol_table,
        }
        .link(&mut output, &mut parsed.diag);

        GrammarBoundQuery {
            analyzed: self,
            grammar: output.finish(),
        }
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
    pub fn is_valid(&self) -> bool {
        self.analyzed.is_valid()
    }

    pub fn interner(&self) -> &Interner {
        &self.analyzed.interner
    }

    pub fn type_analysis(&self) -> &TypeAnalysis {
        self.analyzed.type_analysis()
    }

    pub fn symbol_table(&self) -> &SymbolTable {
        self.analyzed.symbol_table()
    }

    pub fn dependency_analysis(&self) -> &dependencies::DependencyAnalysis {
        self.analyzed.dependency_analysis()
    }

    pub fn source_map(&self) -> &SourceMap {
        self.analyzed.source_map()
    }

    pub fn diagnostics(&self) -> &Diagnostics {
        self.analyzed.diagnostics()
    }

    pub fn ast_map(&self) -> &AstMap {
        self.analyzed.ast_map()
    }

    pub fn arity(&self, node: &SyntaxNode) -> Option<Arity> {
        self.analyzed.arity(node)
    }

    pub fn grammar(&self) -> &link::GrammarBinding {
        &self.grammar
    }

    /// Emit bytecode. Returns `Err(EmitError::InvalidQuery)` if the query has errors.
    pub fn emit(&self) -> Result<Vec<u8>, crate::emit::EmitError> {
        if !self.is_valid() {
            return Err(crate::emit::EmitError::InvalidQuery);
        }
        let compile_result = self.compile();
        crate::emit::emit(self.emit_input(), &compile_result)
    }

    /// Like [`emit`](Self::emit), but without the emitter's debug load self-check.
    /// The caller must load the bytecode itself; used by [`check_compile`](Self::check_compile)
    /// so a malformed-bytecode case surfaces as a diagnostic instead of the debug panic.
    pub fn emit_unchecked(&self) -> Result<Vec<u8>, crate::emit::EmitError> {
        if !self.is_valid() {
            return Err(crate::emit::EmitError::InvalidQuery);
        }
        let compile_result = self.compile();
        crate::emit::emit_unchecked(self.emit_input(), &compile_result)
    }

    /// Full-pipeline dry run for `check`: emit bytecode and load it, reporting any
    /// failure as a diagnostic instead of panicking. Returns the analyze/link
    /// diagnostics plus any emit/load failure; the caller inspects `has_errors()`.
    ///
    /// Uses [`emit_unchecked`](Self::emit_unchecked) and loads the bytecode itself,
    /// so it never reaches the emitter's debug self-check panic — in debug or release.
    pub fn check_compile(&self) -> Diagnostics {
        let mut diag = self.diagnostics().clone();
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
            Ok(_) => {}
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

    fn compile(&self) -> crate::bytecode::CompileResult {
        let ctx = CompileCtx {
            interner: self.interner(),
            type_ctx: self.type_analysis(),
            symbol_table: self.symbol_table(),
            grammar: self.grammar(),
            dependency_analysis: self.dependency_analysis(),
        };
        let mut ir = Compiler::build_ir(&ctx);
        crate::compile::verify::run_verified(
            "eliminate_epsilons",
            &mut ir,
            &ctx,
            eliminate_epsilons,
        );
        crate::compile::verify::run_verified("remove_unreachable", &mut ir, &ctx, remove_unreachable);
        crate::compile::verify::run_verified("collapse_up", &mut ir, &ctx, collapse_up);
        crate::compile::verify::run_verified("lower", &mut ir, &ctx, lower);
        ir
    }

    fn emit_input(&self) -> EmitInput<'_> {
        EmitInput {
            interner: self.interner(),
            type_ctx: self.type_analysis(),
            dependency_analysis: self.dependency_analysis(),
            grammar: self.grammar(),
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
