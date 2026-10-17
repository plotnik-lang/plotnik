use std::fmt::Write as _;

use super::model::{FieldModel, ItemModel, ItemShape, Key, ResultModel};
use super::representation::LifetimeUsage;
use super::template::splice;

/// The result-side code of a generated module: type checks, result-family
/// bindings, JSON shapes, execution points, the logical schema, and the
/// typed publisher that builds region-owned result graphs from walk steps.
pub(super) struct PublishGen<'m> {
    model: &'m ResultModel,
}

/// One executable entrypoint: the item it returns and its matcher entry.
pub(super) struct EntryPoint<'a> {
    pub(super) item: &'a ItemModel,
    /// The matcher-module type implementing `rt::Entry`.
    pub(super) entry: String,
}

impl<'m> PublishGen<'m> {
    pub(super) fn new(model: &'m ResultModel) -> Self {
        Self { model }
    }

    /// Compile-time size checks and result-family bindings for every type.
    pub(super) fn type_checks(&self) -> String {
        let mut out = String::new();
        for item in self.model.items() {
            if matches!(item.shape, ItemShape::MatchOnly) {
                continue;
            }
            let usage = self.model.item_usage(item.name);
            let ty = format!("{}{}", item.ident, static_params(usage));
            let _ = writeln!(
                out,
                "const _: () = assert!(::core::mem::size_of::<{ty}>() <= rt::GENERATED_VALUE_CAP);"
            );
        }
        out.push('\n');
        for item in self.model.items() {
            if !matches!(item.shape, ItemShape::Record(_) | ItemShape::Variant(_)) {
                continue;
            }
            let usage = self.model.item_usage(item.name);
            let _ = writeln!(
                out,
                "rt::result_family!({}{});",
                item.ident,
                usage.params("'r")
            );
        }
        out
    }

    /// `rt::json::Json` (and `Fields`) for every record and variant.
    pub(super) fn json_impls(&self) -> String {
        let mut out = String::new();
        for item in self.model.items() {
            match &item.shape {
                ItemShape::Record(fields) => self.record_json(&mut out, item, fields),
                ItemShape::Variant(_) => self.variant_json(&mut out, item),
                ItemShape::Alias(_) | ItemShape::MatchOnly => {}
            }
        }
        out
    }

    fn record_json(&self, out: &mut String, item: &ItemModel, fields: &[FieldModel]) {
        let ty = self.anonymous(item);
        let count = fields.len();
        let _ = writeln!(out, "\nimpl rt::json::Json for {ty} {{");
        let _ = writeln!(out, "    fn shape(&self) -> rt::json::Shape<'_> {{");
        let _ = writeln!(out, "        rt::json::Shape::Record(self)");
        let _ = writeln!(out, "    }}");
        let _ = writeln!(out, "}}\n");
        let _ = writeln!(out, "impl rt::json::Fields for {ty} {{");
        let _ = writeln!(out, "    fn count(&self) -> usize {{");
        let _ = writeln!(out, "        {count}");
        let _ = writeln!(out, "    }}\n");
        let _ = writeln!(
            out,
            "    fn field(&self, index: usize) -> (&'static str, &dyn rt::json::Json) {{"
        );
        if fields.is_empty() {
            let _ = writeln!(
                out,
                "        unreachable!(\"an empty record has no field {{index}}\")"
            );
            let _ = writeln!(out, "    }}\n}}");
            return;
        }

        let _ = writeln!(out, "        match index {{");
        for (index, field) in fields.iter().enumerate() {
            let _ = writeln!(
                out,
                "            {index} => ({:?}, &self.{}),",
                field.name, field.ident
            );
        }
        let _ = writeln!(
            out,
            "            _ => unreachable!(\"`{}` has {count} fields\"),",
            item.display
        );
        let _ = writeln!(out, "        }}");
        let _ = writeln!(out, "    }}");
        let _ = writeln!(out, "}}");
    }

