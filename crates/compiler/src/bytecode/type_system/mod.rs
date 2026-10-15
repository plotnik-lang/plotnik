mod kind;
mod primitives;

pub use kind::TypeKind;
pub use primitives::{
    PrimitiveType, TYPE_BOOL, TYPE_CUSTOM_START, TYPE_NO_VALUE, TYPE_NODE, TYPE_TEXT,
};
