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

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

use crate::compiler::analyze::AnalysisArtifacts;
use crate::compiler::analyze::output::OutputSchema;
use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::analyze::types::TypeAnalysis;
use crate::compiler::analyze::types::type_shape::{FieldInfo, TYPE_VOID, TypeId, TypeShape};
use crate::compiler::codegen::emit::names::{rust_scope_idents, snake_ident};
use crate::compiler::codegen::emit::sink::indentation;
use crate::compiler::codegen::plan::{
    ReplayItem, ReplayItemKind, ReplayPlan, ReplayScopePlan, ReplayValuePlan, ReplayVariantPlan,
};
use crate::compiler::ids::DefId;
use crate::compiler::typegen::rust::emitter::{Emitter as TypeModel, TypeContext};
use crate::core::{Interner, Symbol};

use super::emitter::{accepts_entry_fn_name, safe_entry_fn_name};

const WORD_BYTES: u64 = 8;
const NODE_VALUE_BYTES: u64 = 48;
const VEC_VALUE_BYTES: u64 = 24;
const OPTION_TAG_BYTES: u64 = 8;
const READER_FRAME_BASE_BYTES: u64 = 128;

pub(super) struct ReaderGen<'a> {
    model: TypeModel<'a>,
    types: &'a TypeAnalysis,
    deps: &'a DependencyAnalysis,
    interner: &'a Interner,
    replay: &'a ReplayPlan,
    tables: ReaderTables,
}

struct ReaderTables {
    /// Item name → reader fn ident, uniqued in item order (nominally distinct
    /// names can share a snake form, e.g. `HTTPServer` / `HttpServer`).
    reader_fns: HashMap<Symbol, String>,
}

impl ReaderTables {
    fn collect(replay: &ReplayPlan, interner: &Interner) -> Self {
        let mut reader_fns = HashMap::new();
        let mut taken = HashSet::new();
        for item in replay.items() {
            if !item.has_reader() {
                continue;
            }
            let mut name = format!("read_{}", snake_ident(interner.resolve(item.name)));
            while !taken.insert(name.clone()) {
                name.push('_');
            }
            reader_fns.insert(item.name, name);
        }

        Self { reader_fns }
    }
}

struct InherentParseSignature {
    ident: String,
    impl_generics: &'static str,
    type_generics: &'static str,
    tree_ref: &'static str,
}

