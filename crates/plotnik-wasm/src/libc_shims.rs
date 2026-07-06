//! libc shims for wasm32-unknown-unknown.
//!
//! Grammar crates compile hand-written C scanners that call the wctype.h
//! family. The upstream `tree-sitter` crate ships its own wasm shims for the
//! allocator family (malloc/free/...), but nothing provides wctype. These
//! definitions live in the cdylib root crate so they are treated as exported
//! symbols and survive LTO, staying visible to the C archive members at link
//! time.
//!
//! C `wint_t` is `u32` here; every function tolerates arbitrary values by
//! mapping non-scalar inputs to "not a member of the class".

use std::ffi::c_int;

fn class(wc: u32, test: impl Fn(char) -> bool) -> c_int {
    c_int::from(char::from_u32(wc).is_some_and(test))
}

#[unsafe(no_mangle)]
pub extern "C" fn iswspace(wc: u32) -> c_int {
    class(wc, char::is_whitespace)
}

#[unsafe(no_mangle)]
pub extern "C" fn iswdigit(wc: u32) -> c_int {
    // C semantics: ASCII decimal digits only, independent of locale.
    class(wc, |c| c.is_ascii_digit())
}

#[unsafe(no_mangle)]
pub extern "C" fn iswxdigit(wc: u32) -> c_int {
    class(wc, |c| c.is_ascii_hexdigit())
}

#[unsafe(no_mangle)]
pub extern "C" fn iswalpha(wc: u32) -> c_int {
    class(wc, char::is_alphabetic)
}

#[unsafe(no_mangle)]
pub extern "C" fn iswalnum(wc: u32) -> c_int {
    class(wc, char::is_alphanumeric)
}

#[unsafe(no_mangle)]
pub extern "C" fn iswupper(wc: u32) -> c_int {
    class(wc, char::is_uppercase)
}

#[unsafe(no_mangle)]
pub extern "C" fn iswlower(wc: u32) -> c_int {
    class(wc, char::is_lowercase)
}

#[unsafe(no_mangle)]
pub extern "C" fn iswpunct(wc: u32) -> c_int {
    class(wc, |c| c.is_ascii_punctuation())
}

/// Unicode simple case mapping: multi-char expansions keep the input, which
/// is what C's towlower contract (single wint_t in, single wint_t out) needs.
fn simple_map(wc: u32, map: impl Fn(char) -> char) -> u32 {
    char::from_u32(wc).map_or(wc, |c| map(c) as u32)
}

#[unsafe(no_mangle)]
pub extern "C" fn towlower(wc: u32) -> u32 {
    simple_map(wc, |c| {
        let mut lower = c.to_lowercase();
        match (lower.next(), lower.next()) {
            (Some(single), None) => single,
            _ => c,
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn towupper(wc: u32) -> u32 {
    simple_map(wc, |c| {
        let mut upper = c.to_uppercase();
        match (upper.next(), upper.next()) {
            (Some(single), None) => single,
            _ => c,
        }
    })
}
