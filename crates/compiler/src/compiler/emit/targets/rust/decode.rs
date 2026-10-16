use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use crate::compiler::analyze::refs::DefinitionGraph;
use crate::compiler::analyze::result::ResultSchema;
use crate::compiler::analyze::types::type_shape::TypeId;
use crate::compiler::emit::plan::{
    DecodeCase, DecodeItem, DecodeItemKind, DecodeScope, DecodeValue, ResultDecodePlan,
};
use crate::compiler::emit::sink::indentation;
use crate::compiler::emit::targets::rust::ident::{rust_scope_idents, snake_ident};
use crate::compiler::emit::targets::rust::{TypeContext, TypeModel};
use crate::compiler::ids::{DefId, ResultMemberId};
use crate::core::{Interner, Symbol};

use super::decoder_frame::DecoderFrameEstimator;
use super::entry_names::apply_fn_name;
use super::representation::LifetimeUsage;

pub(super) struct DecoderGen<'m, 'a> {
    model: &'m TypeModel<'a>,
    definitions: &'a DefinitionGraph,
    interner: &'a Interner,
    decode: &'a ResultDecodePlan,
    tables: DecoderTables,
}

struct DecoderTables {
    decoder_fns: HashMap<Symbol, String>,
}

impl DecoderTables {
    fn collect(decode: &ResultDecodePlan, interner: &Interner) -> Self {
        let mut decoder_fns = HashMap::new();
        let mut taken = HashSet::new();
        for item in decode.items() {
            if !item.has_decoder() {
                continue;
            }
            let mut name = format!("decode_{}", snake_ident(interner.resolve(item.name)));
            while !taken.insert(name.clone()) {
                name.push('_');
            }
            decoder_fns.insert(item.name, name);
        }

        Self { decoder_fns }
    }
}

struct ExecutionSignature {
    ident: String,
    type_generics: &'static str,
    method_generics: &'static str,
    usage: LifetimeUsage,
}

impl ExecutionSignature {
    fn for_item(model: &TypeModel<'_>, item: &DecodeItem) -> Self {
        let ident = model.item_ident(item.name).to_string();
        let usage = item
            .output
            .value()
            .map(|type_id| model.lifetime_usage(type_id))
            .unwrap_or_default();
        let (type_generics, method_generics) = match (usage.tree, usage.source) {
            (false, false) => ("", "<'t, 's>"),
            (true, false) => ("<'t>", "<'s>"),
            (false, true) => ("<'s>", "<'t>"),
            (true, true) => ("<'t, 's>", ""),
        };
        Self {
            ident,
            type_generics,
            method_generics,
            usage,
        }
    }
}

impl<'m, 'a> DecoderGen<'m, 'a> {
    pub(super) fn new(
        schema: &'a ResultSchema<'a>,
        model: &'m TypeModel<'a>,
        decode: &'a ResultDecodePlan,
    ) -> Self {
        let tables = DecoderTables::collect(decode, schema.interner);

        Self {
            model,
            definitions: schema.definitions,
            interner: schema.interner,
            decode,
            tables,
        }
    }

    fn decoder_fn(&self, name: Symbol) -> &str {
        self.tables
            .decoder_fns
            .get(&name)
            .expect("every value item has a decoder")
    }

    fn item_named(&self, name: Symbol) -> &DecodeItem {
        self.decode.item(name)
    }

    fn decoder_call(&self, name: Symbol) -> String {
        let decoder = self.decoder_fn(name);
        let item = self.item_named(name);
        if item.fallible {
            format!("{decoder}(decoder, depth)?")
        } else {
            format!("{decoder}(decoder)")
        }
    }

    pub(super) fn max_decoder_frame_bytes(&self) -> u64 {
        DecoderFrameEstimator::new(self.model, self.decode).max_bytes()
    }

    pub(super) fn uses_decode_depth(&self) -> bool {
        self.decode
            .items()
            .any(|item| item.has_decoder() && item.fallible)
    }

