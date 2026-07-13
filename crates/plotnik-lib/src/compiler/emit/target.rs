//! Public target-dispatched emission API.

use std::borrow::Cow;

use plotnik_rt::{Limit, RuntimeLimitSpec};

use crate::bytecode::Module;
use crate::compiler::diagnostics::{Diagnostics, QueryResult};
use crate::compiler::emit::targets::typescript::{MatchOnlyType, TypeScriptBinding};
use crate::compiler::query::CompiledQuery;
use crate::core::Colors;

/// The artifact and emission-local diagnostics produced by one target.
pub struct Emission<T> {
    artifact: Option<T>,
    diagnostics: Diagnostics,
}

impl<T> Emission<T> {
    pub(crate) fn success(artifact: T, diagnostics: Diagnostics) -> Self {
        Self {
            artifact: Some(artifact),
            diagnostics,
        }
    }

    pub(crate) fn failure(diagnostics: Diagnostics) -> Self {
        Self {
            artifact: None,
            diagnostics,
        }
    }

    pub(crate) fn invalid_query() -> Self {
        Self::failure(Diagnostics::new())
    }

    pub fn artifact(&self) -> Option<&T> {
        self.artifact.as_ref()
    }

    pub fn into_artifact(self) -> Option<T> {
        self.artifact
    }

    pub fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    pub fn is_valid(&self) -> bool {
        self.artifact.is_some() && !self.diagnostics.has_errors()
    }
}

mod private {
    pub trait Sealed {}
}

/// A configuration capable of emitting a complete target artifact.
pub trait EmitTarget: private::Sealed {
    type Output;

    #[doc(hidden)]
    fn emit(self, query: &CompiledQuery) -> QueryResult<Emission<Self::Output>>;
}

/// A source-target configuration capable of emitting output types.
pub trait CodegenTarget: private::Sealed {
    type TypesOutput;

    #[doc(hidden)]
    fn emit_types(self, query: &CompiledQuery) -> QueryResult<Emission<Self::TypesOutput>>;
}

/// Invalid target configuration. It deliberately carries no query span.
#[derive(Clone, Debug, thiserror::Error)]
#[error("{message}")]
pub struct EmitConfigError {
    message: String,
}

impl EmitConfigError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BytecodeInspection {
    #[default]
    None,
    Spans,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct BytecodeConfig {
    inspection: BytecodeInspection,
}

impl BytecodeConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn inspection(mut self, inspection: BytecodeInspection) -> Self {
        self.inspection = inspection;
        self
    }

    pub(crate) fn inspection_enabled(&self) -> bool {
        self.inspection == BytecodeInspection::Spans
    }
}

impl private::Sealed for BytecodeConfig {}

impl EmitTarget for BytecodeConfig {
    type Output = Module;

