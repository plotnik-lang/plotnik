use super::*;

#[test]
fn size_and_alignment() {
    assert_eq!(size_of::<RefTransition>(), 4);
    assert_eq!(align_of::<RefTransition>(), 2);
}

#[test]
fn none_is_default() {
    assert_eq!(RefTransition::default(), RefTransition::None);
}

#[test]
fn is_none() {
    assert!(RefTransition::None.is_none());
    assert!(!RefTransition::Enter(1).is_none());
    assert!(!RefTransition::Exit(1).is_none());
}

#[test]
fn ref_id_extraction() {
    assert_eq!(RefTransition::None.ref_id(), None);
    assert_eq!(RefTransition::Enter(42).ref_id(), Some(42));
    assert_eq!(RefTransition::Exit(123).ref_id(), Some(123));
}
