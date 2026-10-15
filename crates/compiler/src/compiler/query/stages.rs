use indexmap::IndexMap;
use rowan::TextRange;

use crate::compiler::analyze::grammar::bind;
use crate::compiler::analyze::grammar::{GrammarBinding, GrammarBindingBuilder};
use crate::compiler::analyze::names::resolve_names;
use crate::compiler::analyze::refs::{DefinitionGraph, build_definition_graph, validate_recursion};
use crate::compiler::analyze::result::{ResultModel, ResultSchema};
use crate::compiler::analyze::shape::validation::{
    AnchorValidationInput, ShapeValidationInput, validate_anchors, validate_ast,
};
use crate::compiler::analyze::shape::{PatternFacts, RootExtent};
use crate::compiler::analyze::types::check_entrypoints;
use crate::compiler::analyze::types::type_check::{self, TypeAnalysis};
use crate::compiler::analyze::{AnalysisArtifacts, Located};
#[cfg(test)]
use crate::compiler::emit::targets::bytecode::tables::EmitError;
use crate::compiler::emit::{
    BytecodeConfig, CodegenProvenance, Emission, EmitTarget, RustCodegenConfig, RustModuleOutput,
    RustTypesOutput, TypeScriptCodegenConfig, TypeScriptTypesOutput,
};
use crate::compiler::lower::ir::SemanticNfa;
use crate::compiler::lower::spans::assign_spans;
use crate::compiler::lower::{LowerInput, lower_semantic, pack_lowered};
use crate::compiler::parse::{Parser, Root, lex, parse_lossless};
use crate::core::grammar::Grammar;
use crate::core::{Colors, Interner};

use crate::bytecode::Module;
use crate::compiler::Diagnostics;
use crate::compiler::diagnostics::{DiagnosticKind, Span};
use crate::compiler::source::{SourceId, SourceMap};

use super::input::{CompilationInput, CompilationUnit, SourceCollection};

pub(crate) type AstMap = IndexMap<SourceId, Root>;

pub struct QueryBuilder {
    input: CompilationInput,
}

impl QueryBuilder {
    pub fn new(source_map: SourceMap) -> Self {
        Self::from_sources(SourceCollection::explicit(source_map))
    }

    /// Compile registered module sources, grouping directory members only after
    /// the caller supplies a grammar. Stored non-module sources do not participate.
    pub fn from_sources(collection: SourceCollection) -> Self {
        Self {
            input: CompilationInput::new(collection),
        }
    }

    pub fn from_inline(src: &str) -> Self {
        let source_map = SourceMap::from_inline(src);
        Self::new(source_map)
    }

    /// Attach a separately stored expression. It can refer to named definitions,
    /// but is not itself part of their namespace.
    /// This source is parsed as an expression, not as a named module source.
    /// Source generation rejects a compiled query that contains an expression.
    pub fn with_expression(mut self, source: SourceId) -> Self {
        self.input.expression = Some(source);
        self
    }

    pub fn with_parse_fuel(mut self, fuel: u32) -> Self {
        self.input.limits = self.input.limits.with_parse_fuel(fuel);
        self
    }

    /// Override the parser's source-nesting ceiling.
    pub fn with_parse_max_depth(mut self, limit: u32) -> Self {
        self.input.limits = self.input.limits.with_parse_max_depth(limit);
        self
    }

    /// Override the reference graph's recursion ceiling.
    pub fn with_reference_max_depth(mut self, limit: u32) -> Self {
        self.input.limits = self.input.limits.with_reference_max_depth(limit);
        self
    }

    /// Override the satisfiability automaton's inlined-pattern nesting ceiling.
    pub fn with_satisfiability_automaton_max_depth(mut self, limit: u32) -> Self {
        self.input.limits = self
            .input
            .limits
            .with_satisfiability_automaton_max_depth(limit);
        self
    }

    /// Override the satisfiability solve's work ceiling. Raise it for a query that
    /// legitimately needs a wide child list the default rejects as too complex; the
    /// default protects against an adversarial one driving the quadratic solve for an
    /// unbounded stretch.
    pub fn with_satisfiability_work_budget(mut self, budget: u64) -> Self {
        self.input.limits = self.input.limits.with_satisfiability_work_budget(budget);
        self
    }

