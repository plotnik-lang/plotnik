//! TypeScript declarations rendered directly from target-neutral output facts.

use std::collections::{HashMap, HashSet};

use crate::compiler::analyze::output::{OutputItem, OutputItemKind, OutputSchema};
use crate::compiler::analyze::types::type_shape::{TYPE_VOID, TypeId, TypeShape};
use crate::compiler::emit::sink::{Sink, Style};
use crate::core::Symbol;

use super::DtsRange;
use super::config::{Config, VoidType};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SemanticTag {
    type_id: u32,
    member: Option<u16>,
}

pub(crate) fn emit_schema(schema: &OutputSchema<'_>, config: Config) -> String {
    SchemaEmitter::new(schema, config).emit().0
}

pub(crate) fn emit_schema_mapped(
    schema: &OutputSchema<'_>,
    config: Config,
) -> (String, Vec<DtsRange>) {
    assert!(
        config.colors.blue.is_empty()
            && config.colors.green.is_empty()
            && config.colors.dim.is_empty()
            && config.colors.reset.is_empty(),
        "mapped TypeScript emission requires colors off"
    );
    SchemaEmitter::new(schema, config).mapped().emit()
}

struct SchemaEmitter<'a> {
    schema: &'a OutputSchema<'a>,
    config: Config,
    items_by_name: HashMap<Symbol, OutputItem>,
    declared_names: HashSet<String>,
    needs_node_type: bool,
    sink: Sink<SemanticTag>,
    map_enabled: bool,
}

impl<'a> SchemaEmitter<'a> {
    fn new(schema: &'a OutputSchema<'a>, config: Config) -> Self {
        let items_by_name = schema
            .entrypoint_items()
            .iter()
            .map(|item| (item.name, *item))
            .collect();
        Self {
            schema,
            config,
            items_by_name,
            declared_names: HashSet::new(),
            needs_node_type: false,
            sink: Sink::new(),
            map_enabled: false,
        }
    }

    fn mapped(mut self) -> Self {
        self.map_enabled = true;
        self
    }

    fn emit(mut self) -> (String, Vec<DtsRange>) {
        let items = self.schema.entrypoint_items().to_vec();
        self.needs_node_type = items
            .iter()
            .any(|item| self.type_uses_node(item.ty, &mut HashSet::new()));
        if self.config.emit_node_interface && self.needs_node_type {
            self.emit_node_interface();
        }
        for item in items {
            self.emit_item(item);
        }

        let mut output = if self.map_enabled {
            self.sink.plain().to_string()
        } else {
            self.sink.render(self.config.colors)
        };
        output.truncate(output.trim_end().len());
        output.push('\n');
        let ranges = self
            .sink
            .tags()
            .iter()
            .map(|range| DtsRange {
                start: u32::try_from(range.start).expect("d.ts range start fits in u32"),
                end: u32::try_from(range.end).expect("d.ts range end fits in u32"),
                type_id: range.tag.type_id,
                member: range.tag.member,
            })
            .collect();
        (output, ranges)
    }

    fn type_uses_node(&self, ty: TypeId, seen: &mut HashSet<TypeId>) -> bool {
        if !seen.insert(ty) {
            return false;
        }
        match self.schema.types.expect_type_shape(ty) {
            TypeShape::Node | TypeShape::Custom(_) => true,
            TypeShape::Ref(definition) => {
                let target = self.schema.types.expect_def_output(*definition);
                target == TYPE_VOID || self.type_uses_node(target, seen)
            }
            shape => shape
                .child_type_ids()
                .any(|child| self.type_uses_node(child, seen)),
        }
    }

    fn emit_item(&mut self, item: OutputItem) {
        let name = self.name(item.name);
        if !self.declared_names.insert(name.clone()) {
            return;
        }
        match item.kind {
            OutputItemKind::Record => self.emit_interface(&name, item.ty),
            OutputItemKind::Variant => self.emit_variant(&name, item.ty),
            OutputItemKind::Alias | OutputItemKind::VoidDef => {
                let body = self.render_shape(item.ty);
                self.emit_type_decl(&name, item.ty, body);
            }
        }
    }

