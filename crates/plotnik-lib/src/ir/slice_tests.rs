use super::*;

#[test]
fn empty_slice() {
    let slice: Slice<u32> = Slice::empty();

    assert!(slice.is_empty());
    assert_eq!(slice.start_index(), 0);
    assert_eq!(slice.len(), 0);
}

#[test]
fn new_slice() {
    let slice: Slice<u16> = Slice::new(42, 10);

    assert!(!slice.is_empty());
    assert_eq!(slice.start_index(), 42);
    assert_eq!(slice.len(), 10);
}

#[test]
fn default_is_empty() {
    let slice: Slice<u8> = Slice::default();
    assert!(slice.is_empty());
}

#[test]
fn from_inner_type() {
    let slice: Slice<()> = Slice::from_inner_type(0x1234);

    assert_eq!(slice.start_index(), 0x1234);
    assert_eq!(slice.len(), 0);
}

#[test]
fn equality() {
    let a: Slice<u32> = Slice::new(10, 5);
    let b: Slice<u32> = Slice::new(10, 5);
    let c: Slice<u32> = Slice::new(10, 6);

    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn size_is_6_bytes() {
    assert_eq!(std::mem::size_of::<Slice<u64>>(), 6);
}
