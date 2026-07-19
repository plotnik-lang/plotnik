//! TypeScript declarations rendered directly from target-neutral output facts.

use std::collections::{HashMap, HashSet};

use crate::compiler::analyze::result::{
    CaptureMemberKind, CaptureScopeKind, ResultItem, ResultItemKind, ResultSchema,
};
use crate::compiler::analyze::types::type_shape::{
    CasePayload, DefinitionOutput, ListMinimum, RecordField, TypeId, TypeShape,
};
use crate::compiler::emit::sink::{Sink, Style};
use crate::compiler::ids::{ResultMemberId, ResultTypeId};
use crate::core::Symbol;

use super::TypeScriptBinding;
use super::config::{Config, MatchOnlyType};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SemanticTag {
    type_id: ResultTypeId,
    member: Option<ResultMemberId>,
}

pub(crate) fn emit_schema(schema: &ResultSchema<'_>, config: Config) -> String {
    SchemaEmitter::new(schema, config).emit().0
}

pub(crate) fn emit_schema_mapped(
    schema: &ResultSchema<'_>,
    config: Config,
) -> (String, Vec<TypeScriptBinding>) {
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
    schema: &'a ResultSchema<'a>,
    config: Config,
    items_by_name: HashMap<Symbol, ResultItem>,
    declared_names: HashSet<String>,
    needs_node_type: bool,
    sink: Sink<SemanticTag>,
    map_enabled: bool,
}

