//! Configuration types for TypeScript emission.

use crate::core::Colors;

/// How to represent the void type in TypeScript.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VoidType {
    /// `undefined` - the absence of a value
    #[default]
    Undefined,
    /// `null` - explicit null value
    Null,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub(crate) export: bool,
    pub(crate) emit_node_interface: bool,
    pub(crate) verbose_nodes: bool,
    pub(crate) void_type: VoidType,
    pub(crate) colors: Colors,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            export: true,
            emit_node_interface: true,
            verbose_nodes: false,
            void_type: VoidType::default(),
            colors: Colors::OFF,
        }
    }
}

impl Config {
    /// Create a new Config with default values.
    pub fn builder() -> Self {
        Self::default()
    }

    pub fn export(mut self, value: bool) -> Self {
        self.export = value;
        self
    }

    pub fn emit_node_interface(mut self, value: bool) -> Self {
        self.emit_node_interface = value;
        self
    }

    pub fn verbose_nodes(mut self, value: bool) -> Self {
        self.verbose_nodes = value;
        self
    }

    pub fn void_type(mut self, value: VoidType) -> Self {
        self.void_type = value;
        self
    }

    pub fn colored(mut self, enabled: bool) -> Self {
        self.colors = Colors::new(enabled);
        self
    }
}
