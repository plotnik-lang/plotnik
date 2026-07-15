use serde::ser::{SerializeMap, SerializeSeq, SerializeStruct};
use serde::{Serialize, Serializer};

struct FixtureValue;

impl Serialize for FixtureValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(4))?;
        map.serialize_entry("text", "héllo\nworld")?;
        map.serialize_entry("enabled", &true)?;
        map.serialize_entry("missing", &Option::<bool>::None)?;
        map.serialize_entry("items", &Items)?;
        map.end()
    }
}

struct Items;

impl Serialize for Items {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut sequence = serializer.serialize_seq(Some(2))?;
        sequence.serialize_element(&Node)?;
        sequence.serialize_element(&"tail")?;
        sequence.end()
    }
}

struct Node;

impl Serialize for Node {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut node = serializer.serialize_struct("Node", 3)?;
        node.serialize_field("kind", "identifier")?;
        node.serialize_field("text", "foo")?;
        node.serialize_field("span", &[0, 3])?;
        node.end()
    }
}

#[test]
fn canonical_json_sorts_objects_recursively_and_compacts_node_spans() {
    let actual = super::debug::to_json(&FixtureValue).unwrap();

    assert_eq!(
        actual,
        r#"{
  "enabled": true,
  "items": [
    {
      "kind": "identifier",
      "span": [0, 3],
      "text": "foo"
    },
    "tail"
  ],
  "missing": null,
  "text": "héllo\nworld"
}"#
    );
}

struct Variant;

impl Serialize for Variant {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("$tag", "Some")?;
        map.serialize_entry("$data", &FixtureValue)?;
        map.end()
    }
}

#[test]
fn canonical_json_ignores_serializer_insertion_order() {
    let forward = super::debug::to_json(&serde_json::json!({ "b": 2, "a": 1 })).unwrap();
    let reverse = super::debug::to_json(&ReverseMap).unwrap();

    assert_eq!(forward, reverse);
    assert_eq!(forward, "{\n  \"a\": 1,\n  \"b\": 2\n}");
}

struct ReverseMap;

impl Serialize for ReverseMap {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("b", &2)?;
        map.serialize_entry("a", &1)?;
        map.end()
    }
}

#[test]
fn canonical_json_sorts_reserved_variant_keys_without_exceptions() {
    let actual = super::debug::to_json(&Variant).unwrap();

    assert!(actual.starts_with("{\n  \"$data\":"));
    assert!(actual.ends_with("  \"$tag\": \"Some\"\n}"));
}

#[test]
fn ordinary_two_element_lists_remain_multiline() {
    let actual = super::debug::to_json(&[1, 2]).unwrap();

    assert_eq!(actual, "[\n  1,\n  2\n]");
}
