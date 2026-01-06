//! Configuration types for TypeScript emission.

use crate::Colors;

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
    pub export: bool,
    /// Whether to emit the Node type definition
    pub emit_node_type: bool,
    /// Use verbose node representation (with kind, text, etc.)
    pub verbose_nodes: bool,
    /// How to represent the void type
    pub void_type: VoidType,
    /// Color configuration for output
    pub colors: Colors,
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
