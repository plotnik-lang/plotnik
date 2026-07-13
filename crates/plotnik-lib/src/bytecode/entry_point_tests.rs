use super::*;

#[test]
fn entry_point_size() {
    assert_eq!(std::mem::size_of::<EntryPoint>(), 8);
}
