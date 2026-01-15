use super::*;

#[test]
fn entrypoint_size() {
    assert_eq!(std::mem::size_of::<Entrypoint>(), 8);
}