impl<'a> SchemaEmitter<'a> {
    fn new(schema: &'a ResultSchema<'a>, config: Config) -> Self {
        let items_by_name = schema
            .entry_point_items()
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

    fn emit(mut self) -> (String, Vec<TypeScriptBinding>) {
        let items = self.schema.entry_point_items().to_vec();
        self.needs_node_type = items
            .iter()
            .filter_map(|item| item.output.value())
            .any(|type_id| self.type_uses_node(type_id, &mut HashSet::new()));
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
            .map(|range| TypeScriptBinding {
                span: (
                    u32::try_from(range.start).expect("TypeScript span start fits in u32"),
                    u32::try_from(range.end).expect("TypeScript span end fits in u32"),
                ),
                type_id: range.tag.type_id.raw(),
                member_id: range.tag.member.map(ResultMemberId::raw),
            })
            .collect();
        (output, ranges)
    }

    fn type_uses_node(&self, ty: TypeId, seen: &mut HashSet<TypeId>) -> bool {
        if !seen.insert(ty) {
            return false;
        }
        match self.schema.types.expect_type_shape(ty) {
            TypeShape::Node => true,
            TypeShape::Ref(declaration) => self
                .schema
                .types
                .declaration_body(*declaration)
                .is_none_or(|target| self.type_uses_node(target, seen)),
            shape => shape
                .child_type_ids()
                .any(|child| self.type_uses_node(child, seen)),
        }
    }

    fn emit_item(&mut self, item: ResultItem) {
        let name = self.name(item.name);
        if !self.declared_names.insert(name.clone()) {
            return;
        }
        match item.kind {
            ResultItemKind::Record => self.emit_interface(&name, item.value_type()),
            ResultItemKind::Variant => self.emit_variant(&name, item.value_type()),
            ResultItemKind::Alias => {
                let body = self.render_shape(item.value_type());
                self.emit_type_decl(&name, item.output, body);
            }
            ResultItemKind::MatchOnlyDef => {
                let body = match self.config.match_only_type {
                    MatchOnlyType::Undefined => text("undefined"),
                    MatchOnlyType::Null => text("null"),
                };
                self.emit_type_decl(&name, item.output, body);
            }
        }
    }

    fn emit_type_decl(&mut self, name: &str, output: DefinitionOutput, body: Sink<SemanticTag>) {
        emit_export(&mut self.sink, self.config.export);
        self.sink.styled(Style::Dim, "type");
        self.sink.push(" ");
        self.sink.set_style(Style::Blue);
        self.push_mapped_output(name, output);
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

        for (field, info, member) in self.record_members(ty) {
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

        let cases = self.variant_members(ty);
        let last = cases.len().saturating_sub(1);
        for (position, (case, payload, member)) in cases.into_iter().enumerate() {
            let rendered = self.render_variant(
                &case,
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
            && let Some(symbol) = self.schema.types.type_name_of(ty)
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
            TypeShape::Node => text("Node"),
            TypeShape::Text => self.render_builtin("string", ty),
            TypeShape::Bool => self.render_builtin("boolean", ty),
            TypeShape::Option(inner) => self.render_nullable(*inner),
            TypeShape::List { element, minimum } => self.render_array(*element, *minimum),
            TypeShape::Ref(declaration) => {
                if self.schema.types.declaration_body(*declaration).is_none() {
                    return text("Node");
                }
                let name = self.schema.types.declaration_name(*declaration);
                let mut out = Sink::new();
                out.styled(Style::Blue, &self.name(name));
                out
            }
            TypeShape::Record(_) => self.inline_record(ty, false),
            TypeShape::Variant(_) => self.inline_variant(ty),
        }
    }

    fn render_nullable(&self, inner: TypeId) -> Sink<SemanticTag> {
        let mut out = self.render_ty(inner);
        out.push(" ");
        out.styled(Style::Dim, "|");
        out.push(" null");
        out
    }

    fn render_array(&self, element: TypeId, minimum: ListMinimum) -> Sink<SemanticTag> {
        if minimum == ListMinimum::Zero {
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
        let Some(scope) = self.schema.layout().scope(ty) else {
            return self.render_ty(ty);
        };
        if scope.kind() != CaptureScopeKind::Record {
            return self.render_ty(ty);
        }
        let fields = self.record_members(ty);
        if fields.is_empty() {
            let mut out = Sink::new();
            out.styled(Style::Dim, "{}");
            return out;
        }
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

    fn inline_variant(&self, ty: TypeId) -> Sink<SemanticTag> {
        let mut out = Sink::new();
        for (position, (name, payload, _)) in self.variant_members(ty).into_iter().enumerate() {
            if position > 0 {
                out.push(" ");
                out.styled(Style::Dim, "|");
                out.push(" ");
            }
            out.append(self.render_variant(&name, payload, None, false));
        }
        out
    }

    fn record_members(&self, ty: TypeId) -> Vec<(String, RecordField, ResultMemberId)> {
        let scope = self
            .schema
            .layout()
            .scope(ty)
            .expect("record output has a capture scope");
        assert_eq!(
            scope.kind(),
            CaptureScopeKind::Record,
            "record output must have a record capture scope"
        );
        let mut fields = scope
            .members()
            .map(|member| {
                let descriptor = self.schema.layout().expect_member(member);
                let CaptureMemberKind::Field(info) = descriptor.kind else {
                    unreachable!("record capture scope contains only field members");
                };
                (self.name(descriptor.name), info, member)
            })
            .collect::<Vec<_>>();
        fields.sort_by(|left, right| left.0.cmp(&right.0));
        fields
    }

    fn variant_members(&self, ty: TypeId) -> Vec<(String, CasePayload, ResultMemberId)> {
        let scope = self
            .schema
            .layout()
            .scope(ty)
            .expect("variant output has a capture scope");
        assert_eq!(
            scope.kind(),
            CaptureScopeKind::Variant,
            "variant output must have a variant capture scope"
        );
        scope
            .members()
            .map(|member| {
                let descriptor = self.schema.layout().expect_member(member);
                let CaptureMemberKind::Case(payload) = descriptor.kind else {
                    unreachable!("variant capture scope contains only case members");
                };
                (self.name(descriptor.name), payload, member)
            })
            .collect()
    }

    fn render_variant(
        &self,
        name: &str,
        payload: CasePayload,
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
        let Some(payload) = payload.type_id() else {
            out.push(" ");
            out.styled(Style::Dim, "}");
            return out;
        };
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
        emit_node_field(&mut self.sink, "span", span_type());
        if self.config.include_points {
            emit_node_field(&mut self.sink, "startPoint", point_type());
            emit_node_field(&mut self.sink, "endPoint", point_type());
        }
        self.sink.set_style(Style::Dim);
        self.sink.push("}");
        self.sink.reset_style();
        self.sink.push("\n\n");
    }

    fn push_mapped(&mut self, value: &str, ty: TypeId, member: Option<ResultMemberId>) {
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

    fn push_mapped_output(&mut self, value: &str, output: DefinitionOutput) {
        if !self.map_enabled {
            self.sink.push(value);
            return;
        }
        let type_id = match output {
            DefinitionOutput::MatchOnly => self.schema.type_layout().no_value_output_id(),
            DefinitionOutput::Value(type_id) => self.wire_id(type_id),
        };
        self.sink.tagged(
            SemanticTag {
                type_id,
                member: None,
            },
            |sink| sink.push(value),
        );
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

    fn wire_id(&self, ty: TypeId) -> ResultTypeId {
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

fn span_type() -> Sink<SemanticTag> {
    let mut out = Sink::new();
    out.styled(Style::Dim, "[");
    out.push("number");
    out.styled(Style::Dim, ", ");
    out.push("number");
    out.styled(Style::Dim, "]");
    out
}

fn point_type() -> Sink<SemanticTag> {
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