    /// Enable stricter advisory lints that are too noisy for normal compilation.
    pub fn with_strict_lints(mut self, enabled: bool) -> Self {
        self.input.strict_lints = enabled;
        self
    }

    /// Analyze query sources and the optional expression, validating tag metadata.
    pub fn analyze(self) -> crate::compiler::QueryResult<Query> {
        self.parse()?.analyze()
    }

    pub(crate) fn parse(self) -> crate::compiler::QueryResult<QueryParsed> {
        let (unit, diagnostics) = self.input.admit_static(None)?;
        unit.parse(diagnostics)
    }

    /// Compile against the caller's selected grammar, validating language tags.
    /// Tags group directory sources. They do not select or resolve a grammar.
    pub fn compile(self, grammar: &Grammar) -> crate::compiler::QueryResult<CompiledQuery> {
        self.bind(grammar)?.compile()
    }

    /// Compile dynamic query input using the caller's selected grammar.
    /// Invalid grouping tags are ignored, not query syntax or semantic errors.
    /// All eligible named entrypoints and the optional expression are retained.
    pub fn compile_dynamic(self, grammar: &Grammar) -> crate::compiler::QueryResult<CompiledQuery> {
        self.input
            .admit_execution(grammar.name())?
            .parse(Diagnostics::new())?
            .analyze()?
            .bind(grammar)
            .compile()
    }

    pub(crate) fn bind(self, grammar: &Grammar) -> crate::compiler::QueryResult<BindOutcome> {
        let (unit, diagnostics) = self.input.admit_static(Some(grammar.name()))?;
        Ok(unit.parse(diagnostics)?.analyze()?.bind(grammar))
    }
}

impl CompilationUnit {
    fn parse(self, mut diag: Diagnostics) -> crate::compiler::QueryResult<QueryParsed> {
        let mut ast = IndexMap::new();

        for &id in self.modules.keys() {
            let source = self.source_map.source(id);
            let root = parse_lossless(
                source.content,
                source.id,
                &mut diag,
                self.limits.parse().config(),
            )?;
            ast.insert(source.id, root);
        }

        let expression = if let Some(id) = self.expression {
            let source = self.source_map.source(id);
            let root = Parser::new(
                source.content,
                id,
                lex(source.content),
                &mut diag,
                self.limits.parse().config(),
            )
            .parse_expression()?;
            Some(Located::new(id, root))
        } else {
            None
        };

        Ok(QueryParsed {
            input: self,
            diag,
            ast_map: ast,
            expression,
        })
    }
}

#[derive(Debug)]
pub(crate) struct QueryParsed {
    input: CompilationUnit,
    ast_map: AstMap,
    diag: Diagnostics,
    expression: Option<Located<crate::compiler::parse::ast::Expression>>,
}

