//! Result graphs: normalization of accepted construction tapes, typed walks,
//! and publication into region-owned native matches or indexed VM documents.

mod document;
mod family;
mod owner;
mod publish;
mod region;
mod rows;
mod schema;
mod scope;
mod walk;

pub use document::Document;
pub use family::{Publish, ResultFamily};
pub use owner::{Iter, Match, Matches};
pub use publish::Publisher;
pub use rows::{Normalizer, Rows};
pub use schema::{Case, Field, Member, Schema, SchemaTables, Span, Ty};
pub use scope::{Scope, Slice};
pub use walk::{Step, Walk, WalkStack};