impl InherentParseSignature {
    fn for_item(model: &TypeModel<'_>, item: &ReplayItem) -> Self {
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

impl<'a> ReaderGen<'a> {
    pub(super) fn new(
        artifacts: AnalysisArtifacts<'a>,
        output: &OutputSchema<'a>,
        replay: &'a ReplayPlan,
        config: &'a crate::compiler::typegen::rust::Config,
    ) -> Self {
        let types = artifacts.type_analysis;
        let model = TypeModel::model(output.clone(), config);
        let tables = ReaderTables::collect(replay, artifacts.interner);

        Self {
            model,
            types,
            deps: artifacts.dependency_analysis,
            interner: artifacts.interner,
            replay,
            tables,
        }
    }

    fn reader_fn(&self, name: Symbol) -> &str {
        self.tables
            .reader_fns
            .get(&name)
            .expect("every non-void item has a reader")
    }

    fn item_named(&self, name: Symbol) -> &ReplayItem {
        self.replay.item(name)
    }

    fn reader_call(&self, name: Symbol) -> String {
        let reader = self.reader_fn(name);
        let item = self.item_named(name);
        if item.fallible {
            format!("{reader}(t, depth)?")
        } else {
            format!("{reader}(t)")
        }
    }

    /// Conservative source-level stack estimate for generated typed replay.
    ///
    /// Rust does not expose final stack maps here, so this counts the locals
    /// the emitter creates and leaves runtime padding to `rt::replay_depth_auto`.
    /// The estimate intentionally tracks reader shape, not input size: replay
    /// depth protects the native stack.
    pub(super) fn max_reader_frame_bytes(&self) -> u64 {
        self.replay
            .items()
            .iter()
            .filter(|item| item.has_reader())
            .map(|item| self.reader_frame_bytes(item))
            .max()
            .unwrap_or(READER_FRAME_BASE_BYTES)
    }

    fn reader_frame_bytes(&self, item: &ReplayItem) -> u64 {
        let guard_bytes = if item.fallible { WORD_BYTES } else { 0 };
        let local_bytes = match self.types.expect_type_shape(item.ty) {
            TypeShape::Struct(fields) => self.field_scope_frame_bytes(item.ty, fields),
            TypeShape::Enum(variants) => variants
                .values()
                .map(|&payload| self.enum_payload_frame_bytes(item.ty, payload))
                .max()
                .unwrap_or(0),
            TypeShape::Void => 0,
            _ => self.value_temp_bytes(item.ty, ReadContext::item(item.ty, 1)),
        };

        READER_FRAME_BASE_BYTES
            .saturating_add(guard_bytes)
            .saturating_add(local_bytes)
    }

    fn enum_payload_frame_bytes(&self, owner: TypeId, payload: TypeId) -> u64 {
        if payload == TYPE_VOID {
            return 0;
        }
        let TypeShape::Struct(fields) = self.types.expect_type_shape(payload) else {
            unreachable!("enum variant payload is void or an anonymous struct");
        };
        self.field_scope_frame_bytes(owner, fields)
    }

    fn field_scope_frame_bytes(&self, owner: TypeId, fields: &BTreeMap<Symbol, FieldInfo>) -> u64 {
        let context = ReadContext::item(owner, 1).field_value();
        let slots = fields
            .values()
            .map(|info| self.option_value_bytes(self.field_value_bytes(info, context)))
            .fold(0_u64, u64::saturating_add);
        let widest_assignment = fields
            .values()
            .map(|info| self.field_value_bytes(info, context))
            .max()
            .unwrap_or(0);
        slots.saturating_add(widest_assignment)
    }

    fn field_value_bytes(&self, info: &FieldInfo, context: ReadContext) -> u64 {
        self.field_value_bytes_seen(info, context, &mut HashSet::new())
    }

    fn field_value_bytes_seen(
        &self,
        info: &FieldInfo,
        context: ReadContext,
        seen: &mut HashSet<TypeId>,
    ) -> u64 {
        let value = self.type_value_bytes(info.type_id, context, seen);
        if info.optional {
            self.option_value_bytes(value)
        } else {
            value
        }
    }

    fn value_temp_bytes(&self, ty: TypeId, context: ReadContext) -> u64 {
        match self.types.expect_type_shape(ty) {
            TypeShape::Array { element, .. } => VEC_VALUE_BYTES.saturating_add(
                self.type_value_bytes(*element, context.array_element(), &mut HashSet::new()),
            ),
            _ => self.type_value_bytes(ty, context, &mut HashSet::new()),
        }
    }

    fn type_value_bytes(
        &self,
        ty: TypeId,
        context: ReadContext,
        seen: &mut HashSet<TypeId>,
    ) -> u64 {
        if !seen.insert(ty) {
            return WORD_BYTES;
        }

        match self.types.expect_type_shape(ty) {
            TypeShape::Void => 0,
            TypeShape::Node | TypeShape::Custom(_) => NODE_VALUE_BYTES,
            TypeShape::Optional(inner) => {
                let inner = self.type_value_bytes(*inner, context, seen);
                self.option_value_bytes(inner)
            }
            TypeShape::Array { element, .. } => VEC_VALUE_BYTES
                .saturating_add(self.type_value_bytes(*element, context.array_element(), seen)),
            TypeShape::Struct(fields) => fields
                .values()
                .map(|info| {
                    let mut field_seen = seen.clone();
                    self.field_value_bytes_seen(info, context.field_value(), &mut field_seen)
                })
                .fold(0_u64, u64::saturating_add),
            TypeShape::Enum(variants) => {
                let widest = variants
                    .values()
                    .map(|&payload| {
                        let mut variant_seen = seen.clone();
                        self.type_value_bytes(payload, context, &mut variant_seen)
                    })
                    .max()
                    .unwrap_or(0);
                WORD_BYTES.saturating_add(widest)
            }
            TypeShape::Ref(def_id) => {
                let target = self.types.expect_def_output(*def_id);
                if target == TYPE_VOID {
                    return NODE_VALUE_BYTES;
                }
                if self.model.is_boxed_ref(context.type_context, ty) {
                    return WORD_BYTES;
                }
                self.type_value_bytes(target, context, seen)
            }
        }
    }

    fn option_value_bytes(&self, value_bytes: u64) -> u64 {
        align_to_word(value_bytes.saturating_add(OPTION_TAG_BYTES))
    }

    /// The `parse`/`matches` surface, one block per entrypoint definition.
    /// Callable definitions are nominal (parse + matches) or void (matches).
    pub(super) fn parse_api(&self, entrypoints: impl Iterator<Item = DefId>) -> String {
        let mut out = String::new();
        for def_id in entrypoints {
            let name = self.deps.def_name_sym(def_id);
            let def = self.interner.resolve(name).to_string();
            out.push('\n');
            let item = self.replay.item(name);
            match item.kind {
                ReplayItemKind::Struct(_) | ReplayItemKind::Enum(_) => {
                    self.parse_impl(&mut out, &def, item);
                }
                ReplayItemKind::Alias(_) => {
                    unreachable!("callable definitions are nominal or void")
                }
                ReplayItemKind::VoidDefinition => self.matches_impl(&mut out, &def, item),
            }
        }
        out
    }

    /// `matches` for a void definition: it can only answer matched-or-not, and
    /// the public API is always metered.
    fn matches_impl(&self, out: &mut String, def: &str, item: &ReplayItem) {
        let ident = self.model.item_ident(item.name);
        let _ = writeln!(out, "impl {ident} {{");
        self.inherent_matches_method(out, def);
        let _ = writeln!(out, "}}");
        let _ = writeln!(out);
        self.matches_trait_impl(out, item);
    }

    /// Inherent `parse`/`matches` on a nominal (struct/enum) output type.
    fn parse_impl(&self, out: &mut String, def: &str, item: &ReplayItem) {
        let sig = InherentParseSignature::for_item(&self.model, item);
        let reader = self.reader_fn(item.name);
        let safe = safe_entry_fn_name(def);
        let fallible_reader = item.fallible;
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
            "    /// The module's compiled-in limits bound total work, live"
        );
        let _ = writeln!(out, "    /// backtracking state, and typed replay depth.");
        let _ = writeln!(out, "    pub fn parse(");
        let _ = writeln!(out, "        tree: {},", sig.tree_ref);
        let _ = writeln!(out, "        source: &str,");
        let _ = writeln!(
            out,
            "    ) -> ::core::result::Result<::core::option::Option<Self>, rt::LimitExceeded> {{"
        );
        let _ = writeln!(
            out,
            "        let Some(log) = matcher::{safe}(tree, source)? else {{"
        );
        let _ = writeln!(out, "            return Ok(None);");
        let _ = writeln!(out, "        }};");
        let _ = writeln!(out, "        let mut t = rt::TraceReader::new(&log);");
        if fallible_reader {
            let _ = writeln!(
                out,
                "        let depth = rt::ReplayDepth::new(matcher::MAX_REPLAY_DEPTH);"
            );
            let _ = writeln!(out, "        let value = {reader}(&mut t, &depth)?;");
        } else {
            let _ = writeln!(out, "        let value = {reader}(&mut t);");
        }
        let _ = writeln!(out, "        t.finish();");
        let _ = writeln!(out, "        Ok(Some(value))");
        let _ = writeln!(out, "    }}");
        let _ = writeln!(out);
        self.inherent_matches_method(out, def);
        let _ = writeln!(out, "}}");
        let _ = writeln!(out);
        self.matches_trait_impl(out, item);
        let _ = writeln!(out);
        self.parse_trait_impl(out, item);
    }

    fn inherent_matches_method(&self, out: &mut String, def: &str) {
        let accepts = accepts_entry_fn_name(def);
        let _ = writeln!(
            out,
            "    /// Whether `{def}` matches `tree` under the module's compiled-in limits."
        );
        let _ = writeln!(out, "    pub fn matches(");
        let _ = writeln!(out, "        tree: &rt::Tree,");
        let _ = writeln!(out, "        source: &str,");
        let _ = writeln!(
            out,
            "    ) -> ::core::result::Result<bool, rt::LimitExceeded> {{"
        );
        let _ = writeln!(out, "        matcher::{accepts}(tree, source)");
        let _ = writeln!(out, "    }}");
    }

    fn matches_trait_impl(&self, out: &mut String, item: &ReplayItem) {
        let ident = self.model.item_ident(item.name).to_string();
        let (impl_generics, type_generics) = if matches!(item.kind, ReplayItemKind::VoidDefinition)
        {
            ("", "")
        } else {
            let sig = InherentParseSignature::for_item(&self.model, item);
            (sig.impl_generics, sig.type_generics)
        };
        let _ = writeln!(
            out,
            "impl{} rt::Matches for {}{} {{",
            impl_generics, ident, type_generics
        );
        let _ = writeln!(
            out,
            "    fn matches(tree: &rt::Tree, source: &str) -> ::core::result::Result<bool, rt::LimitExceeded> {{"
        );
        let _ = writeln!(out, "        {ident}::matches(tree, source)");
        let _ = writeln!(out, "    }}");
        let _ = writeln!(out, "}}");
    }

    fn parse_trait_impl(&self, out: &mut String, item: &ReplayItem) {
        let sig = InherentParseSignature::for_item(&self.model, item);
        let impl_generics = if sig.impl_generics.is_empty() {
            "<'t>"
        } else {
            sig.impl_generics
        };
        let _ = writeln!(
            out,
            "impl{impl_generics} rt::Parse<'t> for {}{} {{",
            sig.ident, sig.type_generics
        );
        let _ = writeln!(out, "    fn parse(");
        let _ = writeln!(out, "        tree: &'t rt::Tree,");
        let _ = writeln!(out, "        source: &str,");
        let _ = writeln!(
            out,
            "    ) -> ::core::result::Result<::core::option::Option<Self>, rt::LimitExceeded> {{"
        );
        let _ = writeln!(out, "        {}::parse(tree, source)", sig.ident);
        let _ = writeln!(out, "    }}");
        let _ = writeln!(out, "}}");
    }

    /// Every reader fn, in item order.
    pub(super) fn readers(&self) -> String {
        let mut out = String::new();
        for item in self.replay.items() {
            match &item.kind {
                ReplayItemKind::Struct(scope) => self.struct_reader(&mut out, item, scope),
                ReplayItemKind::Enum(variants) => self.enum_reader(&mut out, item, variants),
                ReplayItemKind::Alias(value) => self.alias_reader(&mut out, item, value),
                ReplayItemKind::VoidDefinition => {}
            }
        }
        out
    }

    fn reader_open(&self, out: &mut String, item: &ReplayItem) {
        let ident = self.model.item_ident(item.name).to_string();
        let (fn_generics, reader_generics, return_type) = if self.model.needs_lifetime(item.ty) {
            ("<'t>", "<'_, 't>", format!("{ident}<'t>"))
        } else {
            ("", "<'_, '_>", ident.clone())
        };
        let reader = self.reader_fn(item.name);
        let fallible = item.fallible;
        let depth_param = if fallible {
            ", depth: &rt::ReplayDepth"
        } else {
            ""
        };
        let return_type = if fallible {
            format!("::core::result::Result<{return_type}, rt::LimitExceeded>")
        } else {
            return_type
        };
        let _ = writeln!(out, "/// Replay one committed `{ident}` value.");
        let _ = writeln!(
            out,
            "fn {reader}{fn_generics}(t: &mut rt::TraceReader{reader_generics}{depth_param}) -> {return_type} {{"
        );
        if item.enters_depth {
            out.push_str("    let _depth = depth.enter()?;\n");
        }
    }

    fn struct_reader(&self, out: &mut String, item: &ReplayItem, plan: &ReplayScopePlan) {
        let ident = self.model.item_ident(item.name).to_string();
        out.push('\n');
        self.reader_open(out, item);
        out.push_str("    t.expect_struct_open();\n");
        let scope = Scope::struct_body(item.ty, &ident);
        self.field_scope(out, &scope, plan);
        out.push_str("    t.expect_struct_close();\n");
        self.construct(out, &scope, plan, item.fallible);
        out.push_str("}\n");
    }

    fn enum_reader(&self, out: &mut String, item: &ReplayItem, variants: &[ReplayVariantPlan]) {
        let ident = self.model.item_ident(item.name).to_string();
        let variant_idents = rust_scope_idents(
            variants
                .iter()
                .map(|variant| self.interner.resolve(variant.name)),
        );
        out.push('\n');
        self.reader_open(out, item);
        out.push_str("    match t.expect_enum_open() {\n");
        for (variant, variant_ident) in variants.iter().zip(&variant_idents) {
            let indices = arm_pattern(&variant.indices);
            let label = self.interner.resolve(variant.name);
            let _ = writeln!(out, "        // {label}");
            let _ = writeln!(out, "        {indices} => {{");
            let Some(payload) = &variant.payload else {
                let _ = writeln!(out, "            t.expect_enum_close();");
                if item.fallible {
                    let _ = writeln!(out, "            Ok({ident}::{variant_ident})");
                } else {
                    let _ = writeln!(out, "            {ident}::{variant_ident}");
                }
                out.push_str("        }\n");
                continue;
            };

            let scope = Scope::enum_payload(item.ty, &ident, variant_ident);
            self.field_scope(out, &scope, payload);
            out.push_str("            t.expect_enum_close();\n");
            self.construct(out, &scope, payload, item.fallible);
            out.push_str("        }\n");
        }
        let _ = writeln!(
            out,
            "        other => unreachable!(\"trace shape proven at emit: `{ident}` has no variant index {{other}}\"),"
        );
        out.push_str("    }\n");
        out.push_str("}\n");
    }

    fn alias_reader(&self, out: &mut String, item: &ReplayItem, value: &ReplayValuePlan) {
        out.push('\n');
        self.reader_open(out, item);
        let expr = self.value_expr(value, ReadContext::item(item.ty, 1));
        if item.fallible {
            let _ = writeln!(out, "    Ok({expr})");
        } else {
            let _ = writeln!(out, "    {expr}");
        }
        out.push_str("}\n");
    }

    /// The field-collection loop of one struct-like scope: positional locals,
    /// the peek-dispatch loop over member indices, one `Set` consumed per
    /// field value. Enum payloads reuse it with `EnumClose` as the terminator
    /// (payload `Set`s attach directly to the enum frame — the materializer's
    /// contract).
    fn field_scope(&self, out: &mut String, scope: &Scope<'_>, plan: &ReplayScopePlan) {
        let p = indentation(scope.level());
        for (index, field) in plan.fields.iter().enumerate() {
            let _ = writeln!(
                out,
                "{p}let mut v{index} = None; // {}",
                self.interner.resolve(field.name)
            );
        }
        let _ = writeln!(out, "{p}while !t.{}() {{", scope.kind.probe());
        let _ = writeln!(out, "{p}    match t.peek_set() {{");
        for (index, field) in plan.fields.iter().enumerate() {
            let indices = arm_pattern(&field.indices);
            let expr = self.value_expr(&field.value, scope.field_context());
            let _ = writeln!(out, "{p}        // {}", self.interner.resolve(field.name));
            let _ = writeln!(out, "{p}        {indices} => v{index} = Some({expr}),");
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
        scope: &Scope<'_>,
        plan: &ReplayScopePlan,
        fallible: bool,
    ) {
        let p = indentation(scope.level());
        let head = scope.construction_head();
        let field_idents = rust_scope_idents(
            plan.fields
                .iter()
                .map(|field| self.interner.resolve(field.name)),
        );
        if fallible {
            let _ = writeln!(out, "{p}Ok({head} {{");
        } else {
            let _ = writeln!(out, "{p}{head} {{");
        }
        for (index, (field, field_ident)) in plan.fields.iter().zip(&field_idents).enumerate() {
            let name = self.interner.resolve(field.name);
            let _ = writeln!(
                out,
                "{p}    {field_ident}: v{index}.expect(\"field-stability: every accepting path sets `{name}`\"),"
            );
        }
        if fallible {
            let _ = writeln!(out, "{p}}})");
        } else {
            let _ = writeln!(out, "{p}}}");
        }
    }

    /// Read one planned value. The returned expression's first line splices
    /// inline; continuation lines are indented for `level`. `depth` suffixes
    /// array accumulators so nested arrays don't shadow. `cut` is the item
    /// declaration this position renders inside — the box-placement context,
    /// threaded exactly as the type renderer threads it so `Box::new` sits
    /// precisely where the declared type says `Box`.
    fn value_expr(&self, plan: &ReplayValuePlan, context: ReadContext) -> String {
        match plan {
            ReplayValuePlan::Node => "t.expect_node()".to_string(),
            ReplayValuePlan::Nullable(inner) => self.nullable_expr(inner, context),
            ReplayValuePlan::Array(element) => self.array_expr(element, context),
            ReplayValuePlan::Read { item, source } => {
                let call = self.reader_call(*item);
                if self.model.is_boxed_ref(context.type_context, *source) {
                    format!("::std::boxed::Box::new({call})")
                } else {
                    call
                }
            }
        }
    }

    /// `Null` is the whole absent value — one flat null, however many
    /// `Option` layers the type carries; a present value wraps `Some` at
    /// every layer (the VM never nests nulls).
    fn nullable_expr(&self, inner: &ReplayValuePlan, context: ReadContext) -> String {
        let p = indentation(context.level);
        let inner_expr = self.value_expr(inner, context.in_some_branch());
        let mut out = String::new();
        let _ = writeln!(out, "if t.take_null() {{");
        let _ = writeln!(out, "{p}    None");
        let _ = writeln!(out, "{p}}} else {{");
        let _ = writeln!(out, "{p}    Some({inner_expr})");
        let _ = write!(out, "{p}}}");
        out
    }

    fn array_expr(&self, element: &ReplayValuePlan, context: ReadContext) -> String {
        let p = indentation(context.level);
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

/// Where a value expression is emitted. `type_context` decides recursive
/// `Box` placement, `level` is the emitted indentation, and `array_depth`
/// keeps nested array accumulator names distinct.
#[derive(Clone, Copy)]
struct ReadContext {
    type_context: TypeContext,
    level: usize,
    array_depth: usize,
}

impl ReadContext {
    fn item(item_ty: TypeId, level: usize) -> Self {
        Self {
            type_context: TypeContext::item(item_ty),
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
            type_context: self.type_context.array_element(),
            level: self.level + 2,
            array_depth: self.array_depth + 1,
        }
    }
}

/// One struct-like scope from field collection through value construction.
/// The kind fixes the close probe, indentation, and construction head together.
struct Scope<'a> {
    context: ReadContext,
    kind: ScopeKind<'a>,
    name: &'a str,
}

#[derive(Clone, Copy)]
enum ScopeKind<'a> {
    Struct,
    EnumPayload { variant_ident: &'a str },
}

impl ScopeKind<'_> {
    fn probe(self) -> &'static str {
        match self {
            ScopeKind::Struct => "at_struct_close",
            ScopeKind::EnumPayload { .. } => "at_enum_close",
        }
    }
}

impl<'a> Scope<'a> {
    fn struct_body(owner: TypeId, name: &'a str) -> Self {
        Self {
            context: ReadContext::item(owner, 1),
            kind: ScopeKind::Struct,
            name,
        }
    }

    fn enum_payload(owner: TypeId, name: &'a str, variant_ident: &'a str) -> Self {
        Self {
            context: ReadContext::item(owner, 3),
            kind: ScopeKind::EnumPayload { variant_ident },
            name,
        }
    }

    fn field_context(&self) -> ReadContext {
        self.context.field_value()
    }

    fn level(&self) -> usize {
        self.context.level
    }

    fn construction_head(&self) -> String {
        match self.kind {
            ScopeKind::Struct => self.name.to_string(),
            ScopeKind::EnumPayload { variant_ident } => {
                format!("{}::{variant_ident}", self.name)
            }
        }
    }
}

/// `12` or `12 | 27` — the Rust match pattern for planned twin indices.
fn arm_pattern(indices: &[u16]) -> String {
    indices
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(" | ")
}

fn align_to_word(bytes: u64) -> u64 {
    let rem = bytes % WORD_BYTES;
    if rem == 0 {
        return bytes;
    }
    bytes.saturating_add(WORD_BYTES - rem)
}
