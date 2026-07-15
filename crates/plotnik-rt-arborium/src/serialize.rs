//! Source-threading serialization for query result values.
//!
//! Generated query outputs hold `tree_sitter::Node` handles, and a node's text
//! lives in the source buffer — a plain `serde::Serialize` impl has no way to
//! receive it. [`SerializeWithSource`] threads the source through the
//! serialization walk, and [`WithSource`] adapts any implementor into an
//! ordinary `serde::Serialize` value. Node handles serialize as
//! `{kind, text, span: [start, end]}` — the same JSON shape the VM's
//! materialized values use, which is what lets generated output be diffed
//! against VM output byte-for-byte.

use serde::Serialize;
use serde::ser::{SerializeSeq, Serializer};

/// Serialization that needs the source text alongside the value.
///
/// Generated query modules implement this for their output structs/enums;
/// the leaf and container impls below cover everything those types contain.
pub trait SerializeWithSource {
    fn serialize_with_source<S: Serializer>(
        &self,
        source: &str,
        serializer: S,
    ) -> Result<S::Ok, S::Error>;
}

/// Adapter pairing a value with its source, usable anywhere serde expects a
/// `Serialize` value: `serde_json::to_string(&WithSource::new(&out, src))`.
pub struct WithSource<'a, T: ?Sized> {
    value: &'a T,
    source: &'a str,
}

impl<'a, T: ?Sized> WithSource<'a, T> {
    pub fn new(value: &'a T, source: &'a str) -> Self {
        Self { value, source }
    }
}

impl<T: SerializeWithSource + ?Sized> Serialize for WithSource<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.value.serialize_with_source(self.source, serializer)
    }
}

impl SerializeWithSource for tree_sitter::Node<'_> {
    fn serialize_with_source<S: Serializer>(
        &self,
        source: &str,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let text = source
            .get(self.start_byte()..self.end_byte())
            .expect("node span must lie within source on UTF-8 boundaries");
        let mut s = serializer.serialize_struct("Node", 3)?;
        s.serialize_field("kind", self.kind())?;
        s.serialize_field("text", text)?;
        s.serialize_field("span", &[self.start_byte(), self.end_byte()])?;
        s.end()
    }
}

impl SerializeWithSource for &str {
    fn serialize_with_source<S: Serializer>(
        &self,
        _source: &str,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        self.serialize(serializer)
    }
}

impl SerializeWithSource for bool {
    fn serialize_with_source<S: Serializer>(
        &self,
        _source: &str,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        self.serialize(serializer)
    }
}

/// `None` serializes as a bare null — one flat null level, matching the VM,
/// which never nests nulls even where the static type is `Option<Option<T>>`.
impl<T: SerializeWithSource> SerializeWithSource for Option<T> {
    fn serialize_with_source<S: Serializer>(
        &self,
        source: &str,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match self {
            Some(value) => value.serialize_with_source(source, serializer),
            None => serializer.serialize_none(),
        }
    }
}

impl<T: SerializeWithSource> SerializeWithSource for Vec<T> {
    fn serialize_with_source<S: Serializer>(
        &self,
        source: &str,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.len()))?;
        for element in self {
            seq.serialize_element(&WithSource::new(element, source))?;
        }
        seq.end()
    }
}

impl<T: SerializeWithSource + ?Sized> SerializeWithSource for Box<T> {
    fn serialize_with_source<S: Serializer>(
        &self,
        source: &str,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        (**self).serialize_with_source(source, serializer)
    }
}
