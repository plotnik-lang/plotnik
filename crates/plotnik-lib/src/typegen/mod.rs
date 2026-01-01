//! Type declaration generation from compiled bytecode.
//!
//! Extracts type metadata from bytecode modules and generates type declarations
//! for target languages. Currently supports TypeScript `.d.ts` generation.
//!
//! # Example
//!
//! ```ignore
//! use plotnik_lib::typegen::typescript;
//! use plotnik_lib::bytecode::Module;
//!
//! let module = Module::from_bytes(bytecode)?;
//! let output = typescript::emit(&module);
//! ```

pub mod typescript;
