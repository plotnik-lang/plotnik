use super::*;

#[test]
fn slice_range() {
    let slice = Slice { ptr: 5, len: 3 };
    assert_eq!(slice.range(), 5..8);
}
