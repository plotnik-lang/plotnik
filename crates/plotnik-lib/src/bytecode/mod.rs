//! Bytecode module for compiled Plotnik queries.
//!
//! Implements the binary format specified in `docs/binary-format/`.

mod constants;
mod header;
mod ids;

pub use constants::{
    MAGIC, SECTION_ALIGN, STEP_ACCEPT, STEP_SIZE, TYPE_CUSTOM_START, TYPE_NODE, TYPE_STRING,
    TYPE_VOID, VERSION,
};

pub use ids::{QTypeId, StepId, StringId};

pub use header::Header;
