use super::state_set::StateSet;

#[test]
fn insert_reports_novelty_and_membership() {
    let mut set = StateSet::default();
    assert_eq!(set.iter().next(), None);
    assert!(set.insert(3));
    assert!(!set.insert(3));
    assert!(set.contains(3));
    assert!(!set.contains(4));
    assert_eq!(set.iter().collect::<Vec<_>>(), vec![3]);
}

#[test]
fn grows_across_word_boundaries() {
    let mut set = StateSet::singleton(1);
    // 70 lands in the second 64-bit word; the set must grow to hold it.
    assert!(set.insert(70));
    assert!(set.contains(1));
    assert!(set.contains(70));
    assert_eq!(set.iter().collect::<Vec<_>>(), vec![1, 70]);
}

#[test]
fn union_reports_growth() {
    let mut a = StateSet::singleton(1);
    let b = StateSet::singleton(2);
    assert!(a.union_with(&b));
    assert!(!a.union_with(&b));
    assert!(a.contains(1));
    assert!(a.contains(2));
}

#[test]
fn equality_compares_contents_regardless_of_word_length() {
    // `a` allocates a second word for bit 70; `b` reaches the same contents by a
    // different insertion order. Equality must see them as the same set.
    let mut a = StateSet::singleton(1);
    a.insert(70);
    let mut b = StateSet::singleton(70);
    b.insert(1);
    assert_eq!(a, b);

    let narrow = StateSet::singleton(1);
    assert_ne!(a, narrow);
    assert_eq!(narrow, StateSet::singleton(1));
}