    fn emit_type_decl(&mut self, name: &str, ty: TypeId, body: Sink<SemanticTag>) {
        emit_export(&mut self.sink, self.config.export);
        self.sink.styled(Style::Dim, "type");
        self.sink.push(" ");
        self.sink.set_style(Style::Blue);
        self.push_mapped(name, ty, None);
        self.sink.reset_style();
        self.sink.push(" ");
        self.sink.styled(Style::Dim, "=");
        self.sink.push(" ");
        self.sink.append(body);
        self.sink.set_style(Style::Dim);
        self.sink.push(";\n\n");
        self.sink.reset_style();
    }

    fn emit_interface(&mut self, name: &str, ty: TypeId) {
        emit_export(&mut self.sink, self.config.export);
        self.sink.styled(Style::Dim, "interface");
        self.sink.push(" ");
        self.sink.set_style(Style::Blue);
        self.push_mapped(name, ty, None);
        self.sink.reset_style();
        self.sink.push(" ");
        self.sink.set_style(Style::Dim);
        self.sink.push("{\n");

        let TypeShape::Record(fields) = self.schema.types.expect_type_shape(ty) else {
            unreachable!("record output item has a record shape");
        };
        let scope = self
            .schema
            .layout()
            .scope(ty)
            .expect("record output has a capture scope");
        let mut fields = fields
            .iter()
            .enumerate()
            .map(|(index, (&symbol, info))| {
                (self.name(symbol), *info, scope.absolute_index(index as u16))
            })
            .collect::<Vec<_>>();
        fields.sort_by(|left, right| left.0.cmp(&right.0));
        for (field, info, member) in fields {
            let value = self.render_ty(info.final_type);
            self.sink.reset_style();
            self.sink.push("  ");
            self.push_mapped(&field, ty, Some(member));
            self.sink.set_style(Style::Dim);
            self.sink.push(":");
            self.sink.reset_style();
            self.sink.push(" ");
            self.sink.append(value);
            self.sink.set_style(Style::Dim);
            self.sink.push(";\n");
        }
        self.sink.set_style(Style::Dim);
        self.sink.push("}");
        self.sink.reset_style();
        self.sink.push("\n\n");
    }

    fn emit_variant(&mut self, name: &str, ty: TypeId) {
        emit_export(&mut self.sink, self.config.export);
        self.sink.styled(Style::Dim, "type");
        self.sink.push(" ");
        self.sink.set_style(Style::Blue);
        self.push_mapped(name, ty, None);
        self.sink.reset_style();
        self.sink.push(" ");
        self.sink.styled(Style::Dim, "=");
        self.sink.push("\n");

        let TypeShape::Variant(cases) = self.schema.types.expect_type_shape(ty) else {
            unreachable!("variant output item has a variant shape");
        };
        let scope = self
            .schema
            .layout()
            .scope(ty)
            .expect("variant output has a capture scope");
        let last = cases.len().saturating_sub(1);
        for (position, (&symbol, &payload)) in cases.iter().enumerate() {
            let member = scope.absolute_index(position as u16);
            let rendered = self.render_variant(
                &self.name(symbol),
                payload,
                self.map_enabled.then_some(SemanticTag {
                    type_id: self.wire_id(ty),
                    member: Some(member),
                }),
                self.map_enabled,
            );
            self.sink.push("  ");
            self.sink.styled(Style::Dim, "|");
            self.sink.push(" ");
            self.sink.append(rendered);
            self.sink.set_style(Style::Dim);
            if position == last {
                self.sink.push(";");
            }
            self.sink.reset_style();
            self.sink.push("\n");
        }
        self.sink.push("\n");
    }

    fn render_ty(&self, ty: TypeId) -> Sink<SemanticTag> {
        if !ty.is_builtin()
            && let Some(symbol) = self.schema.type_name_of(ty)
            && self.items_by_name.contains_key(&symbol)
        {
            let mut out = Sink::new();
            out.styled(Style::Blue, &self.name(symbol));
            return out;
        }
        self.render_shape(ty)
    }

