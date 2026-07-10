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
use crate::compiler::srcgen::names::rust_scope_idents;

use super::emitter::{Emitter, Item, ItemKind, TypeContext};

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
        let TypeShape::Struct(fields) = types.expect_type_shape(item.ty) else {
            unreachable!("struct item must have a struct shape");
        };
        let field_idents = rust_scope_idents(fields.keys().map(|&sym| interner.resolve(sym)));

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
        SerdeBody {
            code: out,
            uses_source: true,
        }
    }

    fn enum_body(&mut self, item: &Item, ident: &str) -> SerdeBody {
        let types = self.schema.types;
        let interner = self.schema.interner;
        let TypeShape::Enum(variants) = types.expect_type_shape(item.ty) else {
            unreachable!("enum item must have an enum shape");
        };
        let variant_idents = rust_scope_idents(variants.keys().map(|&sym| interner.resolve(sym)));

        let mut out = String::from("        match self {\n");
        let mut uses_source = false;
        for ((&label_sym, &payload), variant_ident) in variants.iter().zip(&variant_idents) {
            let label = interner.resolve(label_sym);
            let arm = if payload != TYPE_VOID {
                uses_source = true;
                self.payload_arm(item.ty, payload, ident, variant_ident, label)
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
        item_ty: TypeId,
        payload: TypeId,
        ident: &str,
        variant_ident: &str,
        label: &str,
    ) -> String {
        let types = self.schema.types;
        let interner = self.schema.interner;
        let rt = self.config.rt_crate.clone();
        let TypeShape::Struct(fields) = types.expect_type_shape(payload) else {
            unreachable!("enum variant payload is void or an anonymous struct");
        };
        let field_idents = rust_scope_idents(fields.keys().map(|&sym| interner.resolve(sym)));
        let (decl_generics, impl_generics) = if fields
            .values()
            .any(|info| self.facts.needs_lifetime(info.type_id))
        {
            ("<'a, 't>", "<'_, '_>")
        } else {
            ("<'a>", "<'_>")
        };

        let mut data_fields = String::new();
        let mut data_entries = String::new();
        let mut bindings = Vec::new();
        let mut data_inits = Vec::new();
        for (index, ((&name_sym, info), field_ident)) in
            fields.iter().zip(&field_idents).enumerate()
        {
            // The helper borrows the enum's actual field, so its type must be
            // spelled with the declaration's own cut context.
            let field_ty = self.field_type(TypeContext::item(item_ty), info);
            writeln!(data_fields, "                    v{index}: &'a {field_ty},")
                .expect("writing to a String is infallible");
            let key = interner.resolve(name_sym);
            writeln!(
                data_entries,
                "                        map.serialize_entry({key:?}, &{rt}::WithSource::new(self.v{index}, self.source))?;"
            )
            .expect("writing to a String is infallible");
            bindings.push(format!("{field_ident}: v{index}"));
            data_inits.push(format!("v{index}"));
        }

        let binding_list = bindings.join(", ");
        let data_inits = data_inits.join(", ");
        let field_count = fields.len();
        format!(
            "            {ident}::{variant_ident} {{ {binding_list} }} => {{
                struct Data{} {{
{data_fields}                    source: &'a str,
                }}
                impl {rt}::serde::Serialize for Data{} {{
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
