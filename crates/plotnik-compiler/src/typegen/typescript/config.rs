//! Configuration types for TypeScript emission.

use plotnik_core::Colors;

/// How to represent the void type in TypeScript.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VoidType {
    /// `undefined` - the absence of a value
    #[default]
    Undefined,
    /// `null` - explicit null value
    Null,
}

/// Configuration for TypeScript emission.
#[derive(Clone, Debug)]
pub struct Config {
    /// Whether to export types
    pub(crate) export: bool,
    /// Whether to emit the Node type definition
    pub(crate) emit_node_type: bool,
    /// Use verbose node representation (with kind, text, etc.)
    pub(crate) verbose_nodes: bool,
    /// How to represent the void type
    pub(crate) void_type: VoidType,
    /// Color configuration for output
    pub(crate) colors: Colors,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            export: true,
            emit_node_type: true,
            verbose_nodes: false,
            void_type: VoidType::default(),
            colors: Colors::OFF,
        }
    }
}

impl Config {
    /// Create a new Config with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set whether to export types.
    pub fn export(mut self, value: bool) -> Self {
        self.export = value;
        self
    }

    /// Set whether to emit the Node type definition.
    pub fn emit_node_type(mut self, value: bool) -> Self {
        self.emit_node_type = value;
        self
    }

    /// Set whether to use verbose node representation.
    pub fn verbose_nodes(mut self, value: bool) -> Self {
        self.verbose_nodes = value;
        self
    }

    /// Set the void type representation.
    pub fn void_type(mut self, value: VoidType) -> Self {
        self.void_type = value;
        self
    }

    /// Set whether to use colored output.
    pub fn colored(mut self, enabled: bool) -> Self {
        self.colors = Colors::new(enabled);
        self
    }
}
