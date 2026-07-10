//! TypeScript type target.

mod config;
mod types;

pub use config::{Config, VoidType};
pub(crate) use types::{emit_schema, emit_schema_mapped};

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct DtsRange {
    pub start: u32,
    pub end: u32,
    pub type_id: u32,
    pub member: Option<u16>,
}
