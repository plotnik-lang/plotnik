//! Typed replay emission: per-type readers and the `parse`/`matches` surface.
//!
//! The committed trace is a tiny wire format whose schema *is* the query's
//! output type, so the replay is a generated deserializer (serde-derive
//! mental model), not an interpreter: one reader fn per named type, shared
//! across every position that holds it, matching only the entries the type
//! admits. It runs once, on the winning path; failed branches never reach it.
//!
//! Two stream facts shape the readers (see the VM materializer, the reference
//! consumer):
//!
//! - **Values are value-first.** A field's entries arrive *before* the
//!   `Set` that names the field, and `Set` order inside one struct varies
//!   between instances of the same type. Struct scopes therefore peek ahead
//!   to the balancing `Set` (`TraceReader::peek_set`) to pick the field's
//!   typed reader, then consume linearly.
//! - **`Set`/`EnumOpen` payloads are absolute member-table indices**, baked
//!   from the same emit tables the matcher folded into its states. Nominal
//!   twins (one name, several structurally-identical analysis types) own
//!   distinct member runs, so an arm matches the union of its twins' indices.
//!
//! Naming is never re-derived: item and field spellings come from the
//! typegen emitter's own model, so a keyword-renamed field reads exactly as
//! it was declared.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;

use crate::compiler::analyze::AnalysisArtifacts;
use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::analyze::types::TypeAnalysis;
use crate::compiler::analyze::types::type_shape::{FieldInfo, TYPE_VOID, TypeId, TypeShape};
use crate::compiler::emit::tables::TypeTableBuilder;
use crate::compiler::ids::DefId;
use crate::compiler::typegen::rust::emitter::{Emitter as TypeModel, Item, ItemKind};
use crate::compiler::typegen::rust::idents::scope_idents;
use crate::core::{Interner, Symbol};

use super::emitter::{entry_fn_name, metered_entry_fn_name, snake_ident};

pub(super) struct ReaderGen<'a> {
    model: TypeModel<'a>,
    types: &'a TypeAnalysis,
    deps: &'a DependencyAnalysis,
    interner: &'a Interner,
    table: &'a TypeTableBuilder,
    tables: ReaderTables,
}

struct ReaderTables {
    /// Item name → reader fn ident, uniqued in item order (nominally distinct
    /// names can share a snake form, e.g. `HTTPServer` / `HttpServer`).
    reader_fns: HashMap<Symbol, String>,
    /// Item name → every table-reachable analysis type carrying it (nominal
    /// twins). The item's own type is always among them.
    twins: HashMap<Symbol, Vec<TypeId>>,
}

impl ReaderTables {
    fn collect(
        model: &TypeModel<'_>,
        types: &TypeAnalysis,
        table: &TypeTableBuilder,
        interner: &Interner,
    ) -> Self {
        let mut reader_fns = HashMap::new();
        let mut taken = HashSet::new();
        let mut twins: HashMap<Symbol, Vec<TypeId>> = HashMap::new();
        for item in model.items() {
            if !item.has_reader() {
                continue;
            }
            let mut name = format!("read_{}", snake_ident(interner.resolve(item.name)));
            while !taken.insert(name.clone()) {
                name.push('_');
            }
            reader_fns.insert(item.name, name);

            if item.is_composite() {
                twins.insert(item.name, collect_twins(types, table, item));
            }
        }

        Self { reader_fns, twins }
    }
}

struct InherentParseSignature {
    ident: String,
    impl_generics: &'static str,
    type_generics: &'static str,
    tree_ref: &'static str,
}

impl InherentParseSignature {
    fn for_item(model: &TypeModel<'_>, item: &Item) -> Self {
        let ident = model.item_ident(item.name).to_string();
        if model.needs_lifetime(item.ty) {
            return Self {
                ident,
                impl_generics: "<'t>",
                type_generics: "<'t>",
                tree_ref: "&'t rt::Tree",
            };
        }
        Self {
            ident,
            impl_generics: "",
            type_generics: "",
            tree_ref: "&rt::Tree",
        }
    }
}

