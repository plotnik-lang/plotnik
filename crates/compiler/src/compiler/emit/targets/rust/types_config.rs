use std::borrow::Cow;

#[derive(Clone, Debug)]
pub struct Config {
    pub(crate) rt_crate: Cow<'static, str>,
    pub(crate) serde: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            rt_crate: Cow::Borrowed("::plotnik_runtime"),
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

    pub fn serde(mut self, enabled: bool) -> Self {
        self.serde = enabled;
        self
    }

    pub(crate) fn rt_crate_path(&self) -> &str {
        &self.rt_crate
    }
}