impl QueryParsed {
    pub(crate) fn analyze(mut self) -> crate::compiler::QueryResult<Query> {
        let Some(validated) = validate_ast(ShapeValidationInput {
            source_map: &self.input.source_map,
            ast_map: &self.ast_map,
            expression: self.expression.as_ref(),
            diag: &mut self.diag,
        }) else {
            return Ok(Query::parsed_only(self));
        };

        let mut interner = Interner::new();
        let collected_definitions = resolve_names(&validated, &mut self.diag);

        // A flat reference chain can recurse as deeply as a nested source tree, so it gets
        // an explicit stack-depth ceiling and the same fatal recursion-limit outcome.
        let mut definitions = build_definition_graph(
            collected_definitions,
            &mut interner,
            self.input.limits.references(),
        )?;
        let expression = validated.expression().cloned();
        if let Some(expression) = &expression {
            definitions.resolve_expression(expression.node(), &interner);
        }
        let pattern_facts = PatternFacts::analyze(&definitions, expression.as_ref());
        let anchors_valid = validate_anchors(AnchorValidationInput {
            pattern_facts: &pattern_facts,
            definitions: &definitions,
            diag: &mut self.diag,
            expression: expression.as_ref(),
        });
        if !anchors_valid {
            return Ok(Query::parsed_only(self));
        }
        validate_recursion(&definitions, &interner, &mut self.diag);

        if !self.diag.has_errors()
            && let Some(expression) = &expression
            && !pattern_facts.expression_is_entrypoint_eligible()
        {
            let message =
                if pattern_facts.pattern_root_extent(expression.node()) != RootExtent::SingleNode {
                    "the expression must match exactly one node"
                } else if pattern_facts.expression_requires_external_anchor_context() {
                    "anchor needs an enclosing node"
                } else {
                    "a field constraint needs an enclosing node"
                };
            self.diag
                .report(
                    DiagnosticKind::NoEntrypoints,
                    expression.span_of(expression.node().text_range()),
                )
                .detail(message)
                .emit();
            return Ok(Query::parsed_only(self));
        }

        let type_analysis = type_check::infer_types(
            &mut interner,
            &definitions,
            &pattern_facts,
            expression.as_ref(),
            &mut self.diag,
        );
        if !self.diag.has_errors() {
            check_entrypoints(validated.ast_map(), &interner, &definitions, &mut self.diag);
            if expression.is_none()
                && validated
                    .ast_map()
                    .values()
                    .all(|root| root.defs().next().is_none())
                && let Some((&id, root)) = validated.ast_map().first()
            {
                self.diag
                    .report(
                        DiagnosticKind::EmptyQuery,
                        Span::new(id, root.syntax().text_range()),
                    )
                    .emit();
            }
        }

        let analysis = Analysis {
            interner,
            definitions,
            pattern_facts,
            type_analysis,
            expression,
        };

        Ok(Query::analyzed(self, analysis))
    }

    pub(crate) fn source_map(&self) -> &SourceMap {
        &self.input.source_map
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
    pub(super) definitions: DefinitionGraph,
    pub(super) pattern_facts: PatternFacts,
    pub(super) type_analysis: TypeAnalysis,
    pub(super) expression: Option<Located<crate::compiler::parse::ast::Pattern>>,
}

impl Query {
    fn analyzed(parsed: QueryParsed, analysis: Analysis) -> Self {
        Self {
            parsed,
            analysis: Some(analysis),
        }
    }

    fn parsed_only(parsed: QueryParsed) -> Self {
        assert!(
            parsed.diag.has_errors(),
            "query analysis produced no artifacts and no diagnostic; an early analysis exit must \
             report the reason"
        );
        Self {
            parsed,
            analysis: None,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.analysis.is_some() && !self.parsed.diag.has_errors()
    }

    pub(super) fn analysis(&self) -> Option<&Analysis> {
        self.analysis.as_ref()
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
        let analysis = analysis.unwrap_or_else(|| {
            panic!(
                "query state is inconsistent: semantic analysis produced neither diagnostics nor \
                 analysis artifacts"
            )
        });
        Ok(AnalyzedQuery { parsed, analysis })
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
            source_map: &analyzed.parsed.input.source_map,
            definitions: &analyzed.analysis.definitions,
            pattern_facts: &analyzed.analysis.pattern_facts,
            strict_lints: analyzed.parsed.input.strict_lints,
            satisfiability_limits: analyzed.parsed.input.limits.satisfiability(),
            expression: analyzed.analysis.expression.as_ref(),
        }
        .bind(&mut output, &mut analyzed.parsed.diag);

        for (&id, source) in &analyzed.parsed.input.modules {
            if source.needs_grouping_hint() {
                analyzed.parsed.diag.add_grouping_hint(id);
            }
        }

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

    pub(crate) fn pattern_facts(&self) -> &PatternFacts {
        &self.analysis.pattern_facts
    }

    pub(crate) fn definitions(&self) -> &DefinitionGraph {
        &self.analysis.definitions
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
    result: Option<ResultModel>,
    semantic_nfa: Option<SemanticNfa>,
    diagnostics: Diagnostics,
}

impl CompiledQuery {
    pub fn is_valid(&self) -> bool {
        self.result.is_some() && self.semantic_nfa.is_some() && !self.diagnostics.has_errors()
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
        let schema = self.result_schema();
        let input = LowerInput {
            analysis: bound.analysis_input(),
            result: self.result_model(),
            inspection: config.inspection_enabled(),
        };
        let mut diagnostics = Diagnostics::new();
        let semantic = if input.inspection {
            self.bound
                .report_inspection_span_degradation_for(&input, &mut diagnostics);
            lower_semantic(&input)
        } else {
            self.semantic_nfa
                .as_ref()
                .expect("valid query retains semantic NFA")
                .clone()
        };
        let lowered = pack_lowered(semantic, &input);
        let bytes = match crate::compiler::emit::targets::bytecode::emit(
            bound.analysis_input(),
            &schema,
            &lowered,
        ) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.bound.report_target_error(&mut diagnostics, error);
                return Ok(Emission::failure(diagnostics));
            }
        };
        let module = Module::validate_and_load(&bytes).unwrap_or_else(|error| {
            panic!("compiler-emitted bytecode failed immediate module validation: {error}")
        });
        Ok(Emission::success(module, diagnostics))
    }