struct FreeParseSignature {
    ident: String,
    fn_generics: &'static str,
    tree_ref: &'static str,
    return_type: String,
}

impl FreeParseSignature {
    fn for_item(model: &TypeModel<'_>, item: &Item) -> Self {
        let ident = model.item_ident(item.name).to_string();
        if model.needs_lifetime(item.ty) {
            return Self {
                ident: ident.clone(),
                fn_generics: "<'t>",
                tree_ref: "&'t rt::Tree",
                return_type: format!("{ident}<'t>"),
            };
        }
        Self {
            return_type: ident.clone(),
            ident,
            fn_generics: "",
            tree_ref: "&rt::Tree",
        }
    }
}

struct TraceReaderSignature {
    ident: String,
    fn_generics: &'static str,
    reader_generics: &'static str,
    return_type: String,
}

impl TraceReaderSignature {
    fn for_item(model: &TypeModel<'_>, item: &Item) -> Self {
        let ident = model.item_ident(item.name).to_string();
        if model.needs_lifetime(item.ty) {
            return Self {
                ident: ident.clone(),
                fn_generics: "<'t>",
                reader_generics: "<'_, 't>",
                return_type: format!("{ident}<'t>"),
            };
        }
        Self {
            return_type: ident.clone(),
            ident,
            fn_generics: "",
            reader_generics: "<'_, '_>",
        }
    }
}

struct ParseReplay<'a> {
    indent: &'a str,
    reader: &'a str,
}

impl<'a> ParseReplay<'a> {
    fn new(indent: &'a str, reader: &'a str) -> Self {
        Self { indent, reader }
    }

    fn emit_unmetered(&self, out: &mut String, trace: &str) {
        let indent = self.indent;
        let _ = writeln!(out, "{indent}let log = {trace}(tree, source)?;");
        self.emit_value(out, "Some(value)");
    }

    fn emit_metered(&self, out: &mut String, metered: &str) {
        let indent = self.indent;
        let _ = writeln!(
            out,
            "{indent}let Some(log) = matcher::{metered}(tree, source)? else {{"
        );
        let _ = writeln!(out, "{indent}    return Ok(None);");
        let _ = writeln!(out, "{indent}}};");
        self.emit_value(out, "Ok(Some(value))");
    }

    fn emit_value(&self, out: &mut String, result: &str) {
        let indent = self.indent;
        let reader = self.reader;
        let _ = writeln!(out, "{indent}let mut t = rt::TraceReader::new(&log);");
        let _ = writeln!(out, "{indent}let value = {reader}(&mut t);");
        let _ = writeln!(out, "{indent}t.finish();");
        let _ = writeln!(out, "{indent}{result}");
    }
}

impl<'a> ReaderGen<'a> {
    pub(super) fn new(
        artifacts: AnalysisArtifacts<'a>,
        table: &'a TypeTableBuilder,
        config: &'a crate::compiler::typegen::rust::Config,
    ) -> Self {
        let types = artifacts.type_analysis;
        let model = TypeModel::model(
            types,
            artifacts.dependency_analysis,
            artifacts.interner,
            config,
        );
        let tables = ReaderTables::collect(&model, types, table, artifacts.interner);

        Self {
            model,
            types,
            deps: artifacts.dependency_analysis,
            interner: artifacts.interner,
            table,
            tables,
        }
    }

    fn reader_fn(&self, name: Symbol) -> &str {
        self.tables
            .reader_fns
            .get(&name)
            .expect("every non-void item has a reader")
    }

