use crate::{Interner, Symbol};

#[test]
fn intern_deduplicates() {
    let mut interner = Interner::new();

    let a = interner.intern("foo");
    let b = interner.intern("foo");
    let c = interner.intern("bar");

    assert_eq!(a, b);
    assert_ne!(a, c);
    assert_eq!(interner.len(), 2);
}

#[test]
fn resolve_roundtrip() {
    let mut interner = Interner::new();

    let sym = interner.intern("hello");
    assert_eq!(interner.resolve(sym), "hello");
}

#[test]
fn intern_owned_avoids_clone_on_hit() {
    let mut interner = Interner::new();

    let a = interner.intern("test");
    let b = interner.intern_owned("test".to_string());

    assert_eq!(a, b);
    assert_eq!(interner.len(), 1);
}

#[test]
fn symbols_are_copy() {
    let mut interner = Interner::new();
    let sym = interner.intern("x");

    let copy = sym;
    assert_eq!(sym, copy);
}

#[test]
fn symbol_ordering_is_insertion_order() {
    let mut interner = Interner::new();

    let z = interner.intern("z");
    let a = interner.intern("a");

    // z was inserted first, so z < a by insertion order
    assert!(z < a);
}

#[test]
fn to_blob_produces_correct_format() {
    let mut interner = Interner::new();
    interner.intern("id");
    interner.intern("foo");

    let (blob, offsets) = interner.to_blob();

    assert_eq!(blob, b"idfoo");
    assert_eq!(offsets, vec![0, 2, 5]);

    // Verify we can reconstruct strings
    let s0 = &blob[offsets[0] as usize..offsets[1] as usize];
    let s1 = &blob[offsets[1] as usize..offsets[2] as usize];
    assert_eq!(s0, b"id");
    assert_eq!(s1, b"foo");
}

#[test]
fn to_blob_empty() {
    let interner = Interner::new();
    let (blob, offsets) = interner.to_blob();

    assert!(blob.is_empty());
    assert_eq!(offsets, vec![0]); // just the sentinel
}

#[test]
fn iter_yields_all_strings() {
    let mut interner = Interner::new();
    let a = interner.intern("alpha");
    let b = interner.intern("beta");

    let items: Vec<_> = interner.iter().collect();
    assert_eq!(items, vec![(a, "alpha"), (b, "beta")]);
}

#[test]
fn symbol_from_raw_roundtrip() {
    let sym = Symbol::from_raw(42);
    assert_eq!(sym.as_u32(), 42);
}
