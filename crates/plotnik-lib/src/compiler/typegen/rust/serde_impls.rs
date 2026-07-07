//! Generated `SerializeWithSource` impls.
//!
//! Mirrors the VM's materialized-value JSON exactly — struct fields in
//! declaration order, `None` as one flat null, nodes as
//! `{kind, text, span}`, enums as `{"$tag": ...}` / `{"$tag", "$data"}` —
//! so serialized generated output can be diffed against VM output verbatim.
//! Serialized keys and tags always use the original query-side names, even
//! when the Rust identifier had to be keyword-renamed.
//!
//! Enum variant payloads are anonymous structs, so each payload arm defines a
//! local `Data` adapter borrowing the matched fields; bindings are positional
//! (`v0`, `v1`, ...) to keep a capture literally named `source` from
//! shadowing the source parameter.

use std::fmt::Write as _;

use crate::compiler::analyze::types::type_shape::{TYPE_VOID, TypeId, TypeShape};

use super::emitter::{Emitter, Item, ItemKind, TypeContext};
use super::idents::scope_idents;

struct EnumVariant<'a> {
    item_ty: TypeId,
    payload: TypeId,
    enum_ident: &'a str,
    variant_ident: &'a str,
    label: &'a str,
}

impl<'a> EnumVariant<'a> {
    fn new(
        item_ty: TypeId,
        payload: TypeId,
        enum_ident: &'a str,
        variant_ident: &'a str,
        label: &'a str,
    ) -> Self {
        Self {
            item_ty,
            payload,
            enum_ident,
            variant_ident,
            label,
        }
    }

    fn has_payload(&self) -> bool {
        self.payload != TYPE_VOID
    }
}

impl Emitter<'_> {
    pub(super) fn serde_impl(&mut self, item: &Item) -> String {
        let rt = self.config.rt_crate.clone();
        let ident = self.item_ident(item.name).to_string();
        let args = if self.lifetime_args(item.ty).is_empty() {
            ""
        } else {
            "<'_>"
        };

        let body = match item.kind {
            ItemKind::Struct => self.struct_body(item),
            ItemKind::Enum => self.enum_body(item, &ident),
            _ => unreachable!("serde impls are generated for structs and enums only"),
        };

        // Only payload fields thread the source through; a tags-only enum
        // never touches it and must not bind it, or the impl warns.
        let source = if body.contains("source") {
            "source"
        } else {
            "_source"
        };

        format!(
            "impl {rt}::SerializeWithSource for {ident}{args} {{
    fn serialize_with_source<S>(
        &self,
        {source}: &str,
        serializer: S,
    ) -> ::core::result::Result<S::Ok, S::Error>
    where
        S: {rt}::serde::Serializer,
    {{
        use {rt}::serde::ser::SerializeMap as _;

{body}    }}
}}"
        )
    }

    fn struct_body(&mut self, item: &Item) -> String {
        let types = self.types;
        let interner = self.interner;
        let rt = self.config.rt_crate.clone();
        let TypeShape::Struct(fields) = types.expect_type_shape(item.ty) else {
            unreachable!("struct item must have a struct shape");
        };
        let field_idents = scope_idents(fields.keys().map(|&sym| interner.resolve(sym)));

        let mut out = format!(
            "        let mut map = serializer.serialize_map(Some({}))?;\n",
            fields.len()
        );
        for ((&name_sym, _), field_ident) in fields.iter().zip(&field_idents) {
            let key = interner.resolve(name_sym);
            writeln!(
                out,
                "        map.serialize_entry({key:?}, &{rt}::WithSource::new(&self.{field_ident}, source))?;"
            )
            .expect("writing to a String is infallible");
        }
        out.push_str("        map.end()\n");
        out
    }

    fn enum_body(&mut self, item: &Item, ident: &str) -> String {
        let types = self.types;
        let interner = self.interner;
        let TypeShape::Enum(variants) = types.expect_type_shape(item.ty) else {
            unreachable!("enum item must have an enum shape");
        };
        let variant_idents = scope_idents(variants.keys().map(|&sym| interner.resolve(sym)));

        let mut out = String::from("        match self {\n");
        for ((&label_sym, &payload), variant_ident) in variants.iter().zip(&variant_idents) {
            let label = interner.resolve(label_sym).to_owned();
            let variant = EnumVariant::new(item.ty, payload, ident, variant_ident, &label);
            let arm = if variant.has_payload() {
                self.payload_arm(&variant)
            } else {
                unit_arm(&variant)
            };
            out.push_str(&arm);
        }
        out.push_str("        }\n");
        out
    }

    fn payload_arm(&mut self, variant: &EnumVariant<'_>) -> String {
        let types = self.types;
        let interner = self.interner;
        let rt = self.config.rt_crate.clone();
        let TypeShape::Struct(fields) = types.expect_type_shape(variant.payload) else {
            unreachable!("enum variant payload is void or an anonymous struct");
        };
        let field_idents = scope_idents(fields.keys().map(|&sym| interner.resolve(sym)));
        let payload_lt = fields
            .values()
            .any(|info| self.facts.needs_lifetime(info.type_id));
        let (decl_lt, impl_lt) = if payload_lt {
            ("<'a, 't>", "<'_, '_>")
        } else {
            ("<'a>", "<'_>")
        };

        let mut data_fields = String::new();
        let mut data_entries = String::new();
        for (index, (&name_sym, info)) in fields.iter().enumerate() {
            // The helper borrows the enum's actual field, so its type must be
            // spelled with the declaration's own cut context.
            let field_ty = self.field_type(TypeContext::item(variant.item_ty), info);
            writeln!(data_fields, "                    v{index}: &'a {field_ty},")
                .expect("writing to a String is infallible");
            let key = interner.resolve(name_sym);
            writeln!(
                data_entries,
                "                        map.serialize_entry({key:?}, &{rt}::WithSource::new(self.v{index}, self.source))?;"
            )
            .expect("writing to a String is infallible");
        }

        let bindings: Vec<String> = field_idents
            .iter()
            .enumerate()
            .map(|(index, field_ident)| format!("{field_ident}: v{index}"))
            .collect();
        let binding_list = bindings.join(", ");
        let data_inits: Vec<String> = (0..fields.len()).map(|i| format!("v{i}")).collect();
        let data_inits = data_inits.join(", ");
        let field_count = fields.len();
        let ident = variant.enum_ident;
        let variant_ident = variant.variant_ident;
        let label = variant.label;

        format!(
            "            {ident}::{variant_ident} {{ {binding_list} }} => {{
                struct Data{decl_lt} {{
{data_fields}                    source: &'a str,
                }}
                impl {rt}::serde::Serialize for Data{impl_lt} {{
                    fn serialize<S>(
                        &self,
                        serializer: S,
                    ) -> ::core::result::Result<S::Ok, S::Error>
                    where
                        S: {rt}::serde::Serializer,
                    {{
                        use {rt}::serde::ser::SerializeMap as _;

                        let mut map = serializer.serialize_map(Some({field_count}))?;
{data_entries}                        map.end()
                    }}
                }}

                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry(\"$tag\", {label:?})?;
                map.serialize_entry(\"$data\", &Data {{ {data_inits}, source }})?;
                map.end()
            }}
"
        )
    }
}

fn unit_arm(variant: &EnumVariant<'_>) -> String {
    let ident = variant.enum_ident;
    let variant_ident = variant.variant_ident;
    let label = variant.label;
    format!(
        "            {ident}::{variant_ident} => {{
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry(\"$tag\", {label:?})?;
                map.end()
            }}
"
    )
}
