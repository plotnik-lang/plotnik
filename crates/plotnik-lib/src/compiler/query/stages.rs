use indexmap::IndexMap;
use rowan::TextRange;

use crate::compiler::analyze::AnalysisArtifacts;
use crate::compiler::analyze::grammar::link;
use crate::compiler::analyze::grammar::{
    DEFAULT_SATISFY_STEP_BUDGET, GrammarBinding, GrammarBindingBuilder,
};
use crate::compiler::analyze::names::{SymbolTable, resolve_names};
use crate::compiler::analyze::refs::{dependencies, validate_recursion};
use crate::compiler::analyze::shape::validation::{ShapeValidationInput, validate_ast};
use crate::compiler::analyze::types::check_entrypoints;
use crate::compiler::analyze::types::type_check::{self, Arity, TypeAnalysis};
use crate::compiler::emit::tables::EmitError;
use crate::compiler::lower::{LowerInput, lower_to_nfa};
use crate::compiler::parse::{
    DEFAULT_FUEL, DEFAULT_MAX_DEPTH, ParseConfig, Parser, Root, SyntaxNode, lex,
};
use crate::core::Interner;
use crate::core::grammar::Grammar;

use crate::bytecode::Module;
use crate::compiler::Diagnostics;
use crate::compiler::diagnostics::{DiagnosticKind, Span};
use crate::compiler::source::{SourceId, SourceMap};

pub(crate) type AstMap = IndexMap<SourceId, Root>;

struct QueryConfig {
    pub parse_fuel: u32,
    pub parse_max_depth: u32,
    pub satisfy_step_budget: u64,
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
            satisfy_step_budget: DEFAULT_SATISFY_STEP_BUDGET,
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

    /// Override the satisfiability solve's work ceiling. Raise it for a query that
    /// legitimately needs a wide child list the default rejects as too complex; the
    /// default protects against an adversarial one driving the quadratic solve for an
    /// unbounded stretch.
    pub fn with_satisfy_step_budget(mut self, budget: u64) -> Self {
        self.config.satisfy_step_budget = budget;
        self
    }

    pub fn analyze(self) -> crate::compiler::QueryResult<Query> {
        self.parse()?.analyze()
    }

    pub fn check(self, grammar: &Grammar) -> crate::compiler::QueryResult<CheckedQuery> {
        Ok(self.link(grammar)?.check())
    }

    pub fn compile(self, grammar: &Grammar) -> crate::compiler::QueryResult<CompiledQuery> {
        Ok(self.link(grammar)?.compile_module())
    }

    pub(crate) fn link(self, grammar: &Grammar) -> crate::compiler::QueryResult<LinkOutcome> {
        Ok(self.analyze()?.link(grammar))
    }

    pub(crate) fn parse(self) -> crate::compiler::QueryResult<QueryParsed> {
        let mut ast = IndexMap::new();
        let mut diag = Diagnostics::new();

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
            ast.insert(source.id, res.into_ast());
        }

        Ok(QueryParsed {
            source_map: self.source_map,
            diag,
            ast_map: ast,
            max_depth: self.config.parse_max_depth,
            satisfy_step_budget: self.config.satisfy_step_budget,
        })
    }
}

#[derive(Debug)]
pub(crate) struct QueryParsed {
    source_map: SourceMap,
    ast_map: AstMap,
    diag: Diagnostics,
    /// The structural-depth ceiling parsing ran under, carried forward so the satisfiability
    /// check can bound automaton construction by the same budget the parser already enforced.
    max_depth: u32,
    /// The satisfiability solve's work ceiling, carried forward to that check.
    satisfy_step_budget: u64,
}

