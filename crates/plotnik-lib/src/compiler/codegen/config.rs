//! Configuration for Rust matcher emission.

use std::borrow::Cow;

use plotnik_rt::{Limit, RuntimeLimitSpec};

#[derive(Clone, Debug)]
pub struct Config {
    /// Absolute path of the runtime crate as generated code should spell it.
    /// The default matches a direct `plotnik-rt` dependency; the proc-macro
    /// backend re-points it at its own re-export.
    pub(crate) rt_crate: Cow<'static, str>,
    /// Also emit `SerializeWithSource` impls for the output types.
    pub(crate) serde: bool,
    /// The limit policy compiled into the module's `try_*` entry points.
    /// Chosen at generation time, never at the call site: the query is
    /// trusted, the input is not, and the query's author is the one who knows
    /// the budget it deserves.
    pub(crate) limits: RuntimeLimitSpec,
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

    /// Override the compiled-in limit policy for the `try_*` entry points.
    pub fn limits(mut self, limits: RuntimeLimitSpec) -> Self {
        self.limits = limits;
        self
    }
}
