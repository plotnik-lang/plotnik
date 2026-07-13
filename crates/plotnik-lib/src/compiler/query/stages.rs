use indexmap::IndexMap;
use rowan::TextRange;

use crate::compiler::analyze::AnalysisArtifacts;
use crate::compiler::analyze::grammar::bind;
use crate::compiler::analyze::grammar::{GrammarBinding, GrammarBindingBuilder};
use crate::compiler::analyze::names::{SymbolTable, resolve_names};
use crate::compiler::analyze::output::OutputSchema;
use crate::compiler::analyze::refs::{dependencies, validate_recursion};
use crate::compiler::analyze::shape::validation::{ShapeValidationInput, validate_ast};
use crate::compiler::analyze::types::check_entrypoints;
use crate::compiler::analyze::types::type_check::{self, RootExtent, TypeAnalysis};
use crate::compiler::emit::targets::bytecode::tables::EmitError;
use crate::compiler::emit::{
    BytecodeConfig, CodegenProvenance, Emission, EmitTarget, RustCodegenConfig, RustModuleOutput,
    RustTypesOutput, TypeScriptCodegenConfig, TypeScriptTypesOutput,
};
use crate::compiler::limits::CompilerLimits;
use crate::compiler::lower::ir::SemanticNfa;
use crate::compiler::lower::semantic_verify;
use crate::compiler::lower::spans::assign_spans;
use crate::compiler::lower::{LowerInput, lower_semantic, pack_lowered};
use crate::compiler::parse::{Root, SyntaxNode, parse_lossless};
use crate::core::grammar::Grammar;
use crate::core::{Colors, Interner};

use crate::bytecode::Module;
use crate::compiler::Diagnostics;
use crate::compiler::diagnostics::{DiagnosticKind, Span};
use crate::compiler::source::{SourceId, SourceMap};

pub(crate) type AstMap = IndexMap<SourceId, Root>;

pub struct QueryBuilder {
    source_map: SourceMap,
    limits: CompilerLimits,
    strict_lints: bool,
}

impl QueryBuilder {
    pub fn new(source_map: SourceMap) -> Self {
        Self {
            source_map,
            limits: CompilerLimits::default(),
            strict_lints: false,
        }
    }

    pub fn from_inline(src: &str) -> Self {
        let source_map = SourceMap::from_inline(src);
        Self::new(source_map)
    }

    pub fn with_parse_fuel(mut self, fuel: u32) -> Self {
        self.limits = self.limits.with_parse_fuel(fuel);
        self
    }

    /// Override the parser's source-nesting ceiling.
    pub fn with_parse_max_depth(mut self, limit: u32) -> Self {
        self.limits = self.limits.with_parse_max_depth(limit);
        self
    }

    /// Override the reference graph's recursion ceiling.
    pub fn with_reference_max_depth(mut self, limit: u32) -> Self {
        self.limits = self.limits.with_reference_max_depth(limit);
        self
    }

    /// Override the satisfiability automaton's inlined-pattern nesting ceiling.
    pub fn with_satisfiability_automaton_max_depth(mut self, limit: u32) -> Self {
        self.limits = self.limits.with_satisfiability_automaton_max_depth(limit);
        self
    }

    /// Override the satisfiability solve's work ceiling. Raise it for a query that
    /// legitimately needs a wide child list the default rejects as too complex; the
    /// default protects against an adversarial one driving the quadratic solve for an
    /// unbounded stretch.
    pub fn with_satisfiability_work_budget(mut self, budget: u64) -> Self {
        self.limits = self.limits.with_satisfiability_work_budget(budget);
        self
    }

    /// Enable stricter advisory lints that are too noisy for normal compilation.
    pub fn with_strict_lints(mut self, enabled: bool) -> Self {
        self.strict_lints = enabled;
        self
    }

    pub fn analyze(self) -> crate::compiler::QueryResult<Query> {
        self.parse()?.analyze()
    }