    pub(super) fn execution_api(&self, entrypoints: impl Iterator<Item = DefId>) -> String {
        let mut out = String::new();
        for def_id in entrypoints {
            let name = self.definitions.definition(def_id).name();
            let def = self.interner.resolve(name).to_string();
            out.push('\n');
            let item = self.decode.item(name);
            self.execution_impl(&mut out, &def, item);
        }
        out
    }

    fn execution_impl(&self, out: &mut String, def: &str, item: &DecodeItem) {
        let sig = ExecutionSignature::for_item(self.model, item);
        let apply_fn = apply_fn_name(def);
        let result_type = if item.has_decoder() {
            "Self"
        } else {
            "rt::Node<'t>"
        };
        let application_generics = if item.has_decoder() { "" } else { "<'t>" };
        let (node_type, engine_type) = if sig.usage.tree || !item.has_decoder() {
            ("rt::Node<'t>", "rt::Engine<'t>")
        } else {
            ("rt::Node<'_>", "rt::Engine<'_>")
        };
        let engine_constructor = if item.has_decoder() {
            "rt::Engine::new"
        } else {
            "rt::Engine::new_match_only"
        };
        let source_ref = if sig.usage.source { "&'s str" } else { "&str" };
        let _ = writeln!(
            out,
            "impl{} {}{} {{",
            sig.type_generics, sig.ident, sig.type_generics
        );
        let generics = sig.method_generics;
        let _ = writeln!(
            out,
            "    /// Apply `{def}` once at a suitable node. A wrong node kind is no-match."
        );
        let _ = writeln!(out, "    pub fn exec{application_generics}(");
        let _ = writeln!(out, "        node: {node_type},");
        let _ = writeln!(out, "        source: {source_ref},");
        let _ = writeln!(
            out,
            "    ) -> ::core::result::Result<::core::option::Option<{result_type}>, rt::LimitExceeded> {{"
        );
        let _ = writeln!(
            out,
            "        let mut budget = matcher::execution_budget(node);"
        );
        let _ = writeln!(
            out,
            "        let mut eng = {engine_constructor}(node.walk());"
        );
        let _ = writeln!(out, "        Self::apply(&mut eng, source, &mut budget)");
        let _ = writeln!(out, "    }}\n");
        let _ = writeln!(
            out,
            "    /// Search inclusively in source-order DFS preorder, retaining nested matches."
        );
        let _ = writeln!(
            out,
            "    /// Matching and traversal share compiled-in limits. An error ends iteration."
        );
        let _ = writeln!(out, "    pub fn each{generics}(");
        let _ = writeln!(out, "        scope: rt::Node<'t>,");
        let _ = writeln!(out, "        source: &'s str,");
        if item.has_decoder() {
            let _ = writeln!(
                out,
                "    ) -> impl ::core::iter::FusedIterator<Item = ::core::result::Result<Self, rt::LimitExceeded>>"
            );
            let _ = writeln!(out, "    + use<'t, 's> {{");
        } else {
            let _ = writeln!(out, "    ) -> impl ::core::iter::FusedIterator<");
            let _ = writeln!(
                out,
                "        Item = ::core::result::Result<{result_type}, rt::LimitExceeded>,"
            );
            let _ = writeln!(out, "    > + use<'t, 's> {{");
        }
        let _ = writeln!(
            out,
            "        let mut eng = {engine_constructor}(scope.walk());"
        );
        let _ = writeln!(out, "        rt::Traversal::new(");
        let _ = writeln!(out, "            scope,");
        let _ = writeln!(out, "            matcher::execution_budget(scope),");
        let _ = writeln!(
            out,
            "            move |node: rt::Node<'t>, budget: &mut rt::ExecutionBudget| {{"
        );
        let _ = writeln!(out, "                eng.reset(node);");
        let _ = writeln!(out, "                Self::apply(&mut eng, source, budget)");
        let _ = writeln!(out, "            }},");
        let _ = writeln!(out, "        )");
        let _ = writeln!(out, "    }}\n");
        let _ = writeln!(out, "    /// Consume the first item of `each`.");
        let _ = writeln!(out, "    pub fn find{application_generics}(");
        let _ = writeln!(out, "        scope: {node_type},");
        let _ = writeln!(out, "        source: {source_ref},");
        let _ = writeln!(
            out,
            "    ) -> ::core::result::Result<::core::option::Option<{result_type}>, rt::LimitExceeded> {{"
        );
        let _ = writeln!(
            out,
            "        <Self as rt::Execute<'_, '_>>::find(scope, source)"
        );
        let _ = writeln!(out, "    }}\n");
        let _ = writeln!(
            out,
            "    /// Collect `each`, returning an error rather than a partial vector."
        );
        let _ = writeln!(out, "    pub fn find_all{application_generics}(");
        let _ = writeln!(out, "        scope: {node_type},");
        let _ = writeln!(out, "        source: {source_ref},");
        let _ = writeln!(
            out,
            "    ) -> ::core::result::Result<::std::vec::Vec<{result_type}>, rt::LimitExceeded> {{"
        );
        let _ = writeln!(
            out,
            "        <Self as rt::Execute<'_, '_>>::find_all(scope, source)"
        );
        let _ = writeln!(out, "    }}\n");
        let _ = writeln!(out, "    fn apply{application_generics}(");
        let _ = writeln!(out, "        eng: &mut {engine_type},");
        let _ = writeln!(out, "        source: {source_ref},");
        let _ = writeln!(out, "        budget: &mut rt::ExecutionBudget,");
        let _ = writeln!(
            out,
            "    ) -> ::core::result::Result<::core::option::Option<{result_type}>, rt::LimitExceeded> {{"
        );
        if !item.has_decoder() {
            let _ = writeln!(out, "        let node = eng.node();");
            let _ = writeln!(
                out,
                "        Ok(matcher::{apply_fn}(eng, source, budget)?.map(|_| node))"
            );
        } else {
            self.decode_application(out, item, &apply_fn);
        }
        let _ = writeln!(out, "    }}");
        let _ = writeln!(out, "}}\n");
        let _ = writeln!(
            out,
            "impl<'t, 's> rt::Execute<'t, 's> for {}{} {{",
            sig.ident, sig.type_generics
        );
        let _ = writeln!(out, "    type Item = {result_type};\n");
        let _ = writeln!(out, "    fn exec(");
        let _ = writeln!(out, "        node: rt::Node<'t>,");
        let _ = writeln!(out, "        source: &'s str,");
        let _ = writeln!(
            out,
            "    ) -> ::core::result::Result<::core::option::Option<{result_type}>, rt::LimitExceeded> {{"
        );
        let _ = writeln!(out, "        Self::exec(node, source)");
        let _ = writeln!(out, "    }}\n");
        let _ = writeln!(out, "    fn each(");
        let _ = writeln!(out, "        scope: rt::Node<'t>,");
        let _ = writeln!(out, "        source: &'s str,");
        let _ = writeln!(
            out,
            "    ) -> impl ::core::iter::FusedIterator<Item = ::core::result::Result<{result_type}, rt::LimitExceeded>>"
        );
        let _ = writeln!(out, "    {{");
        let _ = writeln!(out, "        Self::each(scope, source)");
        let _ = writeln!(out, "    }}");
        let _ = writeln!(out, "}}");
    }

