use std::fmt::Write;
use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;

use plotnik_langs::{Lang, NodeFieldId, NodeTypeId};
use plotnik_lib::Query;
use plotnik_lib::ir::{
    CompiledQuery, NodeKindResolver, QueryEmitter, STRING_NONE, TYPE_NODE, TYPE_STR, TYPE_VOID,
    TypeId, TypeKind,
};

pub struct TypesArgs {
    pub query_text: Option<String>,
    pub query_file: Option<PathBuf>,
    pub lang: Option<String>,
    pub format: String,
    pub root_type: String,
    pub verbose_nodes: bool,
    pub no_node_type: bool,
    pub export: bool,
    pub output: Option<PathBuf>,
}

struct LangResolver(Lang);

impl NodeKindResolver for LangResolver {
    fn resolve_kind(&self, name: &str) -> Option<NodeTypeId> {
        self.0.resolve_named_node(name)
    }

    fn resolve_field(&self, name: &str) -> Option<NodeFieldId> {
        self.0.resolve_field(name)
    }
}

pub fn run(args: TypesArgs) {
    if let Err(msg) = validate(&args) {
        eprintln!("error: {}", msg);
        std::process::exit(1);
    }

    let query_source = load_query(&args);
    let lang = resolve_lang_required(&args.lang);

    // Parse and validate query
    let mut query = Query::new(&query_source).exec().unwrap_or_else(|e| {
        eprintln!("error: {}", e);
        std::process::exit(1);
    });

    if !query.is_valid() {
        eprint!("{}", query.diagnostics().render(&query_source));
        std::process::exit(1);
    }

    // Link query against language
    query.link(&lang);
    if !query.is_valid() {
        eprint!("{}", query.diagnostics().render(&query_source));
        std::process::exit(1);
    }

    // Build transition graph and type info
    let query = query.build_graph();
    if query.has_type_errors() {
        eprint!("{}", query.diagnostics().render(&query_source));
        std::process::exit(1);
    }

    // Emit compiled query (IR)
    let resolver = LangResolver(lang.clone());
    let emitter = QueryEmitter::new(query.graph(), query.type_info(), resolver);
    let compiled = emitter.emit().unwrap_or_else(|e| {
        eprintln!("error: emit failed: {:?}", e);
        std::process::exit(1);
    });

    // Generate TypeScript
    let output = generate_typescript(&compiled, &args);

    // Write output
    if let Some(path) = &args.output {
        fs::write(path, &output).unwrap_or_else(|e| {
            eprintln!("error: failed to write {}: {}", path.display(), e);
            std::process::exit(1);
        });
    } else {
        print!("{}", output);
    }
}

