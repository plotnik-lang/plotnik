//! Generated `SerializeWithSource` impls.
//!
//! Mirrors the VM's materialized-value JSON exactly — record fields in
//! declaration order, `None` as one flat null, nodes as
//! `{kind, text, span}`, variants as `{"$tag": ...}` / `{"$tag", "$data"}` —
//! so serialized generated output can be diffed against VM output verbatim.
//! Serialized keys and tags always use the original query-side names, even
//! when the Rust identifier had to be keyword-renamed.
//!
//! Rust enum variant payloads are anonymous records, so each payload arm defines a
//! local `Data` adapter borrowing the matched fields; bindings are positional
//! (`v0`, `v1`, ...) to keep a capture literally named `source` from
//! shadowing the source parameter.

use std::fmt::Write as _;

use crate::compiler::analyze::types::type_shape::{TypeId, TypeShape};
use crate::compiler::emit::targets::rust::ident::rust_scope_idents;

use super::type_model::TypeContext;
use super::types::{Emitter, Item, ItemKind};

struct SerdeBody {
    code: String,
    uses_source: bool,
}

impl SerdeBody {
    fn source_param(&self) -> &'static str {
        if self.uses_source {
            "source"
        } else {
            "_source"
        }
    }
}

impl Emitter<'_, '_> {
    pub(super) fn serde_impl(&mut self, item: &Item) -> String {
        let rt = self.config.rt_crate.clone();
        let ident = self.item_ident(item.name).to_string();
        let usage = self.lifetime_usage(item.value_type());
        let args = match (usage.tree, usage.source) {
            (false, false) => "",
            (true, false) | (false, true) => "<'_>",
            (true, true) => "<'_, '_>",
        };

        let body = match item.kind {
            ItemKind::Record => self.struct_body(item),
            ItemKind::Variant => self.enum_body(item, &ident),
            _ => unreachable!("serde impls are generated for structs and enums only"),
        };

        // Only payload fields thread the source through; a no-payload enum
        // never touches it and must not bind it, or the impl warns.
        let source = body.source_param();
        let body = body.code;

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

    fn struct_body(&mut self, item: &Item) -> SerdeBody {
        let types = self.schema.types;
        let interner = self.schema.interner;
        let rt = self.config.rt_crate.clone();
        let TypeShape::Record(fields) = types.expect_type_shape(item.value_type()) else {
            unreachable!("struct item must have a record shape");
        };
        let field_idents = rust_scope_idents(fields.keys().map(|&sym| interner.resolve(sym)));

        let mut out = format!(
            "        let mut map = serializer.serialize_map(Some({}))?;\n",
            fields.len()
        );
        for (&name_sym, field_ident) in fields.keys().zip(&field_idents) {
            let key = interner.resolve(name_sym);
            let value = format!("&{rt}::WithSource::new(&self.{field_ident}, source)");
            serialize_entry(&mut out, "        ", key, &value);
        }
        out.push_str("        map.end()\n");
        SerdeBody {
            code: out,
            uses_source: true,
        }
    }

    fn enum_body(&mut self, item: &Item, ident: &str) -> SerdeBody {
        let types = self.schema.types;
        let interner = self.schema.interner;
        let TypeShape::Variant(variants) = types.expect_type_shape(item.value_type()) else {
            unreachable!("Rust enum item must have a variant shape");
        };
        let variant_idents = rust_scope_idents(variants.keys().map(|&sym| interner.resolve(sym)));

        let mut out = String::from("        match self {\n");
        let mut uses_source = false;
        for ((&label_sym, &payload), variant_ident) in variants.iter().zip(&variant_idents) {
            let label = interner.resolve(label_sym);
            let arm = if let Some(payload) = payload.type_id() {
                uses_source = true;
                self.payload_arm(item, payload, variant_ident, label)
            } else {
                unit_arm(ident, variant_ident, label)
            };
            out.push_str(&arm);
        }
        out.push_str("        }\n");
        SerdeBody {
            code: out,
            uses_source,
        }
    }

    fn payload_arm(
        &mut self,
        item: &Item,
        payload: TypeId,
        variant_ident: &str,
        label: &str,
    ) -> String {
        let types = self.schema.types;
        let interner = self.schema.interner;
        let rt = self.config.rt_crate.clone();
        let ident = self.item_ident(item.name).to_string();
        let TypeShape::Record(fields) = types.expect_type_shape(payload) else {
            unreachable!("enum variant has no payload or an anonymous record payload");
        };
        let field_idents = rust_scope_idents(fields.keys().map(|&sym| interner.resolve(sym)));
        let usage = fields
            .values()
            .fold((false, false), |(tree, source), info| {
                let field = self.lifetime_usage(info.final_type);
                (tree || field.tree, source || field.source)
            });
        let (decl_generics, impl_generics) = match usage {
            (false, false) => ("<'a>", "<'_>"),
            (true, false) => ("<'a, 't>", "<'_, '_>"),
            (false, true) => ("<'a, 's>", "<'_, '_>"),
            (true, true) => ("<'a, 't, 's>", "<'_, '_, '_>"),
        };

        let mut data_fields = String::new();
        let mut serialized_entries = Vec::new();
        let mut bindings = Vec::new();
        let mut data_inits = Vec::new();
        for (index, ((&name_sym, info), field_ident)) in
            fields.iter().zip(&field_idents).enumerate()
        {
            // The helper borrows the enum's actual field, so its type must be
            // spelled with the declaration's own cut context.
            let field_ty = self.field_type(TypeContext::item(item.value_type()), info);
            writeln!(data_fields, "                    v{index}: &'a {field_ty},")
                .expect("writing to a String is infallible");
            let key = interner.resolve(name_sym);
            let value = format!("&{rt}::WithSource::new(self.v{index}, self.source)");
            serialized_entries.push((key, value));
            bindings.push(format!("{field_ident}: v{index}"));
            data_inits.push(format!("v{index}"));
        }

        let binding_list = bindings.join(", ");
        let data_inits = data_inits.join(", ");
        let field_count = fields.len();
        let mut data_entries = String::new();
        for (key, value) in serialized_entries {
            serialize_entry(&mut data_entries, "                        ", key, &value);
        }
        format!(
            "            {ident}::{variant_ident} {{ {binding_list} }} => {{
                struct Data{} {{
{data_fields}                    source: &'a str,
                }}
                impl {rt}::serde::Serialize for Data{} {{
                    fn serialize<S>(&self, serializer: S) -> ::core::result::Result<S::Ok, S::Error>
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
",
            decl_generics, impl_generics
        )
    }
}

fn unit_arm(ident: &str, variant_ident: &str, label: &str) -> String {
    format!(
        "            {ident}::{variant_ident} => {{
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry(\"$tag\", {label:?})?;
                map.end()
            }}
"
    )
}

fn serialize_entry(out: &mut String, indent: &str, key: &str, value: &str) {
    let compact = format!("{indent}map.serialize_entry({key:?}, {value})?;");
    if compact.len() <= 91 {
        writeln!(out, "{compact}").expect("writing to a String is infallible");
        return;
    }
    writeln!(
        out,
        "{indent}map.serialize_entry(\n{indent}    {key:?},\n{indent}    {value},\n{indent})?;"
    )
    .expect("writing to a String is infallible");
}
