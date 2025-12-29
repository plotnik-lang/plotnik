//! Entrypoint section types.

use super::{QTypeId, StepId, StringId};

/// Named query definition entry point (8 bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct Entrypoint {
    /// Definition name.
    pub name: StringId,
    /// Starting instruction (StepId).
    pub target: StepId,
    /// Result type.
    pub result_type: QTypeId,
    pub(crate) _pad: u16,
}

const _: () = assert!(std::mem::size_of::<Entrypoint>() == 8);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entrypoint_size() {
        assert_eq!(std::mem::size_of::<Entrypoint>(), 8);
    }
}
