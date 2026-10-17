use crate::compiler::emit::sink::Sink;

use super::TypesConfig as Config;
use super::model::{CaseModel, FieldModel, ItemModel, ItemShape, ResultModel};

const DERIVES: &str = "#[derive(Clone, Copy)]";

/// Rust declarations of the result item types.
///
/// Rows are shallow `Copy` values: a list is a slice and a recursive value is
/// a reference, both into the owning match's storage (`'r`). Nodes keep the
/// tree lifetime `'t` and text keeps the source lifetime `'s`.
pub(crate) struct Emitter<'m> {
    model: &'m ResultModel,
    config: &'m Config,
}

impl<'m> Emitter<'m> {
    pub(super) fn new(model: &'m ResultModel, config: &'m Config) -> Self {
        Self { model, config }
    }

    pub(super) fn emit(self) -> String {
        let sections: Vec<String> = self
            .model
            .items()
            .iter()
            .map(|item| self.render_item(item))
            .collect();
        let mut out = Sink::new();
        if self.uses_node() {
            let rt = &self.config.rt_crate;
            out.push(&format!("use {rt}::Node;\n\n"));
        }
        out.push(&sections.join("\n\n"));
        out.push("\n");
        out.plain().to_string()
    }

    fn uses_node(&self) -> bool {
        self.model
            .items()
            .iter()
            .any(|item| self.model.item_usage(item.name).tree)
    }

    fn render_item(&self, item: &ItemModel) -> String {
        let params = self.model.item_usage(item.name).params("'r");
        let ident = &item.ident;
        match &item.shape {
            ItemShape::Record(fields) => self.render_struct(ident, &params, fields),
            ItemShape::Variant(cases) => self.render_enum(ident, &params, cases),
            ItemShape::Alias(body) => {
                format!(
                    "pub type {ident}{params} = {};",
                    self.model.rust_type(*body, "'r")
                )
            }
            ItemShape::MatchOnly => format!("{DERIVES}\npub struct {ident};"),
        }
    }

    fn render_struct(&self, ident: &str, params: &str, fields: &[FieldModel]) -> String {
        let mut out = Sink::new();
        out.line(DERIVES);
        out.line(&format!("pub struct {ident}{params} {{"));
        out.indented(|out| {
            for field in fields {
                let ty = self.model.rust_type(field.ty, "'r");
                out.line(&format!("pub {}: {ty},", field.ident));
            }
        });
        out.push("}");
        out.plain().to_string()
    }

    fn render_enum(&self, ident: &str, params: &str, cases: &[CaseModel]) -> String {
        let payloads: Vec<Option<Vec<String>>> = cases
            .iter()
            .map(|case| {
                let (_, fields) = case.payload.as_ref()?;
                Some(
                    fields
                        .iter()
                        .map(|field| {
                            format!("{}: {}", field.ident, self.model.rust_type(field.ty, "'r"))
                        })
                        .collect(),
                )
            })
            .collect();
        let expand_payloads = payloads
            .iter()
            .flatten()
            .any(|fields| fields.join(", ").len() > 35);

        let mut out = Sink::new();
        out.line(DERIVES);
        out.line(&format!("pub enum {ident}{params} {{"));
        out.indented(|out| {
            for (fields, case) in payloads.iter().zip(cases) {
                let variant = &case.ident;
                let Some(fields) = fields else {
                    out.line(&format!("{variant},"));
                    continue;
                };
                if !expand_payloads {
                    out.line(&format!("{variant} {{ {} }},", fields.join(", ")));
                    continue;
                }
                out.line(&format!("{variant} {{"));
                out.indented(|out| {
                    for field in fields {
                        out.line(&format!("{field},"));
                    }
                });
                out.line("},");
            }
        });
        out.push("}");
        out.plain().to_string()
    }
}
