//! Configuration for Rust matcher emission.

use std::borrow::Cow;

#[derive(Clone, Debug)]
pub struct Config {
    /// Absolute path of the runtime crate as generated code should spell it.
    /// The default matches a direct `plotnik-rt` dependency; the proc-macro
    /// backend re-points it at its own re-export.
    pub(crate) rt_crate: Cow<'static, str>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            rt_crate: Cow::Borrowed("::plotnik_rt"),
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
}
