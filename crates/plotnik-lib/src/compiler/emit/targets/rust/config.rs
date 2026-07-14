//! Configuration for Rust matcher emission.

use crate::compiler::emit::targets::rust::TypesConfig as RustTypesConfig;
use crate::core::grammar::GrammarIdentity;
use plotnik_rt::{Limit, RuntimeLimitSpec};

#[derive(Clone, Debug)]
pub struct Config {
    /// Rust output-type configuration shared with the type renderer.
    pub(crate) rust_types: RustTypesConfig,
    /// The limit policy compiled into the module's safe entry points.
    /// Chosen at generation time, never at the call site: the query is
    /// trusted, the input is not, and the query's author is the one who knows
    /// the budget it deserves.
    pub(crate) limits: RuntimeLimitSpec,
    /// The decode-depth policy for safe `parse`. Not part of
    /// [`RuntimeLimitSpec`] — that spec is shared with the VM, whose output
    /// rendering is iterative; decode depth is a generated-executor resource
    /// (its typed decoder recurses once per nested value).
    pub(crate) decode_depth: Limit,
    /// Diagnostic provenance for the exact grammar used during binding.
    /// Proc-macro output leaves this absent because Cargo already couples its
    /// parser and generated module; product `generate` paths always set it.
    pub(crate) grammar_identity: Option<GrammarIdentity>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            rust_types: RustTypesConfig::new(),
            limits: RuntimeLimitSpec {
                fuel_limit: Limit::Auto,
                memory: Limit::Auto,
            },
            decode_depth: Limit::Auto,
            grammar_identity: None,
        }
    }
}

impl Config {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn rt_crate(mut self, path: impl Into<std::borrow::Cow<'static, str>>) -> Self {
        self.rust_types = self.rust_types.rt_crate(path);
        self
    }

    /// Also emit `SerializeWithSource` impls for the result types.
    pub fn serde(mut self, enabled: bool) -> Self {
        self.rust_types = self.rust_types.serde(enabled);
        self
    }

    /// Override the compiled-in limit policy for the safe entry points.
    pub fn limits(mut self, limits: RuntimeLimitSpec) -> Self {
        self.limits = limits;
        self
    }

    /// Override the compiled-in decode-depth policy for safe `parse` (see the
    /// field's doc for why it lives outside the shared spec).
    pub fn decode_depth(mut self, depth: Limit) -> Self {
        self.decode_depth = depth;
        self
    }

    /// Record the exact grammar artifact used to bind this generated module.
    pub fn grammar_identity(mut self, identity: GrammarIdentity) -> Self {
        self.grammar_identity = Some(identity);
        self
    }

    pub(crate) fn rt_crate_path(&self) -> &str {
        self.rust_types.rt_crate_path()
    }
}
