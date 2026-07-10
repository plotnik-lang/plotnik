//! Configuration for Rust matcher emission.

use std::borrow::Cow;

use crate::core::grammar::GrammarIdentity;
use plotnik_rt::{Limit, RuntimeLimitSpec};

#[derive(Clone, Debug)]
pub struct Config {
    /// Absolute path of the runtime crate as generated code should spell it.
    /// The default matches a direct `plotnik-rt` dependency; the proc-macro
    /// backend re-points it at its own re-export.
    pub(crate) rt_crate: Cow<'static, str>,
    /// Also emit `SerializeWithSource` impls for the output types.
    pub(crate) serde: bool,
    /// The limit policy compiled into the module's safe entry points.
    /// Chosen at generation time, never at the call site: the query is
    /// trusted, the input is not, and the query's author is the one who knows
    /// the budget it deserves.
    pub(crate) limits: RuntimeLimitSpec,
    /// The replay-depth policy for safe `parse`. Not part of
    /// [`RuntimeLimitSpec`] — that spec is shared with the VM, whose output
    /// rendering is iterative; replay depth is a generated-executor resource
    /// (its typed replay recurses once per nested value).
    pub(crate) depth: Limit,
    /// Diagnostic provenance for the exact grammar used during linking.
    /// Proc-macro output leaves this absent because Cargo already couples its
    /// parser and generated module; product `generate` paths always set it.
    pub(crate) grammar_identity: Option<GrammarIdentity>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            rt_crate: Cow::Borrowed("::plotnik_rt"),
            serde: false,
            limits: RuntimeLimitSpec {
                steps: Limit::Auto,
                memory: Limit::Auto,
            },
            depth: Limit::Auto,
            grammar_identity: None,
        }
    }
}

impl Config {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn rt_crate(mut self, path: impl Into<Cow<'static, str>>) -> Self {
        self.rt_crate = path.into();
        self
    }

    /// Also emit `SerializeWithSource` impls for the output types.
    pub fn serde(mut self, enabled: bool) -> Self {
        self.serde = enabled;
        self
    }

    /// Override the compiled-in limit policy for the safe entry points.
    pub fn limits(mut self, limits: RuntimeLimitSpec) -> Self {
        self.limits = limits;
        self
    }

    /// Override the compiled-in replay-depth policy for safe `parse` (see the
    /// field's doc for why it lives outside the shared spec).
    pub fn depth(mut self, depth: Limit) -> Self {
        self.depth = depth;
        self
    }

    /// Record the exact grammar artifact used to link this generated module.
    pub fn grammar_identity(mut self, identity: GrammarIdentity) -> Self {
        self.grammar_identity = Some(identity);
        self
    }
}
