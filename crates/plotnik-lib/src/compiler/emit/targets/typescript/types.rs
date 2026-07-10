//! TypeScript declarations rendered directly from target-neutral output facts.

use std::collections::{BinaryHeap, HashMap, HashSet};

use crate::compiler::analyze::output::{OutputItem, OutputItemKind, OutputSchema};
use crate::compiler::analyze::types::type_shape::{TYPE_NODE, TYPE_VOID, TypeId, TypeShape};
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
    items_by_type: HashMap<TypeId, OutputItem>,
    items_by_name: HashMap<Symbol, OutputItem>,
    wire_ids: HashMap<TypeId, u32>,
    declared_names: HashSet<String>,
    needs_node_type: bool,
    sink: Sink<SemanticTag>,
    map_enabled: bool,
}

impl<'a> SchemaEmitter<'a> {
    fn new(schema: &'a OutputSchema<'a>, config: Config) -> Self {
        let items_by_type = schema
            .items()
            .iter()
            .filter(|item| item.kind != OutputItemKind::VoidDef)
            .map(|item| (item.ty, *item))
            .collect();
        let items_by_name = schema
            .items()
            .iter()
            .map(|item| (item.name, *item))
            .collect();
        Self {
            schema,
            config,
            items_by_type,
            items_by_name,
            wire_ids: wire_ids(schema),
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
        self.needs_node_type = self
            .schema
            .items()
            .iter()
            .any(|item| self.type_uses_node(item.ty, &mut HashSet::new()));
        if self.config.emit_node_interface && self.needs_node_type {
            self.emit_node_interface();
        }
        for item in self.sorted_items() {
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

    fn sorted_items(&self) -> Vec<OutputItem> {
        let mut reachable = HashSet::new();
        let mut seen = HashSet::new();
        for (_, ty) in self.schema.types.iter_entrypoint_output() {
            self.collect_reachable(ty, &mut reachable, &mut seen);
        }
        let items: HashMap<TypeId, OutputItem> = self
            .items_by_type
            .iter()
            .filter(|(ty, _)| reachable.contains(ty))
            .map(|(&ty, &item)| (ty, item))
            .collect();
        let mut deps: HashMap<TypeId, HashSet<TypeId>> = items
            .keys()
            .copied()
            .map(|ty| (ty, self.direct_deps(ty)))
            .collect();
        for values in deps.values_mut() {
            values.retain(|dep| items.contains_key(dep));
        }
        let mut reverse: HashMap<TypeId, Vec<TypeId>> = HashMap::new();
        for (&ty, values) in &deps {
            for &dependency in values {
                reverse.entry(dependency).or_default().push(ty);
            }
        }
        let mut ready: BinaryHeap<(u32, TypeId)> = deps
            .iter()
            .filter_map(|(&ty, values)| values.is_empty().then_some((self.wire_id(ty), ty)))
            .collect();
        let mut output = Vec::with_capacity(items.len());
        while output.len() < items.len() {
            let ty = match ready.pop() {
                Some((_, ty)) => ty,
                None => deps
                    .values()
                    .flatten()
                    .copied()
                    .max_by_key(|ty| self.wire_id(*ty))
                    .expect("pending output items contain a dependency cycle"),
            };
            if deps.remove(&ty).is_none() {
                continue;
            }
            output.push(items[&ty]);
            if let Some(dependents) = reverse.get(&ty) {
                for dependent in dependents {
                    if let Some(values) = deps.get_mut(dependent) {
                        values.remove(&ty);
                        if values.is_empty() {
                            ready.push((self.wire_id(*dependent), *dependent));
                        }
                    }
                }
            }
        }
        output.extend(
            self.schema
                .items()
                .iter()
                .copied()
                .filter(|item| item.kind == OutputItemKind::VoidDef),
        );
        output
    }

    fn direct_deps(&self, ty: TypeId) -> HashSet<TypeId> {
        let mut out = HashSet::new();
        match self.schema.types.expect_type_shape(ty) {
            TypeShape::Struct(fields) => {
                for field in fields.values() {
                    self.collect_wire_dep(field.type_id, &mut out, &mut HashSet::new());
                }
            }
            TypeShape::Enum(_) => {}
            TypeShape::Array { element, .. } | TypeShape::Optional(element) => {
                self.collect_wire_dep(*element, &mut out, &mut HashSet::new());
            }
            TypeShape::Ref(definition) => {
                self.collect_wire_dep(
                    self.schema.types.expect_def_output(*definition),
                    &mut out,
                    &mut HashSet::new(),
                );
            }
            TypeShape::Custom(_) => {
                if self.items_by_type.contains_key(&ty) {
                    out.insert(ty);
                }
            }
            TypeShape::Void | TypeShape::Node => {}
        }
        out.remove(&ty);
        out
    }

    fn collect_wire_dep(&self, ty: TypeId, out: &mut HashSet<TypeId>, seen: &mut HashSet<TypeId>) {
        if !seen.insert(ty) {
            return;
        }
        match self.schema.types.expect_type_shape(ty) {
            TypeShape::Array { element, .. } | TypeShape::Optional(element) => {
                self.collect_wire_dep(*element, out, seen);
            }
            TypeShape::Ref(definition) => {
                self.collect_wire_dep(self.schema.types.expect_def_output(*definition), out, seen);
            }
            TypeShape::Struct(_) | TypeShape::Enum(_) => {
                if let Some(item) = self.item_for_type(ty) {
                    out.insert(item.ty);
                }
            }
            TypeShape::Custom(_) => {
                if let Some(item) = self.item_for_type(ty) {
                    out.insert(item.ty);
                }
            }
            TypeShape::Void | TypeShape::Node => {}
        }
    }

    fn collect_reachable(&self, ty: TypeId, out: &mut HashSet<TypeId>, seen: &mut HashSet<TypeId>) {
        if !seen.insert(ty) {
            return;
        }
        if self.items_by_type.contains_key(&ty) {
            out.insert(ty);
        }
        match self.schema.types.expect_type_shape(ty) {
            TypeShape::Struct(fields) => {
                for field in fields.values() {
                    self.collect_reachable(field.type_id, out, seen);
                }
            }
            TypeShape::Enum(variants) => {
                for payload in variants.values() {
                    self.collect_reachable(*payload, out, seen);
                }
            }
            TypeShape::Array { element, .. } | TypeShape::Optional(element) => {
                self.collect_reachable(*element, out, seen);
            }
            TypeShape::Ref(definition) => {
                let name = self.schema.deps.def_name_sym(*definition);
                if let Some(item) = self.items_by_name.get(&name) {
                    out.insert(item.ty);
                }
                self.collect_reachable(self.schema.types.expect_def_output(*definition), out, seen);
            }
            TypeShape::Void | TypeShape::Node | TypeShape::Custom(_) => {}
        }
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
            OutputItemKind::Struct => self.emit_interface(&name, item.ty),
            OutputItemKind::Enum => self.emit_enum(&name, item.ty),
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

        let TypeShape::Struct(fields) = self.schema.types.expect_type_shape(ty) else {
            unreachable!("struct output item has a struct shape");
        };
        let scope = self
            .schema
            .layout()
            .scope(ty)
            .expect("struct output has a capture scope");
        let mut fields = fields
            .iter()
            .enumerate()
            .map(|(index, (&symbol, info))| {
                (self.name(symbol), *info, scope.absolute_index(index as u16))
            })
            .collect::<Vec<_>>();
        fields.sort_by(|left, right| left.0.cmp(&right.0));
        for (field, info, member) in fields {
            let value = self.render_field(info.type_id, info.optional);
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

    fn emit_enum(&mut self, name: &str, ty: TypeId) {
        emit_export(&mut self.sink, self.config.export);
        self.sink.styled(Style::Dim, "type");
        self.sink.push(" ");
        self.sink.set_style(Style::Blue);
        self.push_mapped(name, ty, None);
        self.sink.reset_style();
        self.sink.push(" ");
        self.sink.styled(Style::Dim, "=");
        self.sink.push("\n");

        let TypeShape::Enum(variants) = self.schema.types.expect_type_shape(ty) else {
            unreachable!("enum output item has an enum shape");
        };
        let scope = self
            .schema
            .layout()
            .scope(ty)
            .expect("enum output has a capture scope");
        let last = variants.len().saturating_sub(1);
        for (position, (&symbol, &payload)) in variants.iter().enumerate() {
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
            TypeShape::Optional(inner) => self.render_nullable(*inner),
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
            TypeShape::Struct(_) => self.inline_struct(ty, false),
            TypeShape::Enum(variants) => {
                let mut out = Sink::new();
                for (position, (&name, &payload)) in variants.iter().enumerate() {
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

    fn render_field(&self, ty: TypeId, optional: bool) -> Sink<SemanticTag> {
        if !optional {
            return self.render_ty(ty);
        }
        let mut out = self.render_ty(ty);
        out.push(" ");
        out.styled(Style::Dim, "|");
        out.push(" null");
        out
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

    fn inline_struct(&self, ty: TypeId, tags: bool) -> Sink<SemanticTag> {
        let TypeShape::Struct(fields) = self.schema.types.expect_type_shape(ty) else {
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
            .expect("inline struct has a capture scope");
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
            out.append(self.render_field(info.type_id, info.optional));
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
        out.append(self.inline_struct(payload, payload_tags));
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

    fn wire_id(&self, ty: TypeId) -> u32 {
        *self
            .wire_ids
            .get(&ty)
            .expect("every rendered output type has a projected identity")
    }

    fn item_for_type(&self, ty: TypeId) -> Option<OutputItem> {
        self.items_by_type.get(&ty).copied().or_else(|| {
            self.schema
                .type_name_of(ty)
                .and_then(|name| self.items_by_name.get(&name).copied())
        })
    }

    fn name(&self, symbol: Symbol) -> String {
        self.schema.interner.resolve(symbol).to_string()
    }
}

fn wire_ids(schema: &OutputSchema<'_>) -> HashMap<TypeId, u32> {
    let mut uses_void = false;
    let mut uses_node = false;
    let mut seen = HashSet::new();
    for &ty in schema.ordered_types() {
        collect_builtin_usage(schema, ty, &mut seen, &mut uses_void, &mut uses_node);
    }
    for (_, ty) in schema.types.iter_def_output() {
        uses_void |= ty == TYPE_VOID;
        uses_node |= ty == TYPE_NODE;
    }
    let mut ids = HashMap::new();
    let mut next = 0u32;
    if uses_void {
        ids.insert(TYPE_VOID, next);
        next += 1;
    }
    if uses_node {
        ids.insert(TYPE_NODE, next);
        next += 1;
    }
    for &ty in schema.ordered_types() {
        ids.insert(ty, next);
        next += 1;
    }
    ids
}

fn collect_builtin_usage(
    schema: &OutputSchema<'_>,
    ty: TypeId,
    seen: &mut HashSet<TypeId>,
    uses_void: &mut bool,
    uses_node: &mut bool,
) {
    if !seen.insert(ty) {
        return;
    }
    match schema.types.expect_type_shape(ty) {
        TypeShape::Void => *uses_void = true,
        TypeShape::Node | TypeShape::Custom(_) => *uses_node = true,
        TypeShape::Ref(definition) => {
            let target = schema.types.expect_def_output(*definition);
            if target == TYPE_VOID {
                *uses_node = true;
            } else {
                collect_builtin_usage(schema, target, seen, uses_void, uses_node);
            }
        }
        shape => {
            for child in shape.child_type_ids() {
                collect_builtin_usage(schema, child, seen, uses_void, uses_node);
            }
        }
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
