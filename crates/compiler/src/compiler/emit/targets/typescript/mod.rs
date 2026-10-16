mod config;
mod types;

pub use config::Config;
pub(crate) use types::emit_schema;

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct TypeScriptBinding {
    pub span: (u32, u32),
    pub type_id: u32,
    pub member_id: Option<u16>,
}
