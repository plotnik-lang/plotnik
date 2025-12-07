use super::types::{TypeKey, TypeTable, TypeValue, to_pascal};
use indexmap::IndexMap;

#[test]
fn type_key_to_pascal_case_builtins() {
    assert_eq!(TypeKey::Node.to_pascal_case(), "Node");
    assert_eq!(TypeKey::String.to_pascal_case(), "String");
    assert_eq!(TypeKey::Unit.to_pascal_case(), "Unit");
}

#[test]
fn type_key_to_pascal_case_named() {
    assert_eq!(
        TypeKey::Named("FunctionInfo").to_pascal_case(),
        "FunctionInfo"
    );
    assert_eq!(TypeKey::Named("Stmt").to_pascal_case(), "Stmt");
}

#[test]
fn type_key_to_pascal_case_synthetic() {
    assert_eq!(TypeKey::Synthetic(vec!["Foo"]).to_pascal_case(), "Foo");
    assert_eq!(
        TypeKey::Synthetic(vec!["Foo", "bar"]).to_pascal_case(),
        "FooBar"
    );
    assert_eq!(
        TypeKey::Synthetic(vec!["Foo", "bar", "baz"]).to_pascal_case(),
        "FooBarBaz"
    );
}

#[test]
fn type_key_to_pascal_case_snake_case_segments() {
    assert_eq!(
        TypeKey::Synthetic(vec!["Foo", "bar_baz"]).to_pascal_case(),
        "FooBarBaz"
    );
    assert_eq!(
        TypeKey::Synthetic(vec!["function_info", "params"]).to_pascal_case(),
        "FunctionInfoParams"
    );
}

#[test]
fn type_table_new_has_builtins() {
    let table = TypeTable::new();
    assert_eq!(table.get(&TypeKey::Node), Some(&TypeValue::Node));
    assert_eq!(table.get(&TypeKey::String), Some(&TypeValue::String));
    assert_eq!(table.get(&TypeKey::Unit), Some(&TypeValue::Unit));
}

#[test]
fn type_table_insert_and_get() {
    let mut table = TypeTable::new();
    let key = TypeKey::Named("Foo");
    let value = TypeValue::Struct(IndexMap::new());
    table.insert(key.clone(), value.clone());
    assert_eq!(table.get(&key), Some(&value));
}

#[test]
fn type_table_cyclic_tracking() {
    let mut table = TypeTable::new();
    let key = TypeKey::Named("Recursive");

    assert!(!table.is_cyclic(&key));
    table.mark_cyclic(key.clone());
    assert!(table.is_cyclic(&key));

    // Double marking is idempotent
    table.mark_cyclic(key.clone());
    assert_eq!(table.cyclic.len(), 1);
}

#[test]
fn type_table_iter_preserves_order() {
    let mut table = TypeTable::new();
    table.insert(TypeKey::Named("A"), TypeValue::Unit);
    table.insert(TypeKey::Named("B"), TypeValue::Unit);
    table.insert(TypeKey::Named("C"), TypeValue::Unit);

    let keys: Vec<_> = table.iter().map(|(k, _)| k.clone()).collect();
    // Builtins first, then inserted order
    assert_eq!(keys[0], TypeKey::Node);
    assert_eq!(keys[1], TypeKey::String);
    assert_eq!(keys[2], TypeKey::Unit);
    assert_eq!(keys[3], TypeKey::Named("A"));
    assert_eq!(keys[4], TypeKey::Named("B"));
    assert_eq!(keys[5], TypeKey::Named("C"));
}

#[test]
fn type_table_default() {
    let table: TypeTable = Default::default();
    assert!(table.get(&TypeKey::Node).is_some());
}

#[test]
fn type_value_equality() {
    let s1 = TypeValue::Struct(IndexMap::new());
    let s2 = TypeValue::Struct(IndexMap::new());
    assert_eq!(s1, s2);

    let mut fields = IndexMap::new();
    fields.insert("x", TypeKey::Node);
    let s3 = TypeValue::Struct(fields);
    assert_ne!(s1, s3);
}