    /// The `parse`/`try_parse` or `matches` surface, one block per entrypoint
    /// definition. Nominal outputs get inherent parse fns on their type, void
    /// outputs get an inherent safe `matches`, and alias outputs still get free
    /// parse fns.
    pub(super) fn parse_api(&self, entrypoints: impl Iterator<Item = DefId>) -> String {
        let mut out = String::new();
        for def_id in entrypoints {
            let name = self.deps.def_name_sym(def_id);
            let def = self.interner.resolve(name).to_string();
            out.push('\n');
            let item = *self
                .model
                .items()
                .iter()
                .find(|item| item.name == name)
                .expect("every entrypoint definition declares an item");
            match item.kind {
                ItemKind::Struct | ItemKind::Enum => self.parse_impl(&mut out, &def, &item),
                ItemKind::Alias => self.parse_free_fns(&mut out, &def, &item),
                ItemKind::VoidDef => self.matches_impl(&mut out, &def, &item),
            }
        }
        out
    }

    /// `matches` for a void definition: it can only answer matched-or-not, and
    /// the public API is always metered.
    fn matches_impl(&self, out: &mut String, def: &str, item: &Item) {
        let ident = self.model.item_ident(item.name);
        let metered = metered_entry_fn_name(def);
        let _ = writeln!(out, "impl {ident} {{");
        let _ = writeln!(
            out,
            "    /// Whether `{def}` matches `tree` under the module's compiled-in limits."
        );
        let _ = writeln!(out, "    pub fn matches(");
        let _ = writeln!(out, "        tree: &rt::Tree,");
        let _ = writeln!(out, "        source: &str,");
        let _ = writeln!(
            out,
            "    ) -> ::core::result::Result<bool, rt::LimitError> {{"
        );
        let _ = writeln!(
            out,
            "        Ok(matcher::{metered}(tree, source)?.is_some())"
        );
        let _ = writeln!(out, "    }}");
        let _ = writeln!(out, "}}");
    }

