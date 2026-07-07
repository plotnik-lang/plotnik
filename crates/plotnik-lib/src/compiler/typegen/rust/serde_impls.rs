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

use crate::compiler::analyze::types::type_shape::{FieldInfo, TYPE_VOID, TypeId, TypeShape};

use super::emitter::{Emitter, Item, ItemKind, TypeContext};
use super::idents::scope_idents;

struct SerdeBody {
    code: String,
    uses_source: bool,
}

impl SerdeBody {
    fn with_source(code: String) -> Self {
        Self {
            code,
            uses_source: true,
        }
    }

    fn without_source(code: String) -> Self {
        Self {
            code,
            uses_source: false,
        }
    }

    fn source_param(&self) -> &'static str {
        if self.uses_source {
            "source"
        } else {
            "_source"
        }
    }
}

struct EnumVariant<'a> {
    item_ty: TypeId,
    payload: TypeId,
    enum_ident: &'a str,
    variant_ident: &'a str,
    label: String,
}

struct EnumSerdeContext<'a> {
    item_ty: TypeId,
    ident: &'a str,
}

impl<'a> EnumSerdeContext<'a> {
    fn for_item(item: &Item, ident: &'a str) -> Self {
        Self {
            item_ty: item.ty,
            ident,
        }
    }

    fn variant(&self, payload: TypeId, variant_ident: &'a str, label: String) -> EnumVariant<'a> {
        EnumVariant {
            item_ty: self.item_ty,
            payload,
            enum_ident: self.ident,
            variant_ident,
            label,
        }
    }
}

impl EnumVariant<'_> {
    fn has_payload(&self) -> bool {
        self.payload != TYPE_VOID
    }
}

struct PayloadAdapterSignature {
    decl_generics: &'static str,
    impl_generics: &'static str,
}

impl PayloadAdapterSignature {
    fn from_fields<'a>(
        mut fields: impl Iterator<Item = &'a FieldInfo>,
        facts: &super::analysis::TypeFacts,
    ) -> Self {
        let needs_lifetime = fields.any(|info| facts.needs_lifetime(info.type_id));
        if needs_lifetime {
            return Self {
                decl_generics: "<'a, 't>",
                impl_generics: "<'_, '_>",
            };
        }
        Self {
            decl_generics: "<'a>",
            impl_generics: "<'_>",
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
        SerdeBody::with_source(out)
    }

    fn enum_body(&mut self, item: &Item, ident: &str) -> SerdeBody {
        let types = self.types;
        let interner = self.interner;
        let TypeShape::Enum(variants) = types.expect_type_shape(item.ty) else {
            unreachable!("enum item must have an enum shape");
        };
        let variant_idents = scope_idents(variants.keys().map(|&sym| interner.resolve(sym)));
        let enum_context = EnumSerdeContext::for_item(item, ident);

        let mut out = String::from("        match self {\n");
        let mut uses_source = false;
        for ((&label_sym, &payload), variant_ident) in variants.iter().zip(&variant_idents) {
            let variant = enum_context.variant(
                payload,
                variant_ident,
                interner.resolve(label_sym).to_owned(),
            );
            let arm = if variant.has_payload() {
                uses_source = true;
                self.payload_arm(&variant)
            } else {
                unit_arm(&variant)
            };
            out.push_str(&arm);
        }
        out.push_str("        }\n");
        if uses_source {
            SerdeBody::with_source(out)
        } else {
            SerdeBody::without_source(out)
        }
    }

    fn payload_arm(&mut self, variant: &EnumVariant<'_>) -> String {
        let types = self.types;
        let interner = self.interner;
        let rt = self.config.rt_crate.clone();
        let TypeShape::Struct(fields) = types.expect_type_shape(variant.payload) else {
            unreachable!("enum variant payload is void or an anonymous struct");
        };
        let field_idents = scope_idents(fields.keys().map(|&sym| interner.resolve(sym)));
        let sig = PayloadAdapterSignature::from_fields(fields.values(), &self.facts);

        let mut data_fields = String::new();
        let mut data_entries = String::new();
        let mut bindings = Vec::new();
        let mut data_inits = Vec::new();
        for (index, ((&name_sym, info), field_ident)) in
            fields.iter().zip(&field_idents).enumerate()
        {
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
            bindings.push(format!("{field_ident}: v{index}"));
            data_inits.push(format!("v{index}"));
        }

        let binding_list = bindings.join(", ");
        let data_inits = data_inits.join(", ");
        let field_count = fields.len();
        let ident = variant.enum_ident;
        let variant_ident = variant.variant_ident;
        let label = &variant.label;

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
            sig.decl_generics, sig.impl_generics
        )
    }
}

fn unit_arm(variant: &EnumVariant<'_>) -> String {
    let ident = variant.enum_ident;
    let variant_ident = variant.variant_ident;
    let label = &variant.label;
    format!(
        "            {ident}::{variant_ident} => {{
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry(\"$tag\", {label:?})?;
                map.end()
            }}
"
    )
}