    fn render_shape(&self, ty: TypeId) -> Sink<SemanticTag> {
        match self.schema.types.expect_type_shape(ty) {
            TypeShape::Void => match self.config.void_type {
                VoidType::Undefined => text("undefined"),
                VoidType::Null => text("null"),
            },
            TypeShape::Node | TypeShape::Custom(_) => text("Node"),
            TypeShape::Text => self.render_builtin("string", ty),
            TypeShape::Bool => self.render_builtin("boolean", ty),
            TypeShape::Option(inner) => self.render_nullable(*inner),
            TypeShape::Array { element, non_empty } => self.render_array(*element, *non_empty),
            TypeShape::Ref(definition) => {
                let target = self.schema.types.expect_def_output(*definition);
                if target == TYPE_VOID {
                    return text("Node");
                }
                let name = self.schema.deps.def_name_sym(*definition);
                let mut out = Sink::new();
                out.styled(Style::Blue, &self.name(name));
                out
            }
            TypeShape::Record(_) => self.inline_record(ty, false),
            TypeShape::Variant(cases) => {
                let mut out = Sink::new();
                for (position, (&name, &payload)) in cases.iter().enumerate() {
                    if position > 0 {
                        out.push(" ");
                        out.styled(Style::Dim, "|");
                        out.push(" ");
                    }
                    out.append(self.render_variant(&self.name(name), payload, None, false));
                }
                out
            }
        }
    }

    fn render_nullable(&self, inner: TypeId) -> Sink<SemanticTag> {
        let mut out = self.render_ty(inner);
        out.push(" ");
        out.styled(Style::Dim, "|");
        out.push(" null");
        out
    }

    fn render_array(&self, element: TypeId, non_empty: bool) -> Sink<SemanticTag> {
        if !non_empty {
            let mut out = self.render_ty(element);
            out.styled(Style::Dim, "[]");
            return out;
        }
        let mut out = Sink::new();
        out.styled(Style::Dim, "[");
        out.append(self.render_ty(element));
        out.styled(Style::Dim, ", ...");
        out.append(self.render_ty(element));
        out.styled(Style::Dim, "[]]");
        out
    }

    fn inline_record(&self, ty: TypeId, tags: bool) -> Sink<SemanticTag> {
        let TypeShape::Record(fields) = self.schema.types.expect_type_shape(ty) else {
            return self.render_ty(ty);
        };
        if fields.is_empty() {
            let mut out = Sink::new();
            out.styled(Style::Dim, "{}");
            return out;
        }
        let scope = self
            .schema
            .layout()
            .scope(ty)
            .expect("inline record has a capture scope");
        let mut fields = fields
            .iter()
            .enumerate()
            .map(|(index, (&symbol, info))| {
                (self.name(symbol), *info, scope.absolute_index(index as u16))
            })
            .collect::<Vec<_>>();
        fields.sort_by(|left, right| left.0.cmp(&right.0));
        let mut out = Sink::new();
        out.styled(Style::Dim, "{");
        out.push(" ");
        let last = fields.len() - 1;
        for (position, (name, info, member)) in fields.into_iter().enumerate() {
            if tags {
                out.tagged(
                    SemanticTag {
                        type_id: self.wire_id(ty),
                        member: Some(member),
                    },
                    |out| out.push(&name),
                );
            } else {
                out.push(&name);
            }
            out.set_style(Style::Dim);
            out.push(":");
            out.reset_style();
            out.push(" ");
            out.append(self.render_ty(info.final_type));
            if position != last {
                out.set_style(Style::Dim);
                out.push("; ");
            }
        }
        out.push(" ");
        out.styled(Style::Dim, "}");
        out
    }