    /// Inherent `parse`/`try_parse` on a nominal (struct/enum) output type.
    fn parse_impl(&self, out: &mut String, def: &str, item: &Item) {
        let sig = InherentParseSignature::for_item(&self.model, item);
        let reader = self.reader_fn(item.name);
        let replay = ParseReplay::new("        ", reader);
        let trace = entry_fn_name(def);
        let metered = metered_entry_fn_name(def);
        let _ = writeln!(
            out,
            "impl{} {}{} {{",
            sig.impl_generics, sig.ident, sig.type_generics
        );
        let _ = writeln!(
            out,
            "    /// Match `{def}` against `tree` and replay the committed trace into"
        );
        let _ = writeln!(
            out,
            "    /// the typed output. `None` is the no-match outcome."
        );
        let _ = writeln!(
            out,
            "    pub fn parse(tree: {}, source: &str) -> ::core::option::Option<Self> {{",
            sig.tree_ref
        );
        replay.emit_unmetered(out, &trace);
        let _ = writeln!(out, "    }}");
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "    /// [`Self::parse`] under the module's compiled-in limits (steps bound"
        );
        let _ = writeln!(
            out,
            "    /// total work, memory bounds live backtracking state)."
        );
        let _ = writeln!(out, "    pub fn try_parse(");
        let _ = writeln!(out, "        tree: {},", sig.tree_ref);
        let _ = writeln!(out, "        source: &str,");
        let _ = writeln!(
            out,
            "    ) -> ::core::result::Result<::core::option::Option<Self>, rt::LimitError> {{"
        );
        replay.emit_metered(out, &metered);
        let _ = writeln!(out, "    }}");
        let _ = writeln!(out, "}}");
    }

    /// Free `parse` fns for an alias output — `impl` blocks cannot attach to
    /// a `pub type` alias of a non-local type.
    fn parse_free_fns(&self, out: &mut String, def: &str, item: &Item) {
        let sig = FreeParseSignature::for_item(&self.model, item);
        let snake = snake_ident(def);
        let reader = self.reader_fn(item.name);
        let replay = ParseReplay::new("    ", reader);
        let trace = entry_fn_name(def);
        let metered = metered_entry_fn_name(def);
        let _ = writeln!(
            out,
            "/// Match `{def}` against `tree` and replay the committed trace into the"
        );
        let _ = writeln!(
            out,
            "/// typed output (`{}` aliases a non-nominal type, so `parse` is a",
            sig.ident
        );
        let _ = writeln!(out, "/// free function). `None` is the no-match outcome.");
        let _ = writeln!(out, "pub fn {snake}_parse{}(", sig.fn_generics);
        let _ = writeln!(out, "    tree: {},", sig.tree_ref);
        let _ = writeln!(out, "    source: &str,");
        let _ = writeln!(out, ") -> ::core::option::Option<{}> {{", sig.return_type);
        replay.emit_unmetered(out, &trace);
        let _ = writeln!(out, "}}");
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "/// [`{snake}_parse`] under the module's compiled-in limits."
        );
        let _ = writeln!(out, "pub fn {snake}_try_parse{}(", sig.fn_generics);
        let _ = writeln!(out, "    tree: {},", sig.tree_ref);
        let _ = writeln!(out, "    source: &str,");
        let _ = writeln!(
            out,
            ") -> ::core::result::Result<::core::option::Option<{}>, rt::LimitError> {{",
            sig.return_type
        );
        replay.emit_metered(out, &metered);
        let _ = writeln!(out, "}}");
    }

    /// Every reader fn, in item order.
    pub(super) fn readers(&self) -> String {
        let mut out = String::new();
        for item in self.model.items() {
            match item.kind {
                ItemKind::Struct => self.struct_reader(&mut out, item),
                ItemKind::Enum => self.enum_reader(&mut out, item),
                ItemKind::Alias => self.alias_reader(&mut out, item),
                ItemKind::VoidDef => {}
            }
        }
        out
    }

    fn reader_open(&self, out: &mut String, item: &Item) {
        let sig = TraceReaderSignature::for_item(&self.model, item);
        let reader = self.reader_fn(item.name);
        let _ = writeln!(out, "/// Replay one committed `{}` value.", sig.ident);
        let _ = writeln!(
            out,
            "fn {reader}{}(t: &mut rt::TraceReader{}) -> {} {{",
            sig.fn_generics, sig.reader_generics, sig.return_type
        );
    }

    fn struct_reader(&self, out: &mut String, item: &Item) {
        let TypeShape::Struct(fields) = self.types.expect_type_shape(item.ty) else {
            unreachable!("struct item must have a struct shape");
        };
        let ident = self.model.item_ident(item.name).to_string();
        let twins = &self.tables.twins[&item.name];
        out.push('\n');
        self.reader_open(out, item);
        out.push_str("    t.expect_struct_open();\n");
        let scope = Scope::struct_body(item.ty, &ident);
        self.field_scope(out, &scope, fields, |k| {
            member_indices(self.table, twins, k)
        });
        out.push_str("    t.expect_struct_close();\n");
        self.construct(out, Construction::struct_body(&ident), fields);
        out.push_str("}\n");
    }

    fn enum_reader(&self, out: &mut String, item: &Item) {
        let TypeShape::Enum(variants) = self.types.expect_type_shape(item.ty) else {
            unreachable!("enum item must have an enum shape");
        };
        let ident = self.model.item_ident(item.name).to_string();
        let twins = &self.tables.twins[&item.name];
        let variant_idents = scope_idents(variants.keys().map(|&sym| self.interner.resolve(sym)));
        out.push('\n');
        self.reader_open(out, item);
        out.push_str("    match t.expect_enum_open() {\n");
        for (k, ((&label, &payload), variant_ident)) in
            variants.iter().zip(&variant_idents).enumerate()
        {
            let indices = arm_pattern(member_indices(self.table, twins, k));
            let label = self.interner.resolve(label);
            let _ = writeln!(out, "        // {label}");
            let _ = writeln!(out, "        {indices} => {{");
            if payload == TYPE_VOID {
                let _ = writeln!(out, "            t.expect_enum_close();");
                let _ = writeln!(out, "            {ident}::{variant_ident}");
                out.push_str("        }\n");
                continue;
            }

            let TypeShape::Struct(fields) = self.types.expect_type_shape(payload) else {
                unreachable!("enum variant payload is void or an anonymous struct");
            };
            let payloads = payload_twins(self.types, twins, k);
            let scope = Scope::enum_payload(item.ty, &ident);
            self.field_scope(out, &scope, fields, |j| {
                member_indices(self.table, &payloads, j)
            });
            out.push_str("            t.expect_enum_close();\n");
            self.construct(
                out,
                Construction::enum_variant(&ident, variant_ident),
                fields,
            );
            out.push_str("        }\n");
        }
        let _ = writeln!(
            out,
            "        other => unreachable!(\"trace shape proven at emit: `{ident}` has no variant index {{other}}\"),"
        );
        out.push_str("    }\n");
        out.push_str("}\n");
    }

    fn alias_reader(&self, out: &mut String, item: &Item) {
        out.push('\n');
        self.reader_open(out, item);
        let expr = self.value_expr(item.ty, ReadContext::item(item.ty, 1));
        let _ = writeln!(out, "    {expr}");
        out.push_str("}\n");
    }

    /// The field-collection loop of one struct-like scope: positional locals,
    /// the peek-dispatch loop over member indices, one `Set` consumed per
    /// field value. Enum payloads reuse it with `EnumClose` as the terminator
    /// (payload `Set`s attach directly to the enum frame — the materializer's
    /// contract).
    fn field_scope(
        &self,
        out: &mut String,
        scope: &Scope<'_>,
        fields: &BTreeMap<Symbol, FieldInfo>,
        indices_of: impl Fn(usize) -> Vec<u16>,
    ) {
        let p = pad(scope.level());
        for (k, &name) in fields.keys().enumerate() {
            let _ = writeln!(
                out,
                "{p}let mut v{k} = None; // {}",
                self.interner.resolve(name)
            );
        }
        let _ = writeln!(out, "{p}while !t.{}() {{", scope.close.probe());
        let _ = writeln!(out, "{p}    match t.peek_set() {{");
        for (k, (&name, info)) in fields.iter().enumerate() {
            let indices = arm_pattern(indices_of(k));
            let expr = self.field_expr(info, scope.field_context());
            let _ = writeln!(out, "{p}        // {}", self.interner.resolve(name));
            let _ = writeln!(out, "{p}        {indices} => v{k} = Some({expr}),");
        }
        let _ = writeln!(
            out,
            "{p}        other => unreachable!(\"trace shape proven at emit: `{}` has no member index {{other}}\"),",
            scope.name
        );
        let _ = writeln!(out, "{p}    }}");
        let _ = writeln!(out, "{p}    t.expect_set();");
        let _ = writeln!(out, "{p}}}");
    }

    /// The construction expression closing a scope: every field was set
    /// exactly once (field-stability null-defaulting guarantees a `Set` per
    /// field on every accepting path), so the positional locals unwrap.
    fn construct(
        &self,
        out: &mut String,
        construction: Construction,
        fields: &BTreeMap<Symbol, FieldInfo>,
    ) {
        let p = pad(construction.level);
        let head = construction.head;
        let field_idents = scope_idents(fields.keys().map(|&sym| self.interner.resolve(sym)));
        let _ = writeln!(out, "{p}{head} {{");
        for (k, ((&name, _), field_ident)) in fields.iter().zip(&field_idents).enumerate() {
            let name = self.interner.resolve(name);
            let _ = writeln!(
                out,
                "{p}    {field_ident}: v{k}.expect(\"field-stability: every accepting path sets `{name}`\"),"
            );
        }
        let _ = writeln!(out, "{p}}}");
    }

    /// A field's read expression: the capture-level `optional` flag wraps one
    /// more null check around the base, exactly like the type wraps one more
    /// `Option`.
    fn field_expr(&self, info: &FieldInfo, context: ReadContext) -> String {
        if info.optional {
            self.nullable_expr(info.type_id, context)
        } else {
            self.value_expr(info.type_id, context)
        }
    }

    /// Read one value of `ty`. The returned expression's first line splices
    /// inline; continuation lines are indented for `level`. `depth` suffixes
    /// array accumulators so nested arrays don't shadow. `cut` is the item
    /// declaration this position renders inside — the box-placement context,
    /// threaded exactly as the type renderer threads it so `Box::new` sits
    /// precisely where the declared type says `Box`.
    fn value_expr(&self, ty: TypeId, context: ReadContext) -> String {
        match self.types.expect_type_shape(ty) {
            // A `Custom` is a named alias of Node; the node is the value.
            TypeShape::Node | TypeShape::Custom(_) => "t.expect_node()".to_string(),
            TypeShape::Optional(inner) => self.nullable_expr(*inner, context),
            TypeShape::Array { element, .. } => self.array_expr(*element, context),
            TypeShape::Struct(_) | TypeShape::Enum(_) => {
                let name = self
                    .model
                    .type_name_of(ty)
                    .expect("naming pass names every composite outside enum-variant payloads");
                format!("{}(t)", self.reader_fn(name))
            }
            TypeShape::Ref(def_id) => {
                let target = self.types.expect_def_output(*def_id);
                if target == TYPE_VOID {
                    // A void target contributes no value; the capture holds
                    // the matched node itself.
                    return "t.expect_node()".to_string();
                }
                let call = format!("{}(t)", self.reader_fn(self.deps.def_name_sym(*def_id)));
                if self.model.is_boxed_ref(context.cut, ty) {
                    format!("::std::boxed::Box::new({call})")
                } else {
                    call
                }
            }
            TypeShape::Void => unreachable!("void cannot appear in an output position"),
        }
    }

    /// `Null` is the whole absent value — one flat null, however many
    /// `Option` layers the type carries; a present value wraps `Some` at
    /// every layer (the VM never nests nulls).
    fn nullable_expr(&self, inner: TypeId, context: ReadContext) -> String {
        let p = pad(context.level);
        let inner_expr = self.value_expr(inner, context.in_some_branch());
        let mut out = String::new();
        let _ = writeln!(out, "if t.take_null() {{");
        let _ = writeln!(out, "{p}    None");
        let _ = writeln!(out, "{p}}} else {{");
        let _ = writeln!(out, "{p}    Some({inner_expr})");
        let _ = write!(out, "{p}}}");
        out
    }

    fn array_expr(&self, element: TypeId, context: ReadContext) -> String {
        let p = pad(context.level);
        let items = format!("items{}", context.array_depth);
        let elem = self.value_expr(element, context.array_element());
        let mut out = String::new();
        let _ = writeln!(out, "{{");
        let _ = writeln!(out, "{p}    t.expect_array_open();");
        let _ = writeln!(out, "{p}    let mut {items} = ::std::vec::Vec::new();");
        let _ = writeln!(out, "{p}    while !t.at_array_close() {{");
        let _ = writeln!(out, "{p}        {items}.push({elem});");
        let _ = writeln!(out, "{p}        t.expect_push();");
        let _ = writeln!(out, "{p}    }}");
        let _ = writeln!(out, "{p}    t.expect_array_close();");
        let _ = writeln!(out, "{p}    {items}");
        let _ = write!(out, "{p}}}");
        out
    }
}

