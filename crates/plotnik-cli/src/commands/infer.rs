use std::fs;
use std::io::{self, Read};

use plotnik_lib::Query;
use plotnik_lib::infer::{Indirection, OptionalStyle};

use crate::cli::{IndirectionChoice, OptionalChoice, OutputLang};

pub struct InferArgs {
    pub query_text: Option<String>,
    pub query_file: Option<std::path::PathBuf>,
    pub lang: OutputLang,
    pub entry_name: Option<String>,
    pub color: bool,
    // Rust options
    pub indirection: Option<IndirectionChoice>,
    pub derive: Option<Vec<String>>,
    pub no_derive: bool,
    // TypeScript options
    pub optional: Option<OptionalChoice>,
    pub export: bool,
    pub readonly: bool,
    pub type_alias: bool,
    pub node_type: Option<String>,
    pub nested: bool,
}

pub fn run(args: InferArgs) {
    if let Err(msg) = validate(&args) {
        eprintln!("error: {}", msg);
        std::process::exit(1);
    }

    let query_source = load_query(&args);

    let query = Query::try_from(query_source.as_str()).unwrap_or_else(|e| {
        eprintln!("error: {}", e);
        std::process::exit(1);
    });

    if !query.is_valid() {
        eprint!(
            "{}",
            query
                .diagnostics()
                .render_colored(&query_source, args.color)
        );
        std::process::exit(1);
    }

    let output = emit_types(&query, &args);
    println!("{}", output);

    if query.diagnostics().has_warnings() {
        eprint!(
            "{}",
            query
                .diagnostics()
                .render_colored(&query_source, args.color)
        );
    }
}

fn validate(args: &InferArgs) -> Result<(), &'static str> {
    if args.query_text.is_none() && args.query_file.is_none() {
        return Err("query input required: -q/--query or --query-file");
    }

    Ok(())
}

fn load_query(args: &InferArgs) -> String {
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
    unreachable!()
}

fn emit_types(query: &Query<'_>, args: &InferArgs) -> String {
    let mut printer = query.type_printer();

    if let Some(ref name) = args.entry_name {
        printer = printer.entry_name(name);
    }

    match args.lang {
        OutputLang::Rust => emit_rust(printer, args),
        OutputLang::Typescript | OutputLang::Ts => emit_typescript(printer, args),
    }
}

fn emit_rust(printer: plotnik_lib::infer::TypePrinter<'_>, args: &InferArgs) -> String {
    let mut rust = printer.rust();

    if let Some(ind) = args.indirection {
        let indirection = match ind {
            IndirectionChoice::Box => Indirection::Box,
            IndirectionChoice::Rc => Indirection::Rc,
            IndirectionChoice::Arc => Indirection::Arc,
        };
        rust = rust.indirection(indirection);
    }

    if args.no_derive {
        rust = rust.derive(&[]);
    } else if let Some(ref traits) = args.derive {
        let trait_refs: Vec<&str> = traits.iter().map(|s| s.as_str()).collect();
        rust = rust.derive(&trait_refs);
    }

    rust.render()
}

fn emit_typescript(printer: plotnik_lib::infer::TypePrinter<'_>, args: &InferArgs) -> String {
    let mut ts = printer.typescript();

    if let Some(opt) = args.optional {
        let style = match opt {
            OptionalChoice::Null => OptionalStyle::Null,
            OptionalChoice::Undefined => OptionalStyle::Undefined,
            OptionalChoice::QuestionMark => OptionalStyle::QuestionMark,
        };
        ts = ts.optional(style);
    }

    ts = ts.export(args.export);
    ts = ts.readonly(args.readonly);
    ts = ts.type_alias(args.type_alias);
    ts = ts.nested(args.nested);

    if let Some(ref name) = args.node_type {
        ts = ts.node_type(name);
    }

    ts.render()
}