impl QueryParsed {
    pub(crate) fn analyze(mut self) -> crate::compiler::QueryResult<Query> {
        let Some(validated) = validate_ast(ShapeValidationInput {
            source_map: &self.source_map,
            ast_map: &self.ast_map,
            diag: &mut self.diag,
        }) else {
            return Ok(Query::parsed_only(self));
        };

        let mut interner = Interner::new();
        let symbol_table = resolve_names(&validated, &mut self.diag);

        // A reference chain deeper than the structural ceiling is rejected here (the
        // dependency graph's recursion would otherwise overflow), the same fatal
        // recursion-limit outcome the parser produces for over-deep nesting.
        let dependency_analysis =
            dependencies::analyze_dependencies(&symbol_table, &mut interner, self.max_depth)?;
        validate_recursion(
            &dependency_analysis,
            validated.ast_map(),
            &symbol_table,
            &interner,
            &mut self.diag,
        );

        let type_analysis = type_check::infer_types(
            &mut interner,
            &symbol_table,
            &dependency_analysis,
            &mut self.diag,
        );
        if !self.diag.has_errors() {
            check_entrypoints(
                validated.ast_map(),
                &interner,
                &type_analysis,
                &dependency_analysis,
                &mut self.diag,
            );
        }

        let analysis = Analysis {
            interner,
            symbol_table,
            type_analysis,
            dependency_analysis,
        };

        Ok(Query::analyzed(self, analysis))
    }

    pub(crate) fn source_map(&self) -> &SourceMap {
        &self.source_map
    }

    pub(crate) fn diagnostics(&self) -> &Diagnostics {
        &self.diag
    }

    pub(crate) fn ast_map(&self) -> &AstMap {
        &self.ast_map
    }

    pub(crate) fn definition_names(&self) -> impl Iterator<Item = String> + '_ {
        self.ast_map
            .values()
            .flat_map(|root| root.defs())
            .filter_map(|def| def.name())
            .map(|name| name.text().to_string())
    }
}

pub struct Query {
    parsed: QueryParsed,
    analysis: Option<Analysis>,
}

pub(crate) struct AnalyzedQuery {
    parsed: QueryParsed,
    analysis: Analysis,
}

pub(super) struct Analysis {
    pub(super) interner: Interner,
    pub(super) symbol_table: SymbolTable,
    pub(super) type_analysis: TypeAnalysis,
    pub(super) dependency_analysis: dependencies::DependencyAnalysis,
}

impl Query {
    fn analyzed(parsed: QueryParsed, analysis: Analysis) -> Self {
        Self {
            parsed,
            analysis: Some(analysis),
        }
    }