    pub(crate) fn emit_rust_types(
        &self,
        config: &RustCodegenConfig,
    ) -> crate::compiler::QueryResult<Emission<RustTypesOutput>> {
        if !self.is_valid() {
            return Ok(Emission::invalid_query());
        }
        self.require_static_module()?;
        let schema = self.result_schema();
        let source =
            crate::compiler::emit::targets::rust::emit_types(schema, &config.rust_types_config());
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
        self.require_static_module()?;
        let bound = self
            .bound
            .bound()
            .expect("valid compiled query is grammar-bound");
        let schema = self.result_schema();
        let mut matcher = config.matcher_config();
        if config.provenance_mode() == CodegenProvenance::Full {
            let identity = bound.grammar().identity().cloned().ok_or_else(|| {
                crate::compiler::emit::EmitConfigError::new(
                    "full provenance requested, but the bound grammar has no artifact identity",
                )
            })?;
            matcher = matcher.grammar_identity(identity);
        }
        let plan = match bound.codegen_plan(
            self.semantic_nfa
                .as_ref()
                .expect("valid query retains semantic NFA"),
            schema,
        ) {
            Ok(plan) => plan,
            Err(error) => {
                let mut diagnostics = Diagnostics::new();
                self.bound.report_target_error(&mut diagnostics, error);
                return Ok(Emission::failure(diagnostics));
            }
        };
        let source = match crate::compiler::emit::targets::rust::generate(&plan, &matcher) {
            Ok(source) => source,
            Err(error) => {
                let mut diagnostics = Diagnostics::new();
                self.bound.report_target_error(&mut diagnostics, error);
                return Ok(Emission::failure(diagnostics));
            }
        };
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
        self.require_static_module()?;
        let schema = self.result_schema();
        let emitter = config.emitter_config();
        let (source, bindings) = if config.colored_output() {
            (
                crate::compiler::emit::targets::typescript::emit_schema(&schema, emitter),
                Vec::new(),
            )
        } else {
            crate::compiler::emit::targets::typescript::emit_schema_mapped(&schema, emitter)
        };
        Ok(Emission::success(
            TypeScriptTypesOutput::new(source, bindings),
            Diagnostics::new(),
        ))
    }

    fn require_static_module(&self) -> crate::compiler::QueryResult<()> {
        let bound = self.bound.bound().expect("valid query is grammar-bound");
        let diagnostics = bound.analyzed.parsed.input.metadata_diagnostics();
        if diagnostics.has_errors() {
            return Err(crate::compiler::Error::QueryAnalyzeError(diagnostics));
        }

        if bound.analysis_input().expression.is_some() {
            return Err(crate::compiler::emit::EmitConfigError::new(
                "source generation does not accept an execution expression",
            )
            .into());
        }

        Ok(())
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
                .analysis_input()
                .iter_entrypoint_outputs()
                .map(|(definition, _)| {
                    bound
                        .interner()
                        .resolve(bound.definitions().definition(definition).name())
                        .to_string()
                })
        })
    }

    fn result_schema(&self) -> ResultSchema<'_> {
        let bound = self
            .bound
            .bound()
            .expect("valid compiled query is grammar-bound");
        self.result_model().schema(bound.analysis_input())
    }

    fn result_model(&self) -> &ResultModel {
        self.result
            .as_ref()
            .expect("valid compiled query retains its result model")
    }

    /// Render the optimized pre-pack NFA — the IR every backend consumes — in
    /// the bytecode dump format (label space, with definition provenance).
    /// `None` when the program did not compile.
    pub fn dump_nfa(&self, colors: Colors) -> Option<String> {
        let semantic = self.semantic_nfa.as_ref()?;
        let bound = self.bound.bound()?;
        let schema = self.result_schema();
        Some(crate::compiler::lower::dump::dump_nfa(
            semantic,
            bound.analysis_input(),
            schema.layout(),
            colors,
        ))
    }
}