    fn variant_json(&self, out: &mut String, item: &ItemModel) {
        let ItemShape::Variant(cases) = &item.shape else {
            unreachable!("variant JSON for a variant item");
        };
        let ty = self.anonymous(item);
        let ident = &item.ident;
        let _ = writeln!(out, "\nimpl rt::json::Json for {ty} {{");
        let _ = writeln!(out, "    fn shape(&self) -> rt::json::Shape<'_> {{");
        let _ = writeln!(out, "        match self {{");
        for case in cases {
            let pattern = case_pattern(ident, &case.ident, case.payload.is_some());
            let data = if case.payload.is_some() {
                "Some(self)"
            } else {
                "None"
            };
            let _ = writeln!(
                out,
                "            {pattern} => rt::json::Shape::Variant {{ tag: {:?}, data: {data} }},",
                case.name
            );
        }
        let _ = writeln!(out, "        }}");
        let _ = writeln!(out, "    }}");
        let _ = writeln!(out, "}}\n");

        if cases.iter().all(|case| case.payload.is_none()) {
            return;
        }

        let _ = writeln!(out, "impl rt::json::Fields for {ty} {{");
        let _ = writeln!(out, "    fn count(&self) -> usize {{");
        let _ = writeln!(out, "        match self {{");
        for case in cases {
            let count = case.payload.as_ref().map_or(0, |(_, fields)| fields.len());
            let pattern = case_pattern(ident, &case.ident, case.payload.is_some());
            let _ = writeln!(out, "            {pattern} => {count},");
        }
        let _ = writeln!(out, "        }}");
        let _ = writeln!(out, "    }}\n");
        let _ = writeln!(
            out,
            "    fn field(&self, index: usize) -> (&'static str, &dyn rt::json::Json) {{"
        );
        let _ = writeln!(out, "        match (self, index) {{");
        for case in cases {
            let Some((_, fields)) = &case.payload else {
                continue;
            };
            for (index, field) in fields.iter().enumerate() {
                let binding = &field.ident;
                let _ = writeln!(
                    out,
                    "            ({ident}::{} {{ {binding}, .. }}, {index}) => ({:?}, {binding}),",
                    case.ident, field.name
                );
            }
        }
        let _ = writeln!(
            out,
            "            _ => unreachable!(\"`{}` case has no field {{index}}\"),",
            item.display
        );
        let _ = writeln!(out, "        }}");
        let _ = writeln!(out, "    }}");
        let _ = writeln!(out, "}}");
    }