    fn parsed_only(parsed: QueryParsed) -> Self {
        Self {
            parsed,
            analysis: None,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.analysis.is_some() && !self.parsed.diag.has_errors()
    }

    pub(crate) fn arity(&self, node: &SyntaxNode) -> Option<Arity> {
        let analysis = self.analysis.as_ref()?;

        use crate::compiler::parse::ast;

        if let Some(pattern) = ast::Pattern::cast(node.clone()) {
            return analysis.type_analysis.arity(&pattern);
        }

        if let Some(root) = ast::Root::cast(node.clone()) {
            return Some(if root.defs().nth(1).is_some() {
                Arity::Many
            } else {
                Arity::One
            });
        }

        if let Some(def) = ast::Def::cast(node.clone()) {
            return def.body().and_then(|b| analysis.type_analysis.arity(&b));
        }

        if let Some(branch) = ast::Branch::cast(node.clone()) {
            return branch.body().and_then(|b| analysis.type_analysis.arity(&b));
        }

        None
    }

    pub(super) fn analysis(&self) -> Option<&Analysis> {
        self.analysis.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn symbol_table(&self) -> &SymbolTable {
        &self
            .analysis
            .as_ref()
            .expect("test query must be valid before inspecting symbols")
            .symbol_table
    }

    pub fn source_map(&self) -> &SourceMap {
        self.parsed.source_map()
    }

    pub fn diagnostics(&self) -> &Diagnostics {
        self.parsed.diagnostics()
    }

    pub(crate) fn ast_map(&self) -> &AstMap {
        self.parsed.ast_map()
    }

    pub fn definition_names(&self) -> impl Iterator<Item = String> + '_ {
        self.parsed.definition_names()
    }

    #[allow(clippy::result_large_err)]
    fn into_analyzed(self) -> Result<AnalyzedQuery, Query> {
        if self.parsed.diag.has_errors() {
            return Err(self);
        }

        let Query { parsed, analysis } = self;
        match analysis {
            Some(analysis) => Ok(AnalyzedQuery { parsed, analysis }),
            None => Err(Query::parsed_only(parsed)),
        }
    }

    pub(crate) fn link(self, grammar: &Grammar) -> LinkOutcome {
        let mut analyzed = match self.into_analyzed() {
            Ok(analyzed) => analyzed,
            Err(query) => return LinkOutcome::Invalid(query),
        };

        let mut output = GrammarBindingBuilder::new();
        link::GrammarLinkInput {
            interner: &mut analyzed.analysis.interner,
            grammar,
            source_map: &analyzed.parsed.source_map,
            ast_map: &analyzed.parsed.ast_map,
            symbol_table: &analyzed.analysis.symbol_table,
            max_depth: analyzed.parsed.max_depth,
            satisfy_step_budget: analyzed.parsed.satisfy_step_budget,
        }
        .link(&mut output, &mut analyzed.parsed.diag);

        if analyzed.parsed.diag.has_errors() {
            return LinkOutcome::Invalid(analyzed.into_query());
        }

        LinkOutcome::Linked(LinkedQuery {
            analyzed,
            grammar: output.finish(),
        })
    }
}

impl AnalyzedQuery {
    fn into_query(self) -> Query {
        Query::analyzed(self.parsed, self.analysis)
    }

    pub(crate) fn interner(&self) -> &Interner {
        &self.analysis.interner
    }

    pub(crate) fn type_analysis(&self) -> &TypeAnalysis {
        &self.analysis.type_analysis
    }

    pub(crate) fn symbol_table(&self) -> &SymbolTable {
        &self.analysis.symbol_table
    }

    pub(crate) fn dependency_analysis(&self) -> &dependencies::DependencyAnalysis {
        &self.analysis.dependency_analysis
    }

    pub fn source_map(&self) -> &SourceMap {
        self.parsed.source_map()
    }

    pub fn diagnostics(&self) -> &Diagnostics {
        self.parsed.diagnostics()
    }

    pub fn definition_names(&self) -> impl Iterator<Item = String> + '_ {
        self.parsed.definition_names()
    }
}

impl TryFrom<&str> for Query {
    type Error = crate::compiler::Error;

    fn try_from(src: &str) -> crate::compiler::QueryResult<Self> {
        QueryBuilder::new(SourceMap::from_inline(src)).analyze()
    }
}

pub(crate) enum LinkOutcome {
    Linked(LinkedQuery),
    Invalid(Query),
}

pub(crate) struct LinkedQuery {
    analyzed: AnalyzedQuery,
    grammar: GrammarBinding,
}

pub struct CheckedQuery {
    query: LinkOutcome,
    diagnostics: Diagnostics,
}

impl CheckedQuery {
    fn new(query: LinkOutcome, diagnostics: Diagnostics) -> Self {
        Self { query, diagnostics }
    }

    pub fn is_valid(&self) -> bool {
        !self.diagnostics.has_errors()
    }

    pub fn source_map(&self) -> &SourceMap {
        self.query.source_map()
    }

    pub fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    pub fn definition_names(&self) -> impl Iterator<Item = String> + '_ {
        self.query.definition_names()
    }
}

pub struct CompiledQuery {
    checked: CheckedQuery,
    compiled: Option<CompiledArtifact>,
}

struct CompiledArtifact {
    bytecode: Vec<u8>,
    module: Module,
}

