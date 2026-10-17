//! Operation-owned storage: one account per operation, fallible buffers, and
//! the raw allocation path shared by every block the runtime creates.

mod account;
mod buffer;
pub(crate) mod raw;
mod shared;
mod text;

pub use account::{Account, Meter};
pub use buffer::Buffer;
pub use shared::Shared;
pub use text::Text;

#[cfg(feature = "failpoints")]
pub use raw::failpoints;

#[cfg(test)]
mod alloc_tests;