fn generate_typescript(ir: &CompiledQuery, args: &TypesArgs) -> String {
    let mut out = String::new();
    let export_prefix = if args.export { "export " } else { "" };

    // Emit Node and Point types unless --no-node-type
    if !args.no_node_type {
        if args.verbose_nodes {
            writeln!(out, "{}interface Point {{", export_prefix).unwrap();
            writeln!(out, "  row: number;").unwrap();
            writeln!(out, "  column: number;").unwrap();
            writeln!(out, "}}").unwrap();
            writeln!(out).unwrap();
            writeln!(out, "{}interface Node {{", export_prefix).unwrap();
            writeln!(out, "  kind: string;").unwrap();
            writeln!(out, "  text: string;").unwrap();
            writeln!(out, "  start_byte: number;").unwrap();
            writeln!(out, "  end_byte: number;").unwrap();
            writeln!(out, "  start_point: Point;").unwrap();
            writeln!(out, "  end_point: Point;").unwrap();
            writeln!(out, "}}").unwrap();
        } else {
            writeln!(out, "{}interface Node {{", export_prefix).unwrap();
            writeln!(out, "  kind: string;").unwrap();
            writeln!(out, "  text: string;").unwrap();
            writeln!(out, "  range: [number, number];").unwrap();
            writeln!(out, "}}").unwrap();
        }
    }

    let emitter = TypeScriptEmitter::new(ir, export_prefix);

    // Emit composite types that are named and not inlinable
    for (idx, type_def) in ir.type_defs().iter().enumerate() {
        let type_id = idx as TypeId + 3; // TYPE_COMPOSITE_START
        if !emitter.should_emit_as_interface(type_id) {
            continue;
        }

        if !out.is_empty() {
            writeln!(out).unwrap();
        }
        emitter.emit_type_def(&mut out, type_id, type_def);
    }

    // Emit entrypoints as type aliases if they differ from their type name
    for entry in ir.entrypoints() {
        let raw_entry_name = ir.string(entry.name_id());
        // Replace anonymous entrypoint "_" with --root-type name
        let entry_name = if raw_entry_name == "_" {
            args.root_type.as_str()
        } else {
            raw_entry_name
        };
        let type_id = entry.result_type();
        let type_name = emitter.get_type_name(type_id);

        // Skip if entrypoint name matches type name (redundant alias)
        if type_name == entry_name {
            continue;
        }

        if !out.is_empty() {
            writeln!(out).unwrap();
        }
        writeln!(
            out,
            "{}type {} = {};",
            export_prefix,
            entry_name,
            emitter.format_type(type_id)
        )
        .unwrap();
    }

    out
}

struct TypeScriptEmitter<'a> {
    ir: &'a CompiledQuery,
    export_prefix: &'a str,
}

impl<'a> TypeScriptEmitter<'a> {
    fn new(ir: &'a CompiledQuery, export_prefix: &'a str) -> Self {
        Self { ir, export_prefix }
    }

    /// Returns true if this type should be emitted as a standalone interface.
    fn should_emit_as_interface(&self, type_id: TypeId) -> bool {
        if type_id < 3 {
            return false; // primitives
        }

        let idx = (type_id - 3) as usize;
        let Some(def) = self.ir.type_defs().get(idx) else {
            return false;
        };

        // Wrapper types are always inlined
        if def.is_wrapper() {
            return false;
        }

        // Named composites get their own interface
        def.name != STRING_NONE
    }

    /// Get the type name for a composite type, or generate one.
    fn get_type_name(&self, type_id: TypeId) -> String {
        match type_id {
            TYPE_VOID => "null".to_string(),
            TYPE_NODE => "Node".to_string(),
            TYPE_STR => "string".to_string(),
            _ => {
                let idx = (type_id - 3) as usize;
                if let Some(def) = self.ir.type_defs().get(idx) {
                    if def.name != STRING_NONE {
                        return self.ir.string(def.name).to_string();
                    }
                }
                // Fallback for anonymous types
                format!("T{}", type_id)
            }
        }
    }

    /// Format a type reference (may be inline or named).
    fn format_type(&self, type_id: TypeId) -> String {
        match type_id {
            TYPE_VOID => "null".to_string(),
            TYPE_NODE => "Node".to_string(),
            TYPE_STR => "string".to_string(),
            _ => {
                let idx = (type_id - 3) as usize;
                let Some(def) = self.ir.type_defs().get(idx) else {
                    return format!("unknown /* T{} */", type_id);
                };

                // Wrapper types: inline
                if let Some(inner) = def.inner_type() {
                    let inner_fmt = self.format_type(inner);
                    return match def.kind {
                        TypeKind::Optional => format!("{} | null", inner_fmt),
                        TypeKind::ArrayStar => format!("{}[]", self.wrap_if_union(&inner_fmt)),
                        TypeKind::ArrayPlus => {
                            format!("[{}, ...{}[]]", inner_fmt, self.wrap_if_union(&inner_fmt))
                        }
                        _ => unreachable!(),
                    };
                }

                // Named composite: reference by name
                if def.name != STRING_NONE {
                    return self.ir.string(def.name).to_string();
                }

                // Anonymous composite: inline
                self.format_inline_composite(type_id, def.kind)
            }
        }
    }