    fn decode_application(&self, out: &mut String, item: &DecodeItem, apply_fn: &str) {
        let decoder_fn = self.decoder_fn(item.name);
        let _ = writeln!(
            out,
            "        let Some(journal) = matcher::{apply_fn}(eng, source, budget)? else {{"
        );
        let _ = writeln!(out, "            return Ok(None);");
        let _ = writeln!(out, "        }};");
        let _ = writeln!(
            out,
            "        let mut decoder = rt::ResultDecoder::new(journal.output_events(), source);"
        );
        if item.fallible {
            let _ = writeln!(
                out,
                "        let depth = rt::DecodeDepth::new(matcher::MAX_DECODE_DEPTH);"
            );
            let _ = writeln!(
                out,
                "        let value = {decoder_fn}(&mut decoder, &depth)?;"
            );
        } else {
            let _ = writeln!(out, "        let value = {decoder_fn}(&mut decoder);");
        }
        let _ = writeln!(out, "        decoder.finish();");
        let _ = writeln!(out, "        Ok(Some(value))");
    }

    pub(super) fn decoders(&self) -> String {
        let mut out = String::new();
        for item in self.decode.items() {
            match &item.kind {
                DecodeItemKind::Record(scope) => self.record_decoder(&mut out, item, scope),
                DecodeItemKind::Variant(cases) => self.variant_decoder(&mut out, item, cases),
                DecodeItemKind::Alias(value) => self.alias_decoder(&mut out, item, value),
                DecodeItemKind::MatchOnlyDefinition => {}
            }
        }
        out
    }

