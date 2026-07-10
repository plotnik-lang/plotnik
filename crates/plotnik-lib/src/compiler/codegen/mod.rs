//! Rust matcher generation — the compiled-code executor backend.
//!
//! Consumes the fork-point NFA ([`SemanticNfa`]) and emits a self-contained
//! Rust module that walks a tree-sitter tree exactly like the bytecode VM
//! interprets the same NFA: one dispatch loop over dense `u16` state ids,
//! per-state arms with every operand const-folded (node kinds, field ids,
//! effect payloads, predicate literals, successor lists), and the shared
//! engine core (`plotnik_rt::Engine`) carrying the checkpoint discipline.
//!
//! Two invariants keep the backends honest:
//!
//! - **No re-derived semantics.** Every arm transcribes one IR instruction;
//!   the control skeletons (dispatch loop, backtrack unwind) mirror
//!   `vm/engine/vm.rs` handler-for-handler, with the VM as the conformance
//!   oracle over the 06-vm corpus.
//! - **Agent debuggability.** State names embed the semantic-NFA label and
//!   owning definition, and every arm carries its instruction rendered in the
//!   dump format, so generated code lines up with `dump_nfa` output 1:1.

mod config;
mod emitter;
mod plan;
mod reader;

#[cfg(test)]
mod emitter_tests;

pub use config::{Config, GrammarIdentity};
pub use emitter::entry_fn_name;
pub(crate) use emitter::generate;
