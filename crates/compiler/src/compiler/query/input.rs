use indexmap::IndexMap;

use crate::compiler::diagnostics::{
    DiagnosticKind, Diagnostics, Error, QueryResult, SourceId, SourceMap,
};
use crate::compiler::limits::CompilerLimits;

use super::{GrammarDeclarationError, parse_grammar_declaration};

/// Original source storage and the module candidates registered by the caller.
/// This collection performs no I/O or grammar resolution. The query builder can
/// assign a stored source the expression role instead of the module role.
#[derive(Clone, Debug)]
pub struct SourceCollection {
    storage: SourceMap,
    candidates: IndexMap<SourceId, ModuleSource>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Acquisition {
    Explicit,
    Directory,
}

#[derive(Clone, Debug)]
enum Tag {
    Absent,
    Valid { language: String },
    Malformed(GrammarDeclarationError),
}

#[derive(Clone, Debug)]
pub(super) struct ModuleSource {
    acquisition: Acquisition,
    tag: Tag,
}

impl SourceCollection {
    pub fn new(storage: SourceMap) -> Self {
        Self {
            storage,
            candidates: IndexMap::new(),
        }
    }

    /// Register an explicitly supplied module source, regardless of its tag.
    pub fn include(&mut self, source: SourceId) {
        self.register(source, Acquisition::Explicit);
    }

    /// Register a directory member for language grouping before body parsing.
    pub fn include_directory(&mut self, source: SourceId) {
        self.register(source, Acquisition::Directory);
    }

    pub fn source_map(&self) -> &SourceMap {
        &self.storage
    }

    /// Derive membership without removing candidates or changing source IDs.
    pub fn participating_sources<'a>(
        &'a self,
        language: &'a str,
    ) -> impl Iterator<Item = SourceId> + 'a {
        self.candidates
            .iter()
            .filter_map(move |(&id, source)| source.participates(Some(language)).then_some(id))
    }

    pub(super) fn explicit(storage: SourceMap) -> Self {
        let ids: Vec<_> = storage.iter().map(|source| source.id).collect();
        let mut collection = Self::new(storage);
        for id in ids {
            collection.include(id);
        }
        collection
    }

    fn register(&mut self, id: SourceId, acquisition: Acquisition) {
        let tag = match parse_grammar_declaration(self.storage.source(id)) {
            Ok(Some(tag)) => Tag::Valid {
                language: tag.language.to_owned(),
            },
            Ok(None) => Tag::Absent,
            Err(error) => Tag::Malformed(error),
        };
        assert!(
            self.candidates
                .insert(id, ModuleSource { acquisition, tag })
                .is_none(),
            "module source is registered once"
        );
    }
}

impl ModuleSource {
    fn participates(&self, language: Option<&str>) -> bool {
        match (&self.tag, self.acquisition, language) {
            (Tag::Valid { language: tag, .. }, Acquisition::Directory, Some(language)) => {
                tag == language
            }
            _ => true,
        }
    }

    pub(super) fn needs_grouping_hint(&self) -> bool {
        self.acquisition == Acquisition::Directory && !matches!(self.tag, Tag::Valid { .. })
    }
}

pub(super) struct CompilationInput {
    pub collection: SourceCollection,
    pub expression: Option<SourceId>,
    pub limits: CompilerLimits,
    pub strict_lints: bool,
}

#[derive(Debug)]
pub(super) struct CompilationUnit {
    pub source_map: SourceMap,
    pub modules: IndexMap<SourceId, ModuleSource>,
    pub expression: Option<SourceId>,
    pub limits: CompilerLimits,
    pub strict_lints: bool,
}

impl CompilationInput {
    pub fn new(collection: SourceCollection) -> Self {
        Self {
            collection,
            expression: None,
            limits: CompilerLimits::default(),
            strict_lints: false,
        }
    }

    pub fn admit_static(
        self,
        language: Option<&str>,
    ) -> QueryResult<(CompilationUnit, Diagnostics)> {
        let unit = self.collect(language)?;
        let diagnostics = unit.metadata_diagnostics();
        Ok((unit, diagnostics))
    }

    pub fn admit_execution(self, language: &str) -> QueryResult<CompilationUnit> {
        self.collect(Some(language))
    }

    fn collect(self, language: Option<&str>) -> QueryResult<CompilationUnit> {
        let expression = self.expression;
        if let Some(id) = expression {
            self.collection.storage.source(id);
        }

        let modules: IndexMap<_, _> = self
            .collection
            .candidates
            .into_iter()
            .filter(|(id, source)| Some(*id) != expression && source.participates(language))
            .collect();
        if modules.is_empty() && expression.is_none() {
            return Err(Error::NoQuerySources);
        }

        Ok(CompilationUnit {
            source_map: self.collection.storage,
            modules,
            expression,
            limits: self.limits,
            strict_lints: self.strict_lints,
        })
    }
}

impl CompilationUnit {
    pub(super) fn metadata_diagnostics(&self) -> Diagnostics {
        let mut diagnostics = Diagnostics::new();
        let module_errors = self
            .modules
            .values()
            .filter_map(|source| match &source.tag {
                Tag::Malformed(error) => Some(error.clone()),
                _ => None,
            });
        let expression_error = self
            .expression
            .and_then(|id| parse_grammar_declaration(self.source_map.source(id)).err());
        for error in module_errors.chain(expression_error) {
            diagnostics
                .report(DiagnosticKind::InvalidGrammarDeclaration, error.span)
                .hint(error.message)
                .emit();
        }
        diagnostics
    }
}