    fn decoder_open(&self, out: &mut String, item: &DecodeItem) {
        let ident = self.model.item_ident(item.name).to_string();
        let fn_generics = "<'t, 's>";
        let decoder_generics = "<'_, 't, 's>";
        let return_type = format!("{ident}{}", lifetime_args(self.model, item.value_type()));
        let decoder_fn = self.decoder_fn(item.name);
        let fallible = item.fallible;
        let depth_param = fallible.then_some("depth: &rt::DecodeDepth");
        let return_type = if fallible {
            format!("::core::result::Result<{return_type}, rt::LimitExceeded>")
        } else {
            return_type
        };
        let _ = writeln!(out, "/// Decode one committed `{ident}` value.");
        let mut params = vec![format!("decoder: &mut rt::ResultDecoder{decoder_generics}")];
        params.extend(depth_param.map(str::to_string));
        let compact = format!(
            "fn {decoder_fn}{fn_generics}({}) -> {return_type} {{",
            params.join(", ")
        );
        if compact.len() <= 100 {
            let _ = writeln!(out, "{compact}");
        } else {
            let _ = writeln!(out, "fn {decoder_fn}{fn_generics}(");
            for param in params {
                let _ = writeln!(out, "    {param},");
            }
            let _ = writeln!(out, ") -> {return_type} {{");
        }
        if item.enters_depth {
            out.push_str("    let _depth = depth.enter()?;\n");
        }
    }

    fn record_decoder(&self, out: &mut String, item: &DecodeItem, plan: &DecodeScope) {
        let ident = self.model.item_ident(item.name).to_string();
        out.push('\n');
        self.decoder_open(out, item);
        out.push_str("    decoder.expect_record_open();\n");
        let scope = Scope::struct_body(item.value_type(), &ident);
        self.field_scope(out, &scope, plan);
        out.push_str("    decoder.expect_record_close();\n");
        self.construct(out, &scope, plan, item.fallible);
        out.push_str("}\n");
    }

    fn variant_decoder(&self, out: &mut String, item: &DecodeItem, cases: &[DecodeCase]) {
        let ident = self.model.item_ident(item.name).to_string();
        let variant_idents = rust_scope_idents(
            cases
                .iter()
                .map(|variant| self.interner.resolve(variant.name)),
        );
        out.push('\n');
        self.decoder_open(out, item);
        out.push_str("    match decoder.expect_variant_open() {\n");
        for (case, variant_ident) in cases.iter().zip(&variant_idents) {
            let indices = arm_pattern(&case.indices);
            let label = self.interner.resolve(case.name);
            let _ = writeln!(out, "        // {label}");
            let _ = writeln!(out, "        {indices} => {{");
            let Some(payload) = &case.payload else {
                let _ = writeln!(out, "            decoder.expect_variant_close();");
                if item.fallible {
                    let _ = writeln!(out, "            Ok({ident}::{variant_ident})");
                } else {
                    let _ = writeln!(out, "            {ident}::{variant_ident}");
                }
                out.push_str("        }\n");
                continue;
            };

            let scope = Scope::variant_payload(item.value_type(), &ident, variant_ident);
            self.field_scope(out, &scope, payload);
            out.push_str("            decoder.expect_variant_close();\n");
            self.construct(out, &scope, payload, item.fallible);
            out.push_str("        }\n");
        }
        unreachable_arm(
            out,
            "        ",
            &format!(
                "journal shape proven at emit: `{ident}` has no variant case at member index {{other}}"
            ),
        );
        out.push_str("    }\n");
        out.push_str("}\n");
    }

