//! Code generation from bytecode Module.
//!
//! This module provides emitters for generating code from compiled bytecode.
//! Each target language has its own submodule with a `Config` struct and `emit()` function.
//!
//! # Example
//!
//! ```ignore
//! use plotnik_lib::codegen::typescript;
//! use plotnik_lib::bytecode::Module;
//!
//! let module = Module::from_bytes(bytecode)?;
//! let config = typescript::Config {
//!     export: true,
//!     emit_node_type: true,
//!     verbose_nodes: false,
//! };
//! let output = typescript::emit_with_config(&module, config);
//! ```

pub mod typescript;