struct Construction {
    level: usize,
    head: String,
}

impl Construction {
    fn struct_body(ident: &str) -> Self {
        Self {
            level: 1,
            head: ident.to_string(),
        }
    }

    fn enum_variant(enum_ident: &str, variant_ident: &str) -> Self {
        Self {
            level: 3,
            head: format!("{enum_ident}::{variant_ident}"),
        }
    }
}

/// Where a value expression is emitted. `cut` is the owning item declaration
/// that decides recursive `Box` placement, `level` is the emitted indentation,
/// and `array_depth` keeps nested array accumulator names distinct.
#[derive(Clone, Copy)]
struct ReadContext {
    cut: Option<TypeId>,
    level: usize,
    array_depth: usize,
}

impl ReadContext {
    fn item(item_ty: TypeId, level: usize) -> Self {
        Self {
            cut: Some(item_ty),
            level,
            array_depth: 0,
        }
    }

    fn field_value(self) -> Self {
        Self {
            level: self.level + 2,
            ..self
        }
    }

    fn in_some_branch(self) -> Self {
        Self {
            level: self.level + 1,
            ..self
        }
    }

    fn array_element(self) -> Self {
        Self {
            cut: None,
            level: self.level + 2,
            array_depth: self.array_depth + 1,
        }
    }
}