impl CompiledQuery {
    fn failed(query: LinkOutcome, diagnostics: Diagnostics) -> Self {
        Self {
            checked: CheckedQuery::new(query, diagnostics),
            compiled: None,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.compiled.is_some() && self.checked.is_valid()
    }

    pub fn source_map(&self) -> &SourceMap {
        self.checked.source_map()
    }

    pub fn diagnostics(&self) -> &Diagnostics {
        self.checked.diagnostics()
    }

    pub fn definition_names(&self) -> impl Iterator<Item = String> + '_ {
        self.checked.definition_names()
    }

    pub fn bytecode(&self) -> Option<&[u8]> {
        self.compiled
            .as_ref()
            .map(|compiled| compiled.bytecode.as_slice())
    }

    pub fn module(&self) -> Option<&Module> {
        self.compiled.as_ref().map(|compiled| &compiled.module)
    }

    pub fn into_module(self) -> Option<Module> {
        self.compiled.map(|compiled| compiled.module)
    }

    pub fn to_typescript(
        &self,
        config: crate::compiler::typegen::typescript::Config,
    ) -> Option<String> {
        self.module()
            .map(|module| crate::compiler::typegen::typescript::emit(module, config))
    }
}

impl LinkOutcome {
    #[cfg(test)]
    pub fn is_valid(&self) -> bool {
        matches!(self, LinkOutcome::Linked(_))
    }

    #[cfg(test)]
    pub(crate) fn interner(&self) -> &Interner {
        self.expect_linked().interner()
    }

    pub fn source_map(&self) -> &SourceMap {
        match self {
            LinkOutcome::Linked(query) => query.source_map(),
            LinkOutcome::Invalid(query) => query.source_map(),
        }
    }

    pub fn diagnostics(&self) -> &Diagnostics {
        match self {
            LinkOutcome::Linked(query) => query.diagnostics(),
            LinkOutcome::Invalid(query) => query.diagnostics(),
        }
    }