    fn emit(self, query: &CompiledQuery) -> QueryResult<Emission<Self::Output>> {
        query.emit_bytecode(&self)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CodegenProvenance {
    #[default]
    Omit,
    Full,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RustCodegenConfig {
    runtime_crate: Cow<'static, str>,
    serde: bool,
    limits: RuntimeLimitSpec,
    decode_depth: Limit,
    provenance: CodegenProvenance,
}

impl Default for RustCodegenConfig {
    fn default() -> Self {
        Self {
            runtime_crate: Cow::Borrowed("::plotnik_rt"),
            serde: false,
            limits: RuntimeLimitSpec {
                fuel_limit: Limit::Auto,
                memory: Limit::Auto,
            },
            decode_depth: Limit::Auto,
            provenance: CodegenProvenance::Omit,
        }
    }
}

impl RustCodegenConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn runtime_crate(mut self, path: impl Into<Cow<'static, str>>) -> Self {
        self.runtime_crate = path.into();
        self
    }

    pub fn serde(mut self, enabled: bool) -> Self {
        self.serde = enabled;
        self
    }

    pub fn limits(mut self, limits: RuntimeLimitSpec) -> Self {
        self.limits = limits;
        self
    }

    pub fn decode_depth(mut self, depth: Limit) -> Self {
        self.decode_depth = depth;
        self
    }

    pub fn provenance(mut self, provenance: CodegenProvenance) -> Self {
        self.provenance = provenance;
        self
    }

    pub(crate) fn validate(&self) -> Result<(), EmitConfigError> {
        let path = self.runtime_crate.as_ref();
        let relative = path.strip_prefix("::").unwrap_or(path);
        if relative.is_empty() || relative.split("::").any(|part| !valid_ident(part)) {
            return Err(EmitConfigError::new(format!(
                "invalid Rust runtime crate path `{path}`"
            )));
        }
        Ok(())
    }

    pub(crate) fn rust_types_config(&self) -> crate::compiler::emit::targets::rust::TypesConfig {
        crate::compiler::emit::targets::rust::TypesConfig::new()
            .rt_crate(self.runtime_crate.clone())
            .serde(self.serde)
    }

    pub(crate) fn matcher_config(&self) -> crate::compiler::emit::targets::rust::Config {
        crate::compiler::emit::targets::rust::Config::new()
            .rt_crate(self.runtime_crate.clone())
            .serde(self.serde)
            .limits(self.limits)
            .decode_depth(self.decode_depth)
    }

    pub(crate) fn provenance_mode(&self) -> CodegenProvenance {
        self.provenance
    }
}

fn valid_ident(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

impl private::Sealed for RustCodegenConfig {}

impl EmitTarget for RustCodegenConfig {
    type Output = RustModuleOutput;

    fn emit(self, query: &CompiledQuery) -> QueryResult<Emission<Self::Output>> {
        self.validate()?;
        query.emit_rust_module(&self)
    }
}

impl CodegenTarget for RustCodegenConfig {
    type TypesOutput = RustTypesOutput;

    fn emit_types(self, query: &CompiledQuery) -> QueryResult<Emission<Self::TypesOutput>> {
        self.validate()?;
        query.emit_rust_types(&self)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TypeScriptNodeRepresentation {
    #[default]
    SerializedValue,
    LiveTreeSitterNode,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TypeScriptCodegenConfig {
    export: bool,
    emit_node_interface: bool,
    include_points: bool,
    match_only_type: MatchOnlyType,
    colors: Colors,
    node_representation: TypeScriptNodeRepresentation,
}

impl Default for TypeScriptCodegenConfig {
    fn default() -> Self {
        Self {
            export: true,
            emit_node_interface: true,
            include_points: false,
            match_only_type: MatchOnlyType::Undefined,
            colors: Colors::OFF,
            node_representation: TypeScriptNodeRepresentation::SerializedValue,
        }
    }
}

impl TypeScriptCodegenConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn export(mut self, enabled: bool) -> Self {
        self.export = enabled;
        self
    }

    pub fn emit_node_interface(mut self, enabled: bool) -> Self {
        self.emit_node_interface = enabled;
        self
    }

    pub fn include_points(mut self, enabled: bool) -> Self {
        self.include_points = enabled;
        self
    }

    pub fn match_only_type(mut self, match_only_type: MatchOnlyType) -> Self {
        self.match_only_type = match_only_type;
        self
    }

    pub fn colored(mut self, enabled: bool) -> Self {
        self.colors = Colors::new(enabled);
        self
    }

    pub fn node_representation(mut self, representation: TypeScriptNodeRepresentation) -> Self {
        self.node_representation = representation;
        self
    }

    pub(crate) fn legacy_config(&self) -> crate::compiler::emit::targets::typescript::Config {
        crate::compiler::emit::targets::typescript::Config::new()
            .export(self.export)
            .emit_node_interface(self.emit_node_interface)
            .include_points(self.include_points)
            .match_only_type(self.match_only_type)
            .colored(!self.colors.blue.is_empty())
    }

    pub(crate) fn colored_output(&self) -> bool {
        !self.colors.blue.is_empty()
    }

    fn validate(&self) -> Result<(), EmitConfigError> {
        if self.node_representation == TypeScriptNodeRepresentation::LiveTreeSitterNode {
            return Err(EmitConfigError::new(
                "live tree-sitter nodes require the future TypeScript module target",
            ));
        }
        Ok(())
    }
}

impl private::Sealed for TypeScriptCodegenConfig {}

impl CodegenTarget for TypeScriptCodegenConfig {
    type TypesOutput = TypeScriptTypesOutput;

    fn emit_types(self, query: &CompiledQuery) -> QueryResult<Emission<Self::TypesOutput>> {
        self.validate()?;
        query.emit_typescript_types(&self)
    }
}

macro_rules! source_output {
    ($name:ident) => {
        pub struct $name {
            source: String,
        }

        impl $name {
            pub(crate) fn new(source: String) -> Self {
                Self { source }
            }

            pub fn source(&self) -> &str {
                &self.source
            }

            pub fn into_source(self) -> String {
                self.source
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.source()
            }
        }
    };
}

source_output!(RustModuleOutput);
source_output!(RustTypesOutput);

pub struct TypeScriptTypesOutput {
    source: String,
    bindings: Vec<TypeScriptBinding>,
}

impl TypeScriptTypesOutput {
    pub(crate) fn new(source: String, bindings: Vec<TypeScriptBinding>) -> Self {
        Self { source, bindings }
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn bindings(&self) -> &[TypeScriptBinding] {
        &self.bindings
    }

    pub fn into_parts(self) -> (String, Vec<TypeScriptBinding>) {
        (self.source, self.bindings)
    }
}
