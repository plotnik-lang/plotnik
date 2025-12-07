//! Builder-pattern printer for emitting inferred types as code.
//!
//! # Example
//!
//! ```ignore
//! let code = query.type_printer()
//!     .entry_name("MyQuery")
//!     .rust()
//!     .derive(&["debug", "clone"])
//!     .render();
//! ```

use super::TypeTable;
use super::emit::{
    Indirection, OptionalStyle, RustEmitConfig, TypeScriptEmitConfig, emit_rust, emit_typescript,
};

/// Builder for type emission. Use [`rust()`](Self::rust) or [`typescript()`](Self::typescript)
/// to select the target language.
pub struct TypePrinter<'src> {
    table: TypeTable<'src>,
    entry_name: String,
}

impl<'src> TypePrinter<'src> {
    /// Create a new type printer from a type table.
    pub fn new(table: TypeTable<'src>) -> Self {
        Self {
            table,
            entry_name: "QueryResult".to_string(),
        }
    }

    /// Set the name for the entry point type (default: "QueryResult").
    pub fn entry_name(mut self, name: impl Into<String>) -> Self {
        self.entry_name = name.into();
        self
    }

    /// Configure Rust output.
    pub fn rust(self) -> RustPrinter<'src> {
        let config = RustEmitConfig {
            entry_name: self.entry_name,
            ..Default::default()
        };
        RustPrinter {
            table: self.table,
            config,
        }
    }

    /// Configure TypeScript output.
    pub fn typescript(self) -> TypeScriptPrinter<'src> {
        let config = TypeScriptEmitConfig {
            entry_name: self.entry_name,
            ..Default::default()
        };
        TypeScriptPrinter {
            table: self.table,
            config,
        }
    }
}

/// Builder for Rust code emission.
pub struct RustPrinter<'src> {
    table: TypeTable<'src>,
    config: RustEmitConfig,
}

impl<'src> RustPrinter<'src> {
    /// Set indirection type for cyclic references (default: Box).
    pub fn indirection(mut self, ind: Indirection) -> Self {
        self.config.indirection = ind;
        self
    }

    /// Set derive macros from a list of trait names.
    ///
    /// Recognized names: "debug", "clone", "partialeq" (case-insensitive).
    /// Unrecognized names are ignored.
    pub fn derive(mut self, traits: &[&str]) -> Self {
        self.config.derive_debug = false;
        self.config.derive_clone = false;
        self.config.derive_partial_eq = false;

        for t in traits {
            match t.to_lowercase().as_str() {
                "debug" => self.config.derive_debug = true,
                "clone" => self.config.derive_clone = true,
                "partialeq" => self.config.derive_partial_eq = true,
                _ => {}
            }
        }
        self
    }

    /// Render the type definitions as Rust code.
    pub fn render(&self) -> String {
        emit_rust(&self.table, &self.config)
    }
}

/// Builder for TypeScript code emission.
pub struct TypeScriptPrinter<'src> {
    table: TypeTable<'src>,
    config: TypeScriptEmitConfig,
}

impl<'src> TypeScriptPrinter<'src> {
    /// Set how optional values are represented (default: Null).
    pub fn optional(mut self, style: OptionalStyle) -> Self {
        self.config.optional = style;
        self
    }

    /// Whether to add `export` keyword to types (default: false).
    pub fn export(mut self, value: bool) -> Self {
        self.config.export = value;
        self
    }

    /// Whether to make fields readonly (default: false).
    pub fn readonly(mut self, value: bool) -> Self {
        self.config.readonly = value;
        self
    }

    /// Whether to emit nested synthetic types instead of inlining (default: false).
    pub fn nested(mut self, value: bool) -> Self {
        self.config.nested = value;
        self
    }

    /// Set the name for the Node type (default: "SyntaxNode").
    pub fn node_type(mut self, name: impl Into<String>) -> Self {
        self.config.node_type = name.into();
        self
    }

    /// Whether to use `type Foo = ...` instead of `interface Foo { ... }` (default: false).
    pub fn type_alias(mut self, value: bool) -> Self {
        self.config.type_alias = value;
        self
    }

    /// Render the type definitions as TypeScript code.
    pub fn render(&self) -> String {
        emit_typescript(&self.table, &self.config)
    }
}