    pub fn compile(self, grammar: &Grammar) -> crate::compiler::QueryResult<CompiledQuery> {
        self.bind(grammar)?.compile()
    }

    pub(crate) fn bind(self, grammar: &Grammar) -> crate::compiler::QueryResult<BindOutcome> {
        Ok(self.analyze()?.bind(grammar))
    }

    pub(crate) fn parse(self) -> crate::compiler::QueryResult<QueryParsed> {
        let mut ast = IndexMap::new();
        let mut diag = Diagnostics::new();

        for source in self.source_map.iter() {
            let root = parse_lossless(
                source.content,
                source.id,
                &mut diag,
                self.limits.parse().config(),
            )?;
            ast.insert(source.id, root);
        }

        Ok(QueryParsed {
            source_map: self.source_map,
            diag,
            ast_map: ast,
            limits: self.limits,
            strict_lints: self.strict_lints,
        })
    }
}

#[derive(Debug)]
pub(crate) struct QueryParsed {
    source_map: SourceMap,
    ast_map: AstMap,
    diag: Diagnostics,
    limits: CompilerLimits,
    strict_lints: bool,
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

        // A flat reference chain can recurse as deeply as a nested source tree, so it gets
        // an explicit stack-depth ceiling and the same fatal recursion-limit outcome.
        let dependency_analysis = dependencies::analyze_dependencies(
            &symbol_table,
            &mut interner,
            self.limits.references(),
        )?;
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