/// One struct-like scope as [`ReaderGen::field_scope`] reads it: the
/// box-placement cut of the owning item, the emitted code's indent level,
/// the terminator probe, and the display name its panics cite.
struct Scope<'a> {
    context: ReadContext,
    close: ScopeClose,
    name: &'a str,
}

#[derive(Clone, Copy)]
enum ScopeClose {
    Struct,
    Enum,
}

impl ScopeClose {
    fn probe(self) -> &'static str {
        match self {
            ScopeClose::Struct => "at_struct_close",
            ScopeClose::Enum => "at_enum_close",
        }
    }
}

impl<'a> Scope<'a> {
    fn struct_body(owner: TypeId, name: &'a str) -> Self {
        Self {
            context: ReadContext::item(owner, 1),
            close: ScopeClose::Struct,
            name,
        }
    }

    fn enum_payload(owner: TypeId, name: &'a str) -> Self {
        Self {
            context: ReadContext::item(owner, 3),
            close: ScopeClose::Enum,
            name,
        }
    }

    fn field_context(&self) -> ReadContext {
        self.context.field_value()
    }

    fn level(&self) -> usize {
        self.context.level
    }
}

/// Every table-reachable analysis type sharing this item's name and shape
/// kind. Structural identity is enforced upstream (same name ⇒ same shape),
/// so twins differ only in their member-run offsets.
fn collect_twins(types: &TypeAnalysis, table: &TypeTableBuilder, item: &Item) -> Vec<TypeId> {
    let wants_struct = item.is_struct();
    let mut out: BTreeSet<TypeId> = BTreeSet::new();
    for (ty, name) in types.iter_type_names() {
        if name != item.name {
            continue;
        }
        let matches_kind = match types.expect_type_shape(ty) {
            TypeShape::Struct(_) => wants_struct,
            TypeShape::Enum(_) => !wants_struct,
            _ => false,
        };
        if !matches_kind {
            continue;
        }
        if table.member_base(ty).is_none() {
            continue;
        }
        out.insert(ty);
    }
    out.insert(item.ty);
    out.into_iter().collect()
}