    fn alias_decoder(&self, out: &mut String, item: &DecodeItem, value: &DecodeValue) {
        out.push('\n');
        self.decoder_open(out, item);
        let expr = self.value_expr(value, DecodeContext::item(item.value_type(), 1));
        if item.fallible {
            let _ = writeln!(out, "    Ok({expr})");
        } else {
            let _ = writeln!(out, "    {expr}");
        }
        out.push_str("}\n");
    }

    fn field_scope(&self, out: &mut String, scope: &Scope<'_>, plan: &DecodeScope) {
        let p = indentation(scope.level());
        for (index, field) in plan.fields.iter().enumerate() {
            let _ = writeln!(
                out,
                "{p}let mut v{index} = None; // {}",
                self.interner.resolve(field.name)
            );
        }
        let _ = writeln!(out, "{p}while !decoder.{}() {{", scope.kind.probe());
        let _ = writeln!(out, "{p}    match decoder.peek_record_set() {{");
        for (index, field) in plan.fields.iter().enumerate() {
            let indices = arm_pattern(&field.indices);
            let context = scope.field_context();
            let mut expr = self.value_expr(&field.value, context);
            let _ = writeln!(out, "{p}        // {}", self.interner.resolve(field.name));
            if expr.contains('\n') {
                expr = self.value_expr(&field.value, context.indented());
                let _ = writeln!(out, "{p}        {indices} => {{");
                let _ = writeln!(out, "{p}            v{index} = Some({expr})");
                let _ = writeln!(out, "{p}        }}");
            } else {
                let _ = writeln!(out, "{p}        {indices} => v{index} = Some({expr}),");
            }
        }
        unreachable_arm(
            out,
            &format!("{p}        "),
            &format!(
                "journal shape proven at emit: `{}` has no member index {{other}}",
                scope.name
            ),
        );
        let _ = writeln!(out, "{p}    }}");
        let _ = writeln!(out, "{p}    decoder.expect_record_set();");
        let _ = writeln!(out, "{p}}}");
    }

    fn construct(&self, out: &mut String, scope: &Scope<'_>, plan: &DecodeScope, fallible: bool) {
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
            let compact = format!(
                "{p}    {field_ident}: v{index}.expect(\"field-stability: every accepting path sets `{name}`\"),"
            );
            if compact.len() <= 100 {
                let _ = writeln!(out, "{compact}");
            } else {
                let _ = writeln!(out, "{p}    {field_ident}: v{index}.expect(");
                let _ = writeln!(
                    out,
                    "{p}        \"field-stability: every accepting path sets `{name}`\","
                );
                let _ = writeln!(out, "{p}    ),");
            }
        }
        if fallible {
            let _ = writeln!(out, "{p}}})");
        } else {
            let _ = writeln!(out, "{p}}}");
        }
    }

    fn value_expr(&self, plan: &DecodeValue, context: DecodeContext) -> String {
        match plan {
            DecodeValue::Node => "decoder.expect_node()".to_string(),
            DecodeValue::Text => "decoder.expect_str()".to_string(),
            DecodeValue::Bool => "decoder.expect_bool()".to_string(),
            DecodeValue::Option(inner) => self.option_expr(inner, context),
            DecodeValue::List(element) => self.list_expr(element, context),
            DecodeValue::Nested { item, source_type } => {
                let call = self.decoder_call(*item);
                if self.model.is_boxed_ref(context.type_context, *source_type) {
                    format!("::std::boxed::Box::new({call})")
                } else {
                    call
                }
            }
        }
    }

    fn option_expr(&self, inner: &DecodeValue, context: DecodeContext) -> String {
        let p = indentation(context.level);
        let inner_expr = self.value_expr(inner, context.in_some_branch());
        let mut out = String::new();
        let _ = writeln!(out, "if decoder.take_absent() {{");
        let _ = writeln!(out, "{p}    None");
        let _ = writeln!(out, "{p}}} else {{");
        let _ = writeln!(out, "{p}    Some({inner_expr})");
        let _ = write!(out, "{p}}}");
        out
    }

    fn list_expr(&self, element: &DecodeValue, context: DecodeContext) -> String {
        let p = indentation(context.level);
        let items = format!("items{}", context.list_depth);
        let elem = self.value_expr(element, context.list_element());
        let mut out = String::new();
        let _ = writeln!(out, "{{");
        let _ = writeln!(out, "{p}    decoder.expect_list_open();");
        let _ = writeln!(out, "{p}    let mut {items} = ::std::vec::Vec::new();");
        let _ = writeln!(out, "{p}    while !decoder.at_list_close() {{");
        let _ = writeln!(out, "{p}        {items}.push({elem});");
        let _ = writeln!(out, "{p}        decoder.expect_array_push();");
        let _ = writeln!(out, "{p}    }}");
        let _ = writeln!(out, "{p}    decoder.expect_list_close();");
        let _ = writeln!(out, "{p}    {items}");
        let _ = write!(out, "{p}}}");
        out
    }
}