    pub(crate) fn root_extent(&self, node: &SyntaxNode) -> Option<RootExtent> {
        let analysis = self.analysis.as_ref()?;

        use crate::compiler::parse::ast;

        if let Some(pattern) = ast::Pattern::cast(node.clone()) {
            return analysis.type_analysis.root_extent(&pattern);
        }

        if let Some(def) = ast::Def::cast(node.clone()) {
            return def
                .body()
                .and_then(|body| analysis.type_analysis.root_extent(&body));
        }

        if let Some(alternative) = ast::Alternative::cast(node.clone()) {
            return alternative
                .body()
                .and_then(|body| analysis.type_analysis.root_extent(&body));
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

    pub(crate) fn bind(self, grammar: &Grammar) -> BindOutcome {
        let mut analyzed = match self.into_analyzed() {
            Ok(analyzed) => analyzed,
            Err(query) => return BindOutcome::Invalid(Box::new(query)),
        };

        let mut output = GrammarBindingBuilder::new();
        output.identity(grammar.identity().cloned());
        bind::GrammarBindInput {
            interner: &mut analyzed.analysis.interner,
            grammar,
            source_map: &analyzed.parsed.source_map,
            ast_map: &analyzed.parsed.ast_map,
            symbol_table: &analyzed.analysis.symbol_table,
            dependency_analysis: &analyzed.analysis.dependency_analysis,
            strict_lints: analyzed.parsed.strict_lints,
            satisfiability_limits: analyzed.parsed.limits.satisfiability(),
        }
        .bind(&mut output, &mut analyzed.parsed.diag);

        if analyzed.parsed.diag.has_errors() {
            return BindOutcome::Invalid(Box::new(analyzed.into_query()));
        }

        BindOutcome::Bound(Box::new(BoundQuery {
            analyzed,
            grammar: output.finish(),
        }))
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

pub(crate) enum BindOutcome {
    Bound(Box<BoundQuery>),
    Invalid(Box<Query>),
}

pub(crate) struct BoundQuery {
    analyzed: AnalyzedQuery,
    grammar: GrammarBinding,
}

pub struct CompiledQuery {
    bound: BindOutcome,
    semantic_nfa: Option<SemanticNfa>,
    diagnostics: Diagnostics,
}

impl CompiledQuery {
    pub fn is_valid(&self) -> bool {
        self.semantic_nfa.is_some() && !self.diagnostics.has_errors()
    }

    pub fn emit<T: EmitTarget>(
        &self,
        target: T,
    ) -> crate::compiler::QueryResult<Emission<T::Output>> {
        target.emit(self)
    }

    pub fn emit_types<T: crate::compiler::emit::CodegenTarget>(
        &self,
        target: T,
    ) -> crate::compiler::QueryResult<Emission<T::TypesOutput>> {
        target.emit_types(self)
    }

    pub(crate) fn emit_bytecode(
        &self,
        config: &BytecodeConfig,
    ) -> crate::compiler::QueryResult<Emission<Module>> {
        if !self.is_valid() {
            return Ok(Emission::invalid_query());
        }
        let bound = self
            .bound
            .bound()
            .expect("valid compiled query is grammar-bound");
        let input = LowerInput {
            analysis: bound.analysis_input(),
            symbol_table: bound.symbol_table(),
            inspection: config.inspection_enabled(),
        };
        let mut diagnostics = Diagnostics::new();
        if config.inspection_enabled() {
            self.bound
                .report_inspection_span_degradation_for(&input, &mut diagnostics);
        }
        let lowered = if config.inspection_enabled() {
            pack_lowered(lower_semantic(&input), &input)
        } else {
            pack_lowered(
                self.semantic_nfa
                    .as_ref()
                    .expect("valid query retains semantic NFA")
                    .clone(),
                &input,
            )
        };
        let bytes = match crate::compiler::emit::targets::bytecode::emit(
            bound.analysis_input(),
            &lowered,
        ) {
            Ok(bytes) => bytes,
            Err(error) => {
                if error.is_target_limit() {
                    self.bound.report_target_error(&mut diagnostics, error);
                    return Ok(Emission::failure(diagnostics));
                }
                return Err(crate::compiler::Error::CompilerInvariantViolation(
                    error.to_string(),
                ));
            }
        };
        let module = Module::load_compiler_output(&bytes).map_err(|error| {
            crate::compiler::Error::CompilerInvariantViolation(format!(
                "bytecode target failed module validation: {error}"
            ))
        })?;
        Ok(Emission::success(module, diagnostics))
    }

    pub(crate) fn emit_rust_types(
        &self,
        config: &RustCodegenConfig,
    ) -> crate::compiler::QueryResult<Emission<RustTypesOutput>> {
        if !self.is_valid() {
            return Ok(Emission::invalid_query());
        }
        let bound = self
            .bound
            .bound()
            .expect("valid compiled query is grammar-bound");
        let source = crate::compiler::emit::targets::rust::emit_types(
            bound.type_analysis(),
            bound.dependency_analysis(),
            bound.interner(),
            &config.rust_types_config(),
        );
        Ok(Emission::success(
            RustTypesOutput::new(source),
            Diagnostics::new(),
        ))
    }

    pub(crate) fn emit_rust_module(
        &self,
        config: &RustCodegenConfig,
    ) -> crate::compiler::QueryResult<Emission<RustModuleOutput>> {
        if !self.is_valid() {
            return Ok(Emission::invalid_query());
        }
        let bound = self
            .bound
            .bound()
            .expect("valid compiled query is grammar-bound");
        let mut matcher = config.matcher_config();
        if config.provenance_mode() == CodegenProvenance::Full {
            let identity = bound.grammar().identity().cloned().ok_or_else(|| {
                crate::compiler::emit::EmitConfigError::new(
                    "full provenance requested, but the bound grammar has no artifact identity",
                )
            })?;
            matcher = matcher.grammar_identity(identity);
        }
        let plan = bound.codegen_plan(
            self.semantic_nfa
                .as_ref()
                .expect("valid query retains semantic NFA"),
        );
        let source = crate::compiler::emit::targets::rust::generate(&plan, &matcher);
        Ok(Emission::success(
            RustModuleOutput::new(source),
            Diagnostics::new(),
        ))
    }

    pub(crate) fn emit_typescript_types(
        &self,
        config: &TypeScriptCodegenConfig,
    ) -> crate::compiler::QueryResult<Emission<TypeScriptTypesOutput>> {
        if !self.is_valid() {
            return Ok(Emission::invalid_query());
        }
        let bound = self
            .bound
            .bound()
            .expect("valid compiled query is grammar-bound");
        let schema = OutputSchema::from_artifacts(bound.analysis_input())
            .expect("target-neutral compilation validated the output schema");
        let legacy = config.legacy_config();
        let (source, bindings) = if config.colored_output() {
            (
                crate::compiler::emit::targets::typescript::emit_schema(&schema, legacy),
                Vec::new(),
            )
        } else {
            crate::compiler::emit::targets::typescript::emit_schema_mapped(&schema, legacy)
        };
        Ok(Emission::success(
            TypeScriptTypesOutput::new(source, bindings),
            Diagnostics::new(),
        ))
    }

    pub fn source_map(&self) -> &SourceMap {
        self.bound.source_map()
    }

    pub fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    pub fn definition_names(&self) -> impl Iterator<Item = String> + '_ {
        self.bound.definition_names()
    }

    pub fn entrypoint_names(&self) -> impl Iterator<Item = String> + '_ {
        self.bound.bound().into_iter().flat_map(|bound| {
            bound
                .type_analysis()
                .iter_entry_point_outputs()
                .map(|(definition, _)| {
                    bound
                        .interner()
                        .resolve(bound.dependency_analysis().def_name_sym(definition))
                        .to_string()
                })
        })
    }

    /// Render the optimized pre-pack NFA — the IR every backend consumes — in
    /// the bytecode dump format (label space, with definition provenance).
    /// `None` when the query didn't compile, mirroring [`Self::module`].
    pub fn dump_nfa(&self, colors: Colors) -> Option<String> {
        let semantic = self.semantic_nfa.as_ref()?;
        let bound = self.bound.bound()?;
        Some(crate::compiler::lower::dump::dump_nfa(
            semantic,
            bound.analysis_input(),
            colors,
        ))
    }
}

impl BindOutcome {
    #[cfg(test)]
    pub fn is_valid(&self) -> bool {
        matches!(self, BindOutcome::Bound(_))
    }

    #[cfg(test)]
    pub(crate) fn interner(&self) -> &Interner {
        self.expect_bound().interner()
    }

    pub fn source_map(&self) -> &SourceMap {
        match self {
            BindOutcome::Bound(query) => query.source_map(),
            BindOutcome::Invalid(query) => query.source_map(),
        }
    }

    pub fn diagnostics(&self) -> &Diagnostics {
        match self {
            BindOutcome::Bound(query) => query.diagnostics(),
            BindOutcome::Invalid(query) => query.diagnostics(),
        }
    }

    pub fn definition_names(&self) -> impl Iterator<Item = String> + '_ {
        self.definition_names_vec().into_iter()
    }

    #[cfg(test)]
    pub(crate) fn grammar(&self) -> &GrammarBinding {
        self.expect_bound().grammar()
    }

    #[cfg(test)]
    pub(in crate::compiler) fn emit_bytecode_for_test(&self) -> Result<Vec<u8>, EmitError> {
        let bound = self
            .bound()
            .expect("test bytecode emission requires a grammar-bound query");
        let input = LowerInput {
            analysis: bound.analysis_input(),
            symbol_table: bound.symbol_table(),
            inspection: false,
        };
        let lowered = pack_lowered(lower_semantic(&input), &input);
        crate::compiler::emit::targets::bytecode::emit(bound.analysis_input(), &lowered)
    }

    pub(crate) fn compile(self) -> crate::compiler::QueryResult<CompiledQuery> {
        let mut diagnostics = self.diagnostics().clone();
        let Some(bound) = self.bound() else {
            return Ok(CompiledQuery {
                bound: self,
                semantic_nfa: None,
                diagnostics,
            });
        };
        if diagnostics.has_errors() {
            return Ok(CompiledQuery {
                bound: self,
                semantic_nfa: None,
                diagnostics,
            });
        }

        let schema = match OutputSchema::from_artifacts(bound.analysis_input()) {
            Ok(schema) => schema,
            Err(error) => {
                self.report_shared_limit_error(&mut diagnostics, error.to_string());
                return Ok(CompiledQuery {
                    bound: self,
                    semantic_nfa: None,
                    diagnostics,
                });
            }
        };
        let input = LowerInput {
            analysis: bound.analysis_input(),
            symbol_table: bound.symbol_table(),
            inspection: false,
        };
        let semantic_nfa = lower_semantic(&input);
        if let Err(error) = semantic_verify::verify(&semantic_nfa, &schema) {
            debug_assert!(false, "semantic NFA verification failed: {error}");
            return Err(crate::compiler::Error::CompilerInvariantViolation(
                error.to_string(),
            ));
        }
        Ok(CompiledQuery {
            bound: self,
            semantic_nfa: Some(semantic_nfa),
            diagnostics,
        })
    }

    fn bound(&self) -> Option<&BoundQuery> {
        match self {
            BindOutcome::Bound(query) => Some(query),
            BindOutcome::Invalid(_) => None,
        }
    }

    #[cfg(test)]
    fn expect_bound(&self) -> &BoundQuery {
        self.bound()
            .expect("grammar-bound query data is only available after binding succeeds")
    }

    fn definition_names_vec(&self) -> Vec<String> {
        match self {
            BindOutcome::Bound(query) => query.definition_names().collect(),
            BindOutcome::Invalid(query) => query.definition_names().collect(),
        }
    }

    /// A coarse fallback span for emit/load failures, none of which carry a
    /// source mapping. Points at the whole first source; the diagnostic's detail
    /// carries the specifics. `None` when the query has no sources at all, so the
    /// operation remains total even on an empty source map.
    fn fallback_span(&self) -> Option<(SourceId, TextRange)> {
        let source = self.source_map().iter().next()?;
        let len = u32::try_from(source.content.len()).unwrap_or(u32::MAX);
        Some((source.id, TextRange::up_to(len.into())))
    }

    fn report_inspection_span_degradation_for(
        &self,
        input: &LowerInput<'_>,
        diagnostics: &mut Diagnostics,
    ) {
        let assignment = assign_spans(input);
        if assignment.dropped_tiers.is_empty() {
            return;
        }
        let (source, range) = assignment
            .first_dropped
            .expect("dropped span tier must have a first construct");
        diagnostics
            .report(
                DiagnosticKind::InspectionSpansDegraded,
                Span::new(source, range),
            )
            .detail(format!(
                "inspection spans degraded: dropped {} detail",
                dropped_tier_names(&assignment.dropped_tiers)
            ))
            .emit();
    }

    fn report_target_error(&self, diagnostics: &mut Diagnostics, error: EmitError) {
        if let Some((source, range)) = self.fallback_span() {
            diagnostics
                .report(
                    DiagnosticKind::TargetLimitExceeded,
                    Span::new(source, range),
                )
                .detail(error.to_string())
                .emit();
        }
    }

    fn report_shared_limit_error(&self, diagnostics: &mut Diagnostics, error: String) {
        if let Some((source, range)) = self.fallback_span() {
            diagnostics
                .report(DiagnosticKind::QueryTooComplex, Span::new(source, range))
                .detail(error)
                .emit();
        }
    }
}

fn dropped_tier_names(tiers: &[u8]) -> String {
    let names: Vec<_> = tiers
        .iter()
        .map(|tier| match tier {
            0 => "definition",
            1 => "capture",
            2 => "pattern/reference",
            3 => "structure",
            4 => "field/capture type",
            _ => "reserved",
        })
        .collect();
    names.join(", ")
}

impl BoundQuery {
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

    fn codegen_plan(&self, semantic: &SemanticNfa) -> crate::compiler::emit::CodegenPlan<'_> {
        crate::compiler::emit::CodegenPlan::build(semantic.raw(), self.analysis_input())
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
