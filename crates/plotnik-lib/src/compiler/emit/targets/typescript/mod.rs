//! TypeScript type target.

mod config;
mod types;

pub use config::{Config, MatchOnlyType};
pub(crate) use types::{emit_schema, emit_schema_mapped};

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct TypeScriptBinding {
    pub span: (u32, u32),
    pub type_id: u32,
    pub member_id: Option<u16>,
}
