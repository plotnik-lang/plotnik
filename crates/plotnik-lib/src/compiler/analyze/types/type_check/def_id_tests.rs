use super::*;

#[test]
fn def_id_roundtrip() {
    let id = DefId::from_raw(42);
    assert_eq!(id.as_u32(), 42);
    assert_eq!(id.index(), 42);
}

#[test]
fn def_id_equality() {
    let a = DefId::from_raw(1);
    let b = DefId::from_raw(1);
    let c = DefId::from_raw(2);

    assert_eq!(a, b);
    assert_ne!(a, c);
}
