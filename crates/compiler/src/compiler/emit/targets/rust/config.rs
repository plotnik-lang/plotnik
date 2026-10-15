use crate::compiler::emit::targets::rust::TypesConfig as RustTypesConfig;
use crate::core::grammar::GrammarIdentity;
use plotnik_runtime::{Limit, RuntimeLimitSpec};

#[derive(Clone, Debug)]
pub struct Config {
    pub(crate) rust_types: RustTypesConfig,
    pub(crate) limits: RuntimeLimitSpec,
    pub(crate) decode_depth: Limit,
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

    pub fn serde(mut self, enabled: bool) -> Self {
        self.rust_types = self.rust_types.serde(enabled);
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

    pub fn grammar_identity(mut self, identity: GrammarIdentity) -> Self {
        self.grammar_identity = Some(identity);
        self
    }

    pub(crate) fn rt_crate_path(&self) -> &str {
        self.rust_types.rt_crate_path()
    }
}