/// Variant `k`'s payload struct across every twin enum. Twins are
/// structurally identical, so the variant lists align by position.
fn payload_twins(types: &TypeAnalysis, twins: &[TypeId], k: usize) -> Vec<TypeId> {
    twins
        .iter()
        .map(|&ty| {
            let TypeShape::Enum(variants) = types.expect_type_shape(ty) else {
                unreachable!("enum twins share the enum shape");
            };
            *variants
                .values()
                .nth(k)
                .expect("twins share the variant list")
        })
        .collect()
}

/// The absolute member indices position `k` of this composite can arrive as,
/// one per twin — the same `base + relative` sum the matcher baked into its
/// `Set`/`EnumOpen` operands.
fn member_indices(table: &TypeTableBuilder, twins: &[TypeId], k: usize) -> Vec<u16> {
    twins
        .iter()
        .map(|&ty| {
            let base = table
                .member_base(ty)
                .expect("twins are collected table-present");
            base + u16::try_from(k).expect("member count fits u16")
        })
        .collect()
}

/// `12` or `12 | 27` — sorted, deduped match pattern over twin indices.
fn arm_pattern(mut indices: Vec<u16>) -> String {
    indices.sort_unstable();
    indices.dedup();
    indices
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(" | ")
}

fn pad(level: usize) -> String {
    "    ".repeat(level)
}
