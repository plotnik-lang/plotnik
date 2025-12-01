use plotnik_langs::Lang;
use std::fs;
use std::io::{self, Read};

use crate::cli::{QueryArgs, SourceArgs};

pub fn load_query(args: &QueryArgs) -> String {
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

pub fn load_source(args: &SourceArgs) -> String {
    if let Some(text) = &args.source_text {
        return text.clone();
    }
    if let Some(path) = &args.source_file {
        if path.as_os_str() == "-" {
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .expect("failed to read stdin");
            return buf;
        }
        return fs::read_to_string(path).expect("failed to read source file");
    }
    unreachable!()
}

pub fn resolve_lang(lang: &Option<String>, source_args: &SourceArgs) -> Lang {
    if let Some(name) = lang {
        return Lang::from_name(name).unwrap_or_else(|| {
            eprintln!("error: unknown language: {}", name);
            std::process::exit(1);
        });
    }

    if let Some(path) = &source_args.source_file {
        if path.as_os_str() != "-" {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                return Lang::from_extension(ext).unwrap_or_else(|| {
                    eprintln!(
                        "error: cannot infer language from extension '.{}', use --lang",
                        ext
                    );
                    std::process::exit(1);
                });
            }
        }
    }

    eprintln!("error: --lang is required (cannot infer from input)");
    std::process::exit(1);
}

pub fn parse_source_ast(source: &str, lang: Lang) -> String {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&lang.language())
        .expect("failed to set language");
    let tree = parser.parse(source, None).expect("failed to parse source");
    tree.root_node().to_sexp()
}