#[test]
fn type_value_wrapper_types() {
    let opt = TypeValue::Optional(TypeKey::Node);
    let list = TypeValue::List(TypeKey::Node);
    let ne_list = TypeValue::NonEmptyList(TypeKey::Node);

    assert_ne!(opt, list);
    assert_ne!(list, ne_list);
}

#[test]
fn type_value_tagged_union() {
    let mut table = TypeTable::new();

    let mut assign_fields = IndexMap::new();
    assign_fields.insert("target", TypeKey::String);
    table.insert(
        TypeKey::Synthetic(vec!["Stmt", "Assign"]),
        TypeValue::Struct(assign_fields),
    );

    let mut call_fields = IndexMap::new();
    call_fields.insert("func", TypeKey::String);
    table.insert(
        TypeKey::Synthetic(vec!["Stmt", "Call"]),
        TypeValue::Struct(call_fields),
    );

    let mut variants = IndexMap::new();
    variants.insert("Assign", TypeKey::Synthetic(vec!["Stmt", "Assign"]));
    variants.insert("Call", TypeKey::Synthetic(vec!["Stmt", "Call"]));

    let union = TypeValue::TaggedUnion(variants);
    table.insert(TypeKey::Named("Stmt"), union);

    let TypeValue::TaggedUnion(v) = table.get(&TypeKey::Named("Stmt")).unwrap() else {
        panic!("expected TaggedUnion");
    };
    assert_eq!(v.len(), 2);
    assert!(v.contains_key("Assign"));
    assert!(v.contains_key("Call"));
    assert!(table.get(&v["Assign"]).is_some());
}

#[test]
fn type_value_tagged_union_empty_variant() {
    let mut table = TypeTable::new();

    let mut variants = IndexMap::new();
    variants.insert("Empty", TypeKey::Unit);
    table.insert(
        TypeKey::Named("MaybeEmpty"),
        TypeValue::TaggedUnion(variants),
    );

    let TypeValue::TaggedUnion(v) = table.get(&TypeKey::Named("MaybeEmpty")).unwrap() else {
        panic!("expected TaggedUnion");
    };
    assert_eq!(v["Empty"], TypeKey::Unit);
}

#[test]
fn to_pascal_empty_string() {
    assert_eq!(to_pascal(""), "");
}

#[test]
fn to_pascal_single_char() {
    assert_eq!(to_pascal("a"), "A");
    assert_eq!(to_pascal("Z"), "Z");
}

#[test]
fn to_pascal_already_pascal() {
    assert_eq!(to_pascal("FooBar"), "FooBar");
}

#[test]
fn to_pascal_multiple_underscores() {
    assert_eq!(to_pascal("foo__bar"), "FooBar");
    assert_eq!(to_pascal("_foo_"), "Foo");
}

#[test]
fn type_key_equality() {
    assert_eq!(TypeKey::Node, TypeKey::Node);
    assert_ne!(TypeKey::Node, TypeKey::String);
    assert_eq!(TypeKey::Named("Foo"), TypeKey::Named("Foo"));
    assert_ne!(TypeKey::Named("Foo"), TypeKey::Named("Bar"));
    assert_eq!(
        TypeKey::Synthetic(vec!["a", "b"]),
        TypeKey::Synthetic(vec!["a", "b"])
    );
    assert_ne!(
        TypeKey::Synthetic(vec!["a", "b"]),
        TypeKey::Synthetic(vec!["a", "c"])
    );
}

#[test]
fn type_key_hash_consistency() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    set.insert(TypeKey::Node);
    set.insert(TypeKey::Named("Foo"));
    set.insert(TypeKey::Synthetic(vec!["a", "b"]));

    assert!(set.contains(&TypeKey::Node));
    assert!(set.contains(&TypeKey::Named("Foo")));
    assert!(set.contains(&TypeKey::Synthetic(vec!["a", "b"])));
    assert!(!set.contains(&TypeKey::String));
}
