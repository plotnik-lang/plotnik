use std::alloc::Layout;

use super::schema::Schema;
use super::scope::Scope;
use super::walk::Walk;
use crate::LimitExceeded;
use crate::alloc::Meter;
use crate::json::Json;

/// A generated result type whose result-storage lifetime can be rebound.
///
/// `Borrowed<'a>` is the same type with only its result-storage lifetime
/// replaced by `'a`; tree and source lifetimes stay. Use
/// [`result_family!`](crate::result_family) to implement it.
///
/// # Safety
///
/// `Borrowed<'a>` must be `Self` with only that lifetime replaced, the type
/// must be covariant in it, and its values must hold only shared references,
/// `Node`s, string slices and plain data, so a value never needs dropping.
#[doc(hidden)]
pub unsafe trait ResultFamily: Copy + Send + Sync {
    type Borrowed<'a>: Copy + Send + Sync + 'a
    where
        Self: 'a;
}

/// A generated executable result root and its publisher.
#[doc(hidden)]
pub trait Publish<'t, 's>: ResultFamily {
    /// The module's logical result schema.
    const SCHEMA: Schema<'static>;
    /// The schema type of this root.
    const ROOT: u32;
    /// The native layout of each schema type, by type id.
    const LAYOUTS: &'static [Layout];

    /// Construct the root from the walk steps of one accepted match, placing
    /// every referenced value and list element in `scope`.
    fn build<'a>(
        walk: &mut Walk<'_, '_, 't, 's>,
        scope: &Scope<'a>,
        meter: &mut Meter<'_>,
    ) -> Result<Self::Borrowed<'a>, LimitExceeded>
    where
        Self: 'a;

    fn json<'a>(value: &'a Self::Borrowed<'a>) -> &'a dyn Json
    where
        Self: 'a;
}

/// Implement [`ResultFamily`] for a generated result type.
///
/// The first lifetime, when named `'r`, is the result-storage lifetime. A
/// type without it borrows no result storage and is its own family. The
/// expansion includes a covariance check that fails to compile for a type
/// that is not covariant in `'r`.
#[doc(hidden)]
#[macro_export]
macro_rules! result_family {
    ($ty:ident<'r $(, $lt:lifetime)*>) => {
        // SAFETY: only the result-storage lifetime changes; the covariance
        // check below proves the change is a subtyping coercion.
        unsafe impl<'r $(, $lt)*> $crate::ResultFamily for $ty<'r $(, $lt)*> {
            type Borrowed<'a> = $ty<'a $(, $lt)*> where Self: 'a;
        }

        const _: () = {
            #[allow(dead_code)]
            fn covariant<'short, 'long: 'short $(, $lt: 'long)*>(
                value: $ty<'long $(, $lt)*>,
            ) -> $ty<'short $(, $lt)*> {
                value
            }
        };
    };
    ($ty:ident $(<$($lt:lifetime),+>)?) => {
        // SAFETY: the type borrows no result storage, so rebinding is identity.
        unsafe impl$(<$($lt),+>)? $crate::ResultFamily for $ty$(<$($lt),+>)? {
            type Borrowed<'a> = Self where Self: 'a;
        }
    };
}