    fn render_variant(
        &self,
        name: &str,
        payload: TypeId,
        tag: Option<SemanticTag>,
        payload_tags: bool,
    ) -> Sink<SemanticTag> {
        let mut out = Sink::new();
        out.styled(Style::Dim, "{");
        out.push(" $tag");
        out.styled(Style::Dim, ":");
        out.push(" ");
        out.set_style(Style::Green);
        out.push("\"");
        if let Some(tag) = tag {
            out.tagged(tag, |out| out.push(name));
        } else {
            out.push(name);
        }
        out.push("\"");
        out.reset_style();
        if payload == TYPE_VOID {
            out.push(" ");
            out.styled(Style::Dim, "}");
            return out;
        }
        out.set_style(Style::Dim);
        out.push("; $data");
        out.set_style(Style::Dim);
        out.push(":");
        out.reset_style();
        out.push(" ");
        out.append(self.inline_record(payload, payload_tags));
        out.push(" ");
        out.styled(Style::Dim, "}");
        out
    }

    fn emit_node_interface(&mut self) {
        emit_export(&mut self.sink, self.config.export);
        self.sink.styled(Style::Dim, "interface");
        self.sink.push(" ");
        self.sink.styled(Style::Blue, "Node");
        self.sink.push(" ");
        self.sink.set_style(Style::Dim);
        self.sink.push("{\n");
        emit_node_field(&mut self.sink, "kind", text("string"));
        emit_node_field(&mut self.sink, "text", text("string"));
        self.sink.reset_style();
        self.sink.push("  span");
        self.sink.styled(Style::Dim, ":");
        self.sink.push(" ");
        self.sink.styled(Style::Dim, "[");
        self.sink.push("number");
        self.sink.styled(Style::Dim, ", ");
        self.sink.push("number");
        self.sink.set_style(Style::Dim);
        self.sink.push("]");
        self.sink.set_style(Style::Dim);
        self.sink.push(";\n");
        if self.config.verbose_nodes {
            emit_node_field(&mut self.sink, "startPosition", position_type());
            emit_node_field(&mut self.sink, "endPosition", position_type());
        }
        self.sink.set_style(Style::Dim);
        self.sink.push("}");
        self.sink.reset_style();
        self.sink.push("\n\n");
    }

    fn push_mapped(&mut self, value: &str, ty: TypeId, member: Option<u16>) {
        if !self.map_enabled {
            self.sink.push(value);
            return;
        }
        let tag = SemanticTag {
            type_id: self.wire_id(ty),
            member,
        };
        self.sink.tagged(tag, |sink| sink.push(value));
    }

    fn render_builtin(&self, value: &str, ty: TypeId) -> Sink<SemanticTag> {
        let mut out = Sink::new();
        if !self.map_enabled {
            out.push(value);
            return out;
        }
        out.tagged(
            SemanticTag {
                type_id: self.wire_id(ty),
                member: None,
            },
            |out| out.push(value),
        );
        out
    }

    fn wire_id(&self, ty: TypeId) -> u32 {
        self.schema.type_layout().output_id(ty)
    }

    fn name(&self, symbol: Symbol) -> String {
        self.schema.interner.resolve(symbol).to_string()
    }
}

fn emit_export(sink: &mut Sink<SemanticTag>, enabled: bool) {
    if enabled {
        sink.styled(Style::Dim, "export");
        sink.push(" ");
    }
}

fn emit_node_field(sink: &mut Sink<SemanticTag>, name: &str, ty: Sink<SemanticTag>) {
    sink.reset_style();
    sink.push("  ");
    sink.push(name);
    sink.styled(Style::Dim, ":");
    sink.push(" ");
    sink.append(ty);
    sink.set_style(Style::Dim);
    sink.push(";\n");
}

fn position_type() -> Sink<SemanticTag> {
    let mut out = Sink::new();
    out.styled(Style::Dim, "{");
    out.push(" row");
    out.styled(Style::Dim, ":");
    out.push(" number");
    out.set_style(Style::Dim);
    out.push("; column");
    out.set_style(Style::Dim);
    out.push(":");
    out.reset_style();
    out.push(" number");
    out.set_style(Style::Dim);
    out.push("; ");
    out.set_style(Style::Dim);
    out.push("}");
    out
}

fn text(value: &str) -> Sink<SemanticTag> {
    let mut out = Sink::new();
    out.push(value);
    out
}