impl BindOutcome {
    #[cfg(test)]
    pub fn is_valid(&self) -> bool {
        matches!(self, BindOutcome::Bound(_))
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
    pub(in crate::compiler) fn emit_bytecode_for_test(&self) -> Result<Vec<u8>, EmitError> {
        let bound = self
            .bound()
            .expect("test bytecode emission requires a grammar-bound query");
        let result = ResultModel::from_artifacts(bound.analysis_input())?;
        let input = LowerInput {
            analysis: bound.analysis_input(),
            result: &result,
            inspection: false,
        };
        let lowered = pack_lowered(lower_semantic(&input), &input);
        let schema = result.schema(bound.analysis_input());
        crate::compiler::emit::targets::bytecode::emit(bound.analysis_input(), &schema, &lowered)
    }

    pub(crate) fn compile(self) -> crate::compiler::QueryResult<CompiledQuery> {
        let mut diagnostics = self.diagnostics().clone();
        let Some(bound) = self.bound() else {
            return Ok(CompiledQuery {
                bound: self,
                result: None,
                semantic_nfa: None,
                diagnostics,
            });
        };
        assert!(
            !diagnostics.has_errors(),
            "grammar binding returned a bound query together with error diagnostics; failed \
             binding must not expose artifacts"
        );

        let result = match ResultModel::from_artifacts(bound.analysis_input()) {
            Ok(result) => result,
            Err(error) => {
                self.report_shared_limit_error(&mut diagnostics, error.to_string());
                return Ok(CompiledQuery {
                    bound: self,
                    result: None,
                    semantic_nfa: None,
                    diagnostics,
                });
            }
        };
        let input = LowerInput {
            analysis: bound.analysis_input(),
            result: &result,
            inspection: false,
        };
        let semantic_nfa = lower_semantic(&input);
        Ok(CompiledQuery {
            bound: self,
            result: Some(result),
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

    fn definition_names_vec(&self) -> Vec<String> {
        match self {
            BindOutcome::Bound(query) => query.definition_names().collect(),
            BindOutcome::Invalid(query) => query.definition_names().collect(),
        }
    }

    /// Emit failures without a precise location refer to participating syntax,
    /// never an excluded source that happens to be first in original storage.
    /// Prefer the execution expression, otherwise the first module syntax root.
    fn fallback_span(&self) -> Option<(SourceId, TextRange)> {
        let parsed = match self {
            Self::Bound(query) => &query.analyzed.parsed,
            Self::Invalid(query) => &query.parsed,
        };
        if let Some(expression) = &parsed.expression {
            return Some((expression.source(), expression.node().text_range()));
        }

        let (&id, root) = parsed.ast_map.first()?;
        Some((id, root.syntax().text_range()))
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

    fn report_target_error(&self, diagnostics: &mut Diagnostics, error: impl std::fmt::Display) {
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

    pub(crate) fn pattern_facts(&self) -> &PatternFacts {
        self.analyzed.pattern_facts()
    }

    pub(crate) fn definitions(&self) -> &DefinitionGraph {
        self.analyzed.definitions()
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

    fn codegen_plan<'a>(
        &'a self,
        semantic: &SemanticNfa,
        result: ResultSchema<'a>,
    ) -> Result<crate::compiler::emit::CodegenPlan<'a>, String> {
        crate::compiler::emit::CodegenPlan::build(semantic.raw(), self.analysis_input(), result)
    }

    fn analysis_input(&self) -> AnalysisArtifacts<'_> {
        AnalysisArtifacts {
            interner: self.interner(),
            type_analysis: self.type_analysis(),
            pattern_facts: self.pattern_facts(),
            definitions: self.definitions(),
            grammar: self.grammar(),
            expression: self.analyzed.analysis.expression.as_ref(),
        }
    }
}
