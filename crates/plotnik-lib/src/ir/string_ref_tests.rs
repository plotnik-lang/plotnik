use super::*;

#[test]
fn string_ref_new() {
    let r = StringRef::new(42, 10);
    assert_eq!(r.offset, 42);
    assert_eq!(r.len, 10);
}

#[test]
fn string_ref_layout() {
    assert_eq!(size_of::<StringRef>(), 8);
    assert_eq!(align_of::<StringRef>(), 4);
}
