//! Small string utilities shared by query passes.
//!
//! This module intentionally stays minimal and dependency-free.
//! Only extract helpers here when they are used by 2+ modules or are clearly
//! pass-agnostic (formatting, suggestion, small string algorithms).

pub(crate) use crate::core::utils::find_similar;
