mod alternation;
pub(crate) mod boundary;
mod builder;
mod capture;
mod capture_type;
mod navigation;
mod nfa_emit;
mod patterns;
mod quantifier;
mod scope;
mod sequences;

pub(in crate::compiler::lower) use builder::NfaBuilder;
