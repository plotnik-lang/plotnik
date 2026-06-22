#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod analyze {
    pub mod type_check {
        pub use plotnik_compiler_core::TypeContext;
        pub use plotnik_compiler_core::type_shape::{
            FieldInfo, TYPE_NODE, TYPE_VOID, TypeId, TypeShape,
        };
    }
}

pub mod emit;
pub use emit::*;
