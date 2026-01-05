//! Entrypoint section types.

use super::instructions::StepAddr;
use super::{QTypeId, StringId};

/// Named query definition entry point (8 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Entrypoint {
    /// Definition name.
    pub name: StringId,
    /// Starting instruction address.
    pub target: StepAddr,
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