    /// Execution points and `rt::Publish` for each executable entrypoint.
    pub(super) fn execution_api(&self, entries: &[EntryPoint<'_>]) -> String {
        let mut out = String::new();
        for entry in entries {
            out.push('\n');
            match entry.item.shape {
                ItemShape::MatchOnly => self.match_only_api(&mut out, entry),
                ItemShape::Record(_) | ItemShape::Variant(_) => self.value_api(&mut out, entry),
                ItemShape::Alias(_) => unreachable!("entrypoints produce records or variants"),
            }
        }
        out
    }

    fn value_api(&self, out: &mut String, entry: &EntryPoint<'_>) {
        let item = entry.item;
        let usage = self.model.item_usage(item.name);
        let params = usage.params("'r");
        let ty = format!("{}{params}", item.ident);
        let root = item.ty.expect("a value entrypoint has a schema type");
        let tree_param = if usage.tree { "" } else { "'t" };
        let source_param = if usage.source { "" } else { "'s" };
        let method_generics = method_generics(&[tree_param, source_param]);
        let exec_generics = method_generics.clone();
        let capture = capture_list(usage);
        let impl_generics = impl_generics(usage, true);
        let all_generics = impl_generics_all(usage);
        let rust_root = self.model.rust_type(root, "'a");
        splice(
            out,
            "",
            VALUE_API,
            &[
                ("DEF", &item.display),
                ("IMPL", &impl_generics),
                ("ALL", &all_generics),
                ("TY", &ty),
                ("GENERICS", &method_generics),
                ("EXEC_GENERICS", &exec_generics),
                ("CAPTURE", &capture),
                ("ENTRY", &entry.entry),
                ("ROOT", &root.to_string()),
                ("ROOT_TY", &rust_root),
                ("STACK", &format!("t{root}")),
            ],
        );
    }

    fn match_only_api(&self, out: &mut String, entry: &EntryPoint<'_>) {
        splice(
            out,
            "",
            MATCH_ONLY_API,
            &[
                ("DEF", &entry.item.display),
                ("TY", &entry.item.ident),
                ("ENTRY", &entry.entry),
            ],
        );
    }

    /// The logical schema and layout tables, and the typed publisher.
    pub(super) fn publisher(&self, entries: &[EntryPoint<'_>]) -> String {
        let roots: Vec<_> = entries.iter().filter_map(|entry| entry.item.ty).collect();
        if roots.is_empty() {
            return String::new();
        }

        let mut out = String::new();
        out.push_str("\n/// Typed publication of a region-owned result graph from walk steps.\n");
        out.push_str("mod publish {\n");
        out.push_str("    use super::*;\n");
        let mut body = String::new();
        self.schema_tables(&mut body);
        for root in roots {
            self.stacks(&mut body, root);
        }
        body.push_str(POP_FN);
        for line in body.lines() {
            if line.is_empty() {
                out.push('\n');
            } else {
                let _ = writeln!(out, "    {line}");
            }
        }
        out.push_str("}\n");
        out
    }

    fn schema_tables(&self, out: &mut String) {
        let mut types = Vec::new();
        let mut fields = Vec::new();
        let mut cases = Vec::new();
        let mut layouts = Vec::new();
        for (id, key) in self.model.keys() {
            types.push(self.schema_ty(key, &mut fields, &mut cases));
            layouts.push(self.layout(id, key));
        }

        let mut members = vec!["rt::Member::UNUSED".to_string(); self.model.member_count()];
        for (id, key) in self.model.keys() {
            let Key::Item(name) = key else {
                continue;
            };
            match &self.model.item(name).shape {
                ItemShape::Record(record) => note_fields(&mut members, id, record),
                ItemShape::Variant(variant) => {
                    for (index, case) in variant.iter().enumerate() {
                        for member in &case.members {
                            members[usize::from(member.raw())] =
                                format!("rt::Member::new({id}, {index})");
                        }
                        if let Some((payload, record)) = &case.payload {
                            note_fields(&mut members, *payload, record);
                        }
                    }
                }
                ItemShape::Alias(_) | ItemShape::MatchOnly => {}
            }
        }

        out.push_str("\n/// The logical result schema shared with the runtime walk and JSON.\n");
        out.push_str("pub(super) const SCHEMA: rt::Schema<'static> = rt::Schema {\n");
        table(out, "types", &types);
        table(out, "fields", &fields);
        table(out, "cases", &cases);
        table(out, "members", &members);
        out.push_str("};\n");
        out.push_str("\n/// Native layout of each schema type, for sizing result blocks.\n");
        out.push_str("pub(super) const LAYOUTS: &[::std::alloc::Layout] = &[\n");
        for layout in layouts {
            let _ = writeln!(out, "    {layout},");
        }
        out.push_str("];\n");
    }

    fn schema_ty(&self, key: Key, fields: &mut Vec<String>, cases: &mut Vec<String>) -> String {
        match key {
            Key::Node => "rt::Ty::Node".to_string(),
            Key::Text => "rt::Ty::Text".to_string(),
            Key::Bool => "rt::Ty::Bool".to_string(),
            Key::Option(inner) => format!("rt::Ty::Option({inner})"),
            Key::List(inner, minimum) => format!(
                "rt::Ty::List {{ element: {inner}, minimum: {} }}",
                u32::from(minimum)
            ),
            Key::Ref(inner) => format!("rt::Ty::Ref({inner})"),
            Key::Payload(item, case) => {
                let ItemShape::Variant(variant) = &self.model.item(item).shape else {
                    unreachable!("a payload belongs to a variant item");
                };
                let (_, record) = variant[case as usize]
                    .payload
                    .as_ref()
                    .expect("a payload key belongs to a payload case");
                record_ty(fields, record)
            }
            Key::Item(name) => match &self.model.item(name).shape {
                ItemShape::Record(record) => record_ty(fields, record),
                ItemShape::Variant(variant) => {
                    let start = cases.len();
                    for case in variant {
                        let payload = case
                            .payload
                            .as_ref()
                            .map_or("rt::Case::NO_PAYLOAD".to_string(), |(ty, _)| ty.to_string());
                        cases.push(format!(
                            "rt::Case {{ name: {:?}, payload: {payload} }}",
                            case.name
                        ));
                    }
                    format!(
                        "rt::Ty::Variant {{ cases: rt::Span::new({start}, {}) }}",
                        variant.len()
                    )
                }
                ItemShape::Alias(body) => self.schema_ty(self.model.key(*body), fields, cases),
                ItemShape::MatchOnly => unreachable!("a match-only item has no value"),
            },
        }
    }

    fn layout(&self, id: u32, key: Key) -> String {
        if let Key::Payload(..) = key {
            return "::std::alloc::Layout::new::<()>()".to_string();
        }
        let ty = self.static_type(id);
        format!("::std::alloc::Layout::new::<{ty}>()")
    }

    /// A schema type's Rust type with every lifetime `'static`, for layouts.
    fn static_type(&self, id: u32) -> String {
        self.model
            .rust_type(id, "'static")
            .replace("'t", "'static")
            .replace("'s ", "'static ")
            .replace("'s>", "'static>")
            .replace("'s,", "'static,")
    }

    fn reachable(&self, root: u32) -> std::collections::HashSet<u32> {
        let mut seen = std::collections::HashSet::new();
        let mut pending = vec![root];
        while let Some(id) = pending.pop() {
            if !seen.insert(id) {
                continue;
            }

            match self.model.key(id) {
                Key::Option(inner) | Key::List(inner, _) | Key::Ref(inner) => pending.push(inner),
                Key::Item(name) => match &self.model.item(name).shape {
                    ItemShape::Alias(inner) => pending.push(*inner),
                    ItemShape::Record(fields) => {
                        pending.extend(fields.iter().map(|field| field.ty))
                    }
                    ItemShape::Variant(cases) => {
                        for case in cases {
                            if let Some((_, fields)) = &case.payload {
                                pending.extend(fields.iter().map(|field| field.ty));
                            }
                        }
                    }
                    ItemShape::MatchOnly => unreachable!("match-only items have no schema type"),
                },
                Key::Node | Key::Text | Key::Bool => {}
                Key::Payload(..) => unreachable!("payloads have no stacks"),
            }
        }
        seen
    }

    fn stacks(&self, out: &mut String, root: u32) {
        let reachable = self.reachable(root);
        let typed: Vec<(u32, Key)> = self
            .model
            .keys()
            .filter(|(id, _)| reachable.contains(id))
            .collect();

        out.push_str(
            "\n/// One value stack per schema type, plus one writer stack per list type.\n",
        );
        let _ = writeln!(out, "pub(super) struct Stacks{root}<'a, 't, 's> {{");
        for &(id, key) in &typed {
            let ty = self.model.rust_type(id, "'a");
            let _ = writeln!(out, "    pub(super) t{id}: rt::Buffer<{ty}>,");
            if let Key::List(element, _) = self.list_key(key) {
                let element = self.model.rust_type(element, "'a");
                let _ = writeln!(out, "    w{id}: rt::Buffer<rt::Slice<'a, {element}>>,");
            }
        }
        out.push_str("    _lifetimes: ::core::marker::PhantomData<(&'a (), &'t (), &'s ())>,\n");
        out.push_str("}\n\n");

        let _ = writeln!(out, "impl<'a, 't, 's> Stacks{root}<'a, 't, 's> {{");
        out.push_str("    pub(super) fn new() -> Self {\n");
        out.push_str("        Self {\n");
        for &(id, key) in &typed {
            let _ = writeln!(out, "            t{id}: rt::Buffer::new(),");
            if let Key::List(..) = self.list_key(key) {
                let _ = writeln!(out, "            w{id}: rt::Buffer::new(),");
            }
        }
        out.push_str("            _lifetimes: ::core::marker::PhantomData,\n");
        out.push_str("        }\n");
        out.push_str("    }\n\n");

        out.push_str(RUN_OPEN);
        for &(id, key) in &typed {
            self.step_arms(out, id, key);
        }
        out.push_str(RUN_CLOSE);

        for &(id, key) in &typed {
            self.close_fn(out, id, key);
        }
        out.push_str("}\n\n");
    }

    /// The structural key behind an alias item.
    fn list_key(&self, key: Key) -> Key {
        match key {
            Key::Item(name) => match &self.model.item(name).shape {
                ItemShape::Alias(body) => self.list_key(self.model.key(*body)),
                _ => key,
            },
            _ => key,
        }
    }

    fn step_arms(&self, out: &mut String, id: u32, key: Key) {
        match self.list_key(key) {
            Key::Node => {
                let _ = writeln!(
                    out,
                    "                rt::Step::Node {{ ty: {id}, node }} => self.t{id}.push(meter, node)?,"
                );
            }
            Key::Text => {
                let _ = writeln!(
                    out,
                    "                rt::Step::Text {{ ty: {id}, text }} => self.t{id}.push(meter, text)?,"
                );
            }
            Key::Bool => {
                let _ = writeln!(
                    out,
                    "                rt::Step::Bool {{ ty: {id}, value }} => self.t{id}.push(meter, value)?,"
                );
            }
            Key::Option(_) => {
                let _ = writeln!(
                    out,
                    "                rt::Step::None {{ ty: {id} }} => self.t{id}.push(meter, None)?,"
                );
                let _ = writeln!(
                    out,
                    "                rt::Step::Close {{ ty: {id}, .. }} => self.close_{id}(meter)?,"
                );
            }
            Key::List(element, _) => {
                let _ = writeln!(
                    out,
                    "                rt::Step::Open {{ ty: {id}, len, .. }} => self.w{id}.push(meter, _scope.slice(len))?,"
                );
                let _ = writeln!(
                    out,
                    "                rt::Step::Element {{ list: {id} }} => {{"
                );
                let _ = writeln!(
                    out,
                    "                    let value = pop(&mut self.t{element});"
                );
                let _ = writeln!(
                    out,
                    "                    let writer = self.w{id}.last_mut().expect(\"an element belongs to an open list\");"
                );
                let _ = writeln!(out, "                    writer.write(meter, value)?;");
                let _ = writeln!(out, "                }}");
                let _ = writeln!(
                    out,
                    "                rt::Step::Close {{ ty: {id}, .. }} => self.close_{id}(meter)?,"
                );
            }
            Key::Ref(_) => {
                let _ = writeln!(
                    out,
                    "                rt::Step::Close {{ ty: {id}, .. }} => self.close_{id}(_scope, meter)?,"
                );
            }
            Key::Item(name) => {
                let case = match self.model.item(name).shape {
                    ItemShape::Variant(_) => "case",
                    _ => "..",
                };
                let arg = if case == "case" { "case, " } else { "" };
                let _ = writeln!(
                    out,
                    "                rt::Step::Close {{ ty: {id}, {case} }} => self.close_{id}({arg}meter)?,"
                );
            }
            Key::Payload(..) => unreachable!("payloads have no stacks"),
        }
    }

    fn close_fn(&self, out: &mut String, id: u32, key: Key) {
        let target = format!("self.t{id}");
        match self.list_key(key) {
            Key::Node | Key::Text | Key::Bool => {}
            Key::Option(inner) => {
                close_open(out, id, "");
                let _ = writeln!(out, "        let value = pop(&mut self.t{inner});");
                let _ = writeln!(out, "        {target}.push(meter, Some(value))");
                out.push_str("    }\n");
            }
            Key::List(..) => {
                close_open(out, id, "");
                let _ = writeln!(
                    out,
                    "        let writer = self.w{id}.pop().expect(\"a list closes its writer\");"
                );
                let _ = writeln!(out, "        {target}.push(meter, writer.finish())");
                out.push_str("    }\n");
            }
            Key::Ref(inner) => {
                close_open(out, id, "scope: &rt::Scope<'a>, ");
                let _ = writeln!(out, "        let value = pop(&mut self.t{inner});");
                let _ = writeln!(out, "        let value = scope.push(meter, value)?;");
                let _ = writeln!(out, "        {target}.push(meter, value)");
                out.push_str("    }\n");
            }
            Key::Item(name) => {
                let item = self.model.item(name);
                match &item.shape {
                    ItemShape::Record(fields) => {
                        close_open(out, id, "");
                        pop_fields(out, fields, "        ");
                        let _ = writeln!(
                            out,
                            "        {target}.push(meter, {} {{ {} }})",
                            item.ident,
                            field_list(fields)
                        );
                        out.push_str("    }\n");
                    }
                    ItemShape::Variant(cases) => {
                        close_open(out, id, "case: u32, ");
                        out.push_str("        let value = match case {\n");
                        for (index, case) in cases.iter().enumerate() {
                            let _ = writeln!(out, "            {index} => {{");
                            match &case.payload {
                                None => {
                                    let _ = writeln!(
                                        out,
                                        "                {}::{}",
                                        item.ident, case.ident
                                    );
                                }
                                Some((_, fields)) => {
                                    pop_fields(out, fields, "                ");
                                    let _ = writeln!(
                                        out,
                                        "                {}::{} {{ {} }}",
                                        item.ident,
                                        case.ident,
                                        field_list(fields)
                                    );
                                }
                            }
                            out.push_str("            }\n");
                        }
                        let _ = writeln!(
                            out,
                            "            _ => unreachable!(\"`{}` has no case {{case}}\"),",
                            item.display
                        );
                        out.push_str("        };\n");
                        let _ = writeln!(out, "        {target}.push(meter, value)");
                        out.push_str("    }\n");
                    }
                    ItemShape::Alias(_) | ItemShape::MatchOnly => {
                        unreachable!("aliases resolve to their structural key")
                    }
                }
            }
            Key::Payload(..) => unreachable!("payloads have no stacks"),
        }
    }

    fn anonymous(&self, item: &ItemModel) -> String {
        let usage = self.model.item_usage(item.name);
        let count = usize::from(usage.rows) + usize::from(usage.tree) + usize::from(usage.source);
        if count == 0 {
            return item.ident.clone();
        }
        format!("{}<{}>", item.ident, vec!["'_"; count].join(", "))
    }
}

fn note_fields(members: &mut [String], owner: u32, fields: &[FieldModel]) {
    for (index, field) in fields.iter().enumerate() {
        for member in &field.members {
            members[usize::from(member.raw())] = format!("rt::Member::new({owner}, {index})");
        }
    }
}

fn record_ty(fields: &mut Vec<String>, record: &[FieldModel]) -> String {
    let start = fields.len();
    for field in record {
        fields.push(format!(
            "rt::Field {{ name: {:?}, ty: {} }}",
            field.name, field.ty
        ));
    }
    format!(
        "rt::Ty::Record {{ fields: rt::Span::new({start}, {}) }}",
        record.len()
    )
}

fn table(out: &mut String, name: &str, entries: &[String]) {
    if entries.is_empty() {
        let _ = writeln!(out, "    {name}: &[],");
        return;
    }
    let _ = writeln!(out, "    {name}: &[");
    for entry in entries {
        let _ = writeln!(out, "        {entry},");
    }
    out.push_str("    ],\n");
}

fn close_open(out: &mut String, id: u32, params: &str) {
    out.push_str("\n    #[inline(never)]\n");
    let _ = writeln!(
        out,
        "    fn close_{id}(&mut self, {params}meter: &mut rt::Meter<'_>) -> Result<(), rt::LimitExceeded> {{"
    );
}

/// Pop fields in reverse: the walk pushed them in field order.
fn pop_fields(out: &mut String, fields: &[FieldModel], indent: &str) {
    for field in fields.iter().rev() {
        let _ = writeln!(
            out,
            "{indent}let {} = pop(&mut self.t{});",
            field.ident, field.ty
        );
    }
}

fn field_list(fields: &[FieldModel]) -> String {
    fields
        .iter()
        .map(|field| field.ident.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn case_pattern(ident: &str, case: &str, payload: bool) -> String {
    if payload {
        format!("{ident}::{case} {{ .. }}")
    } else {
        format!("{ident}::{case}")
    }
}

fn static_params(usage: LifetimeUsage) -> String {
    let count = usize::from(usage.rows) + usize::from(usage.tree) + usize::from(usage.source);
    if count == 0 {
        return String::new();
    }
    format!("<{}>", vec!["'static"; count].join(", "))
}

fn method_generics(params: &[&str]) -> String {
    let params: Vec<&str> = params.iter().copied().filter(|p| !p.is_empty()).collect();
    if params.is_empty() {
        return String::new();
    }
    format!("<{}>", params.join(", "))
}

/// The type's own lifetime parameters, for `impl<...> Ty<...>`.
fn impl_generics(usage: LifetimeUsage, _value: bool) -> String {
    usage.params("'r")
}

/// The type's parameters plus `'t` and `'s`, for trait impls over both.
fn impl_generics_all(usage: LifetimeUsage) -> String {
    let mut params = Vec::new();
    if usage.rows {
        params.push("'r");
    }
    params.push("'t");
    params.push("'s");
    format!("<{}>", params.join(", "))
}

fn capture_list(usage: LifetimeUsage) -> String {
    let mut params = Vec::new();
    if usage.rows {
        params.push("'r");
    }
    params.push("'t");
    params.push("'s");
    params.join(", ")
}

const RUN_OPEN: &str = "    /// Apply walk steps until the walk ends.
    pub(super) fn run(
        &mut self,
        walk: &mut rt::Walk<'_, '_, 't, 's>,
        _scope: &rt::Scope<'a>,
        meter: &mut rt::Meter<'_>,
    ) -> Result<(), rt::LimitExceeded> {
        while let Some(step) = walk.next(meter)? {
            match step {
";

const RUN_CLOSE: &str = "                rt::Step::Open { .. } | rt::Step::Field { .. } => {}
                other => unreachable!(\"the schema admits no step {other:?}\"),
            }
        }
        Ok(())
    }
";

const POP_FN: &str = "pub(super) fn pop<T>(stack: &mut rt::Buffer<T>) -> T {
    stack.pop().expect(\"the walk produced every value before its parent\")
}
";

const VALUE_API: &str = r#"
impl@IMPL@ @TY@ {
    /// Apply `@DEF@` once at a suitable node. A wrong node kind is no-match.
    pub fn exec@EXEC_GENERICS@(
        node: rt::Node<'t>,
        source: &'s str,
    ) -> ::core::result::Result<::core::option::Option<rt::Match<Self>>, rt::LimitExceeded> {
        rt::run::exec::<Self, matcher::@ENTRY@>(node, source)
    }

    /// Search inclusively in source-order DFS preorder, skipping descendants of matches.
    /// Each item owns its result. Matching and traversal share compiled-in limits.
    /// An error ends iteration.
    pub fn each@GENERICS@(
        scope: rt::Node<'t>,
        source: &'s str,
    ) -> impl ::core::iter::FusedIterator<
        Item = ::core::result::Result<rt::Match<Self>, rt::LimitExceeded>,
    > + use<@CAPTURE@> {
        rt::run::each::<Self, matcher::@ENTRY@>(scope, source)
    }

    /// Consume the first item of `each`.
    pub fn find@GENERICS@(
        scope: rt::Node<'t>,
        source: &'s str,
    ) -> ::core::result::Result<::core::option::Option<rt::Match<Self>>, rt::LimitExceeded> {
        Self::each(scope, source).next().transpose()
    }

    /// Collect `each`, returning an error rather than a partial collection.
    pub fn find_all@GENERICS@(
        scope: rt::Node<'t>,
        source: &'s str,
    ) -> ::core::result::Result<rt::Matches<Self>, rt::LimitExceeded> {
        rt::run::find_all::<Self, matcher::@ENTRY@>(scope, source)
    }
}

impl@ALL@ rt::Execute<'t, 's> for @TY@ {
    type Item = rt::Match<Self>;
    type All = rt::Matches<Self>;

    fn exec(
        node: rt::Node<'t>,
        source: &'s str,
    ) -> ::core::result::Result<::core::option::Option<rt::Match<Self>>, rt::LimitExceeded> {
        Self::exec(node, source)
    }

    fn each(
        scope: rt::Node<'t>,
        source: &'s str,
    ) -> impl ::core::iter::FusedIterator<
        Item = ::core::result::Result<rt::Match<Self>, rt::LimitExceeded>,
    > {
        Self::each(scope, source)
    }

    fn find_all(
        scope: rt::Node<'t>,
        source: &'s str,
    ) -> ::core::result::Result<rt::Matches<Self>, rt::LimitExceeded> {
        Self::find_all(scope, source)
    }
}

impl@ALL@ rt::Publish<'t, 's> for @TY@ {
    const SCHEMA: rt::Schema<'static> = publish::SCHEMA;
    const ROOT: u32 = @ROOT@;
    const LAYOUTS: &'static [::std::alloc::Layout] = publish::LAYOUTS;

    fn build<'a>(
        walk: &mut rt::Walk<'_, '_, 't, 's>,
        scope: &rt::Scope<'a>,
        meter: &mut rt::Meter<'_>,
    ) -> ::core::result::Result<@ROOT_TY@, rt::LimitExceeded>
    where
        Self: 'a,
    {
        let mut stacks = publish::Stacks@ROOT@::new();
        stacks.run(walk, scope, meter)?;
        Ok(publish::pop(&mut stacks.@STACK@))
    }

    fn json<'a>(value: &'a @ROOT_TY@) -> &'a dyn rt::json::Json
    where
        Self: 'a,
    {
        value
    }
}
"#;

const MATCH_ONLY_API: &str = r#"
impl @TY@ {
    /// Apply `@DEF@` once at a suitable node. A wrong node kind is no-match.
    pub fn exec<'t>(
        node: rt::Node<'t>,
        source: &str,
    ) -> ::core::result::Result<::core::option::Option<rt::Node<'t>>, rt::LimitExceeded> {
        rt::exec_node::<matcher::@ENTRY@>(node, source)
    }

    /// Search inclusively in source-order DFS preorder, skipping descendants of matches.
    /// Matching and traversal share compiled-in limits. An error ends iteration.
    pub fn each<'t, 's>(
        scope: rt::Node<'t>,
        source: &'s str,
    ) -> impl ::core::iter::FusedIterator<
        Item = ::core::result::Result<rt::Node<'t>, rt::LimitExceeded>,
    > + use<'t, 's> {
        rt::each_node::<matcher::@ENTRY@>(scope, source)
    }

    /// Consume the first item of `each`.
    pub fn find<'t>(
        scope: rt::Node<'t>,
        source: &str,
    ) -> ::core::result::Result<::core::option::Option<rt::Node<'t>>, rt::LimitExceeded> {
        Self::each(scope, source).next().transpose()
    }

    /// Collect `each`, returning an error rather than a partial vector.
    pub fn find_all<'t>(
        scope: rt::Node<'t>,
        source: &str,
    ) -> ::core::result::Result<rt::Buffer<rt::Node<'t>>, rt::LimitExceeded> {
        rt::find_all_nodes::<matcher::@ENTRY@>(scope, source)
    }
}

impl<'t, 's> rt::Execute<'t, 's> for @TY@ {
    type Item = rt::Node<'t>;
    type All = rt::Buffer<rt::Node<'t>>;

    fn exec(
        node: rt::Node<'t>,
        source: &'s str,
    ) -> ::core::result::Result<::core::option::Option<rt::Node<'t>>, rt::LimitExceeded> {
        Self::exec(node, source)
    }

    fn each(
        scope: rt::Node<'t>,
        source: &'s str,
    ) -> impl ::core::iter::FusedIterator<
        Item = ::core::result::Result<rt::Node<'t>, rt::LimitExceeded>,
    > {
        Self::each(scope, source)
    }

    fn find_all(
        scope: rt::Node<'t>,
        source: &'s str,
    ) -> ::core::result::Result<rt::Buffer<rt::Node<'t>>, rt::LimitExceeded> {
        Self::find_all(scope, source)
    }
}
"#;
