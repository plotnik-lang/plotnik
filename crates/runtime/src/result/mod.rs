//! Result construction: normalization of accepted tapes, typed walks, and
//! publication into owned matches.

mod document;
mod family;
mod lease;
mod owner;
mod publish;
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