    /// Wrap type in parens if it contains a union (for array element types).
    fn wrap_if_union(&self, ty: &str) -> String {
        if ty.contains(" | ") {
            format!("({})", ty)
        } else {
            ty.to_string()
        }
    }

    /// Format an anonymous composite type inline.
    fn format_inline_composite(&self, type_id: TypeId, kind: TypeKind) -> String {
        let idx = (type_id - 3) as usize;
        let Some(def) = self.ir.type_defs().get(idx) else {
            return "unknown".to_string();
        };

        let Some(members_slice) = def.members_slice() else {
            return "unknown".to_string();
        };

        let members = self.ir.resolve_type_members(members_slice);

        match kind {
            TypeKind::Record => {
                let fields: Vec<String> = members
                    .iter()
                    .map(|m| format!("{}: {}", self.ir.string(m.name), self.format_type(m.ty)))
                    .collect();
                format!("{{ {} }}", fields.join("; "))
            }
            TypeKind::Enum => {
                let variants: Vec<String> = members
                    .iter()
                    .map(|m| {
                        let tag = self.ir.string(m.name);
                        let data = self.format_type(m.ty);
                        format!("{{ $tag: \"{}\"; $data: {} }}", tag, data)
                    })
                    .collect();
                variants.join(" | ")
            }
            _ => "unknown".to_string(),
        }
    }

    /// Emit a type definition as an interface or type alias.
    fn emit_type_def(&self, out: &mut String, type_id: TypeId, def: &plotnik_lib::ir::TypeDef) {
        let name = if def.name != STRING_NONE {
            self.ir.string(def.name).to_string()
        } else {
            format!("T{}", type_id)
        };

        let Some(members_slice) = def.members_slice() else {
            return;
        };

        let members = self.ir.resolve_type_members(members_slice);

        match def.kind {
            TypeKind::Record => {
                writeln!(out, "{}interface {} {{", self.export_prefix, name).unwrap();
                for m in members {
                    writeln!(
                        out,
                        "  {}: {};",
                        self.ir.string(m.name),
                        self.format_type(m.ty)
                    )
                    .unwrap();
                }
                writeln!(out, "}}").unwrap();
            }
            TypeKind::Enum => {
                let variants: Vec<String> = members
                    .iter()
                    .map(|m| {
                        let tag = self.ir.string(m.name);
                        let data = self.format_type(m.ty);
                        format!("{{ $tag: \"{}\"; $data: {} }}", tag, data)
                    })
                    .collect();
                writeln!(
                    out,
                    "{}type {} =\n  | {};",
                    self.export_prefix,
                    name,
                    variants.join("\n  | ")
                )
                .unwrap();
            }
            _ => {}
        }
    }
}

fn load_query(args: &TypesArgs) -> String {
    if let Some(ref text) = args.query_text {
        return text.clone();
    }
    if let Some(ref path) = args.query_file {
        if path.as_os_str() == "-" {
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .expect("failed to read stdin");
            return buf;
        }
        return fs::read_to_string(path).expect("failed to read query file");
    }
    unreachable!("validation ensures query input exists")
}

fn resolve_lang_required(lang: &Option<String>) -> Lang {
    let name = lang.as_ref().expect("--lang is required");
    plotnik_langs::from_name(name).unwrap_or_else(|| {
        eprintln!("error: unknown language: {}", name);
        std::process::exit(1);
    })
}

fn validate(args: &TypesArgs) -> Result<(), &'static str> {
    let has_query = args.query_text.is_some() || args.query_file.is_some();

    if !has_query {
        return Err("query is required: use -q/--query or --query-file");
    }

    if args.lang.is_none() {
        return Err("--lang is required for type generation");
    }

    let fmt = args.format.to_lowercase();
    if fmt != "typescript" && fmt != "ts" {
        return Err("--format must be 'typescript' or 'ts'");
    }

    Ok(())
}
