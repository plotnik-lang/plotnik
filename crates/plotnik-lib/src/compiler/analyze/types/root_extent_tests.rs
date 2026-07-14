use super::*;

#[test]
fn combine_root_extents() {
    assert_eq!(
        RootExtent::SingleNode.combine(RootExtent::SingleNode),
        RootExtent::SingleNode,
    );
    assert_eq!(
        RootExtent::SingleNode.combine(RootExtent::Other),
        RootExtent::Other,
    );
    assert_eq!(
        RootExtent::Other.combine(RootExtent::SingleNode),
        RootExtent::Other,
    );
    assert_eq!(
        RootExtent::Other.combine(RootExtent::Other),
        RootExtent::Other,
    );
}
