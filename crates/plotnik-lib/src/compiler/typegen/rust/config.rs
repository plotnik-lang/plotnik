//! Configuration for Rust type emission.

use std::borrow::Cow;

#[derive(Clone, Debug)]
pub struct Config {
    /// Absolute path of the runtime crate as generated code should spell it.
    /// The default matches a direct `plotnik-rt` dependency; the proc-macro
    /// backend re-points it at its own re-export.
    pub(crate) rt_crate: Cow<'static, str>,
    pub(crate) serde: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            rt_crate: Cow::Borrowed("::plotnik_rt"),
            serde: false,
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

    /// Also emit `SerializeWithSource` impls for the generated types.
    pub fn serde(mut self, enabled: bool) -> Self {
        self.serde = enabled;
        self
    }
}
