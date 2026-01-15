use super::aligned_vec::{AlignedVec, ALIGN};

fn is_aligned(ptr: *const u8) -> bool {
    (ptr as usize).is_multiple_of(ALIGN)
}

#[test]
fn alignment_guarantee() {
    let data: Vec<u8> = (0..100).collect();
    let vec = AlignedVec::copy_from_slice(&data);
    assert!(is_aligned(vec.as_ptr()));
}

#[test]
fn copy_from_slice() {
    let data = [1u8, 2, 3, 4, 5];
    let vec = AlignedVec::copy_from_slice(&data);

    assert!(is_aligned(vec.as_ptr()));
    assert_eq!(&*vec, &data);
}

#[test]
fn empty_slice() {
    let vec = AlignedVec::copy_from_slice(&[]);
    assert!(vec.is_empty());
    assert_eq!(vec.len(), 0);
    assert_eq!(vec.as_slice(), &[] as &[u8]);
}

#[test]
fn clone_preserves_alignment() {
    let data: Vec<u8> = (0..100).collect();
    let vec = AlignedVec::copy_from_slice(&data);
    let cloned = vec.clone();

    assert!(is_aligned(cloned.as_ptr()));
    assert_eq!(&*cloned, &*vec);
}

#[test]
fn deref_to_slice() {
    let vec = AlignedVec::copy_from_slice(&[10, 20, 30]);

    let slice: &[u8] = &vec;
    assert_eq!(slice, &[10, 20, 30]);
    assert_eq!(vec[0], 10);
    assert_eq!(vec[2], 30);
}

#[test]
fn large_data() {
    let data: Vec<u8> = (0..10_000).map(|i| (i % 256) as u8).collect();
    let vec = AlignedVec::copy_from_slice(&data);

    assert!(is_aligned(vec.as_ptr()));
    assert_eq!(&*vec, &data[..]);
}

#[test]
fn partial_block() {
    let data: Vec<u8> = (0..37).collect();
    let vec = AlignedVec::copy_from_slice(&data);

    assert_eq!(vec.len(), 37);
    assert_eq!(&*vec, &data[..]);
}

#[test]
fn exact_block_boundary() {
    let data: Vec<u8> = (0..128).map(|i| i as u8).collect();
    let vec = AlignedVec::copy_from_slice(&data);

    assert_eq!(vec.len(), 128);
    assert_eq!(&*vec, &data[..]);
}
