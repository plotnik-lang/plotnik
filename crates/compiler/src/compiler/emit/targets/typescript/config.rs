#[derive(Clone, Debug)]
pub struct Config {
    pub(crate) emit_node_interface: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            emit_node_interface: true,
        }
    }
}

impl Config {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn emit_node_interface(mut self, value: bool) -> Self {
        self.emit_node_interface = value;
        self
    }
}