    pub fn definition_names(&self) -> impl Iterator<Item = String> + '_ {
        self.definition_names_vec().into_iter()
    }

    #[cfg(test)]
    pub(crate) fn grammar(&self) -> &GrammarBinding {
        self.expect_linked().grammar()
    }

    pub(crate) fn check(self) -> CheckedQuery {
        let diagnostics = self.dry_run();
        CheckedQuery::new(self, diagnostics)
    }

    pub(crate) fn compile_module(self) -> CompiledQuery {
        let mut diagnostics = self.dry_run();

        if diagnostics.has_errors() {
            return CompiledQuery::failed(self, diagnostics);
        }

        let bytes = match self.emit() {
            Ok(bytes) => bytes,
            Err(err) => {
                if let Some((source, range)) = self.fallback_span() {
                    diagnostics
                        .report(DiagnosticKind::EmitFailed, Span::new(source, range))
                        .detail(err.to_string())
                        .emit();
                }

                return CompiledQuery::failed(self, diagnostics);
            }
        };

        let module = match Module::load(&bytes) {
            Ok(loaded) => loaded,
            Err(err) => {
                if let Some((source, range)) = self.fallback_span() {
                    diagnostics
                        .report(DiagnosticKind::BytecodeRejected, Span::new(source, range))
                        .detail(err.to_string())
                        .emit();
                }

                return CompiledQuery::failed(self, diagnostics);
            }
        };

        CompiledQuery {
            checked: CheckedQuery::new(self, diagnostics),
            compiled: Some(CompiledArtifact {
                bytecode: bytes,
                module,
            }),
        }
    }

    /// Emit bytecode. Returns `Err(EmitError::InvalidQuery)` if the query has errors.
    pub(in crate::compiler) fn emit(&self) -> Result<Vec<u8>, EmitError> {
        match self {
            LinkOutcome::Linked(query) => query.emit(),
            LinkOutcome::Invalid(_) => Err(EmitError::InvalidQuery),
        }
    }

    /// Emit without the emitter's debug load self-check.
    ///
    /// `dry_run` loads the bytecode itself so malformed output is reported
    /// as a diagnostic instead of reaching the debug self-check panic.
    fn emit_unchecked(&self) -> Result<Vec<u8>, EmitError> {
        match self {
            LinkOutcome::Linked(query) => query.emit_unchecked(),
            LinkOutcome::Invalid(_) => Err(EmitError::InvalidQuery),
        }
    }

    /// Full-pipeline dry run for `check`: emit bytecode and load it, reporting any
    /// failure as a diagnostic instead of panicking. Returns the analyze/link
    /// diagnostics plus any emit/load failure; the caller inspects `has_errors()`.
    ///
    /// Loads the bytecode itself, so it never reaches the emitter's debug
    /// self-check panic in debug or release.
    pub(crate) fn dry_run(&self) -> Diagnostics {
        let Some(query) = self.linked() else {
            return self.diagnostics().clone();
        };

        let mut diag = query.diagnostics().clone();
        if diag.has_errors() {
            return diag;
        }

        let bytes = match self.emit_unchecked() {
            Ok(bytes) => bytes,
            Err(err) => {
                if let Some((source, range)) = self.fallback_span() {
                    diag.report(DiagnosticKind::EmitFailed, Span::new(source, range))
                        .detail(err.to_string())
                        .emit();
                }
                return diag;
            }
        };

        match crate::bytecode::Module::load(&bytes) {
            Ok(_) => {}
            Err(err) => {
                if let Some((source, range)) = self.fallback_span() {
                    diag.report(DiagnosticKind::BytecodeRejected, Span::new(source, range))
                        .detail(err.to_string())
                        .emit();
                }
            }
        }

        diag
    }

    fn linked(&self) -> Option<&LinkedQuery> {
        match self {
            LinkOutcome::Linked(query) => Some(query),
            LinkOutcome::Invalid(_) => None,
        }
    }

    #[cfg(test)]
    fn expect_linked(&self) -> &LinkedQuery {
        self.linked()
            .expect("linked query data is only available after link succeeds")
    }

    fn definition_names_vec(&self) -> Vec<String> {
        match self {
            LinkOutcome::Linked(query) => query.definition_names().collect(),
            LinkOutcome::Invalid(query) => query.definition_names().collect(),
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

impl LinkedQuery {
    pub(crate) fn interner(&self) -> &Interner {
        self.analyzed.interner()
    }

    pub(crate) fn type_analysis(&self) -> &TypeAnalysis {
        self.analyzed.type_analysis()
    }

    pub(crate) fn symbol_table(&self) -> &SymbolTable {
        self.analyzed.symbol_table()
    }

    pub(crate) fn dependency_analysis(&self) -> &dependencies::DependencyAnalysis {
        self.analyzed.dependency_analysis()
    }

    pub fn source_map(&self) -> &SourceMap {
        self.analyzed.source_map()
    }

    pub fn diagnostics(&self) -> &Diagnostics {
        self.analyzed.diagnostics()
    }

    pub fn definition_names(&self) -> impl Iterator<Item = String> + '_ {
        self.analyzed.definition_names()
    }

    pub(crate) fn grammar(&self) -> &GrammarBinding {
        &self.grammar
    }

    fn emit(&self) -> Result<Vec<u8>, EmitError> {
        let nfa = self.compile();
        crate::compiler::emit::emit(self.analysis_input(), &nfa)
    }

    fn emit_unchecked(&self) -> Result<Vec<u8>, EmitError> {
        let nfa = self.compile();
        crate::compiler::emit::emit_unchecked(self.analysis_input(), &nfa)
    }

    fn compile(&self) -> crate::compiler::lower::ir::LoweredNfa {
        lower_to_nfa(LowerInput {
            analysis: self.analysis_input(),
            symbol_table: self.symbol_table(),
        })
    }

    fn analysis_input(&self) -> AnalysisArtifacts<'_> {
        AnalysisArtifacts {
            interner: self.interner(),
            type_analysis: self.type_analysis(),
            dependency_analysis: self.dependency_analysis(),
            grammar: self.grammar(),
        }
    }
}