#[derive(Clone, Copy)]
struct DecodeContext {
    type_context: TypeContext,
    level: usize,
    list_depth: usize,
}

impl DecodeContext {
    fn item(item_ty: TypeId, level: usize) -> Self {
        Self {
            type_context: TypeContext::item(item_ty),
            level,
            list_depth: 0,
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

    fn list_element(self) -> Self {
        Self {
            type_context: self.type_context.list_element(),
            level: self.level + 2,
            list_depth: self.list_depth + 1,
        }
    }

    fn indented(self) -> Self {
        Self {
            level: self.level + 1,
            ..self
        }
    }
}

struct Scope<'a> {
    context: DecodeContext,
    kind: ScopeKind<'a>,
    name: &'a str,
}

#[derive(Clone, Copy)]
enum ScopeKind<'a> {
    Struct,
    VariantPayload { variant_ident: &'a str },
}

impl ScopeKind<'_> {
    fn probe(self) -> &'static str {
        match self {
            ScopeKind::Struct => "at_record_close",
            ScopeKind::VariantPayload { .. } => "at_variant_close",
        }
    }
}

impl<'a> Scope<'a> {
    fn struct_body(owner: TypeId, name: &'a str) -> Self {
        Self {
            context: DecodeContext::item(owner, 1),
            kind: ScopeKind::Struct,
            name,
        }
    }

    fn variant_payload(owner: TypeId, name: &'a str, variant_ident: &'a str) -> Self {
        Self {
            context: DecodeContext::item(owner, 3),
            kind: ScopeKind::VariantPayload { variant_ident },
            name,
        }
    }

    fn field_context(&self) -> DecodeContext {
        self.context.field_value()
    }

    fn level(&self) -> usize {
        self.context.level
    }

    fn construction_head(&self) -> String {
        match self.kind {
            ScopeKind::Struct => self.name.to_string(),
            ScopeKind::VariantPayload { variant_ident } => {
                format!("{}::{variant_ident}", self.name)
            }
        }
    }
}

fn arm_pattern(indices: &[ResultMemberId]) -> String {
    indices
        .iter()
        .map(|index| index.raw().to_string())
        .collect::<Vec<_>>()
        .join(" | ")
}

fn lifetime_args(model: &TypeModel<'_>, ty: TypeId) -> &'static str {
    let usage = model.lifetime_usage(ty);
    match (usage.tree, usage.source) {
        (false, false) => "",
        (true, false) => "<'t>",
        (false, true) => "<'s>",
        (true, true) => "<'t, 's>",
    }
}

fn unreachable_arm(out: &mut String, indent: &str, message: &str) {
    let message = format!("{message:?}");
    let compact = format!("{indent}other => unreachable!({message}),");
    if compact.len() <= 100 {
        let _ = writeln!(out, "{compact}");
        return;
    }

    let _ = writeln!(out, "{indent}other => {{");
    let call_indent = format!("{indent}    ");
    let compact_call = format!("{call_indent}unreachable!({message})");
    if compact_call.len() <= 100 {
        let _ = writeln!(out, "{compact_call}");
    } else {
        let _ = writeln!(out, "{call_indent}unreachable!(");
        let _ = writeln!(out, "{call_indent}    {message}");
        let _ = writeln!(out, "{call_indent})");
    }
    let _ = writeln!(out, "{indent}}}");
}
