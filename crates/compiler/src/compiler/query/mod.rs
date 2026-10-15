//! Query facade for Plotnik compilation.

mod declaration;
mod dump;
mod input;
mod printer;
mod stages;

pub use declaration::{GrammarDeclaration, GrammarDeclarationError, parse_grammar_declaration};
pub use input::SourceCollection;
pub use stages::{CompiledQuery, Query, QueryBuilder};
