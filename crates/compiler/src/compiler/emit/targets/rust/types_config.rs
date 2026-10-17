use std::borrow::Cow;

#[derive(Clone, Debug)]
pub struct Config {
    pub(crate) rt_crate: Cow<'static, str>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            rt_crate: Cow::Borrowed("::plotnik_runtime"),
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

    pub(crate) fn rt_crate_path(&self) -> &str {
        &self.rt_crate
    }
}
