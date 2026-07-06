//! Show AST of query and/or source file.

use std::fmt::Write as _;
use std::path::PathBuf;

use plotnik_lib::{QueryBuilder, tree_to_json};
use serde_json::json;

use super::lang_resolver::reconcile_lang;
use super::query_loader::load_query;
use super::run_common;
use crate::error::{CliError, CliResult};

pub struct AstArgs {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub source_path: Option<PathBuf>,
    pub source_text: Option<String>,
    pub lang: Option<String>,
    pub raw: bool,
    pub json: bool,
    pub color: bool,
}

pub fn run(args: AstArgs) -> CliResult {
    let has_query = args.query_path.is_some() || args.query_text.is_some();
    let has_source = args.source_path.is_some() || args.source_text.is_some();

    if !has_query && !has_source {
        return Err(CliError::fatal("query or source required"));
    }
    if args.json {
        if !has_source {
            return Err(CliError::fatal("--json requires source input"));
        }
        let declared_lang = if has_query {
            query_declared_lang(&args)?
        } else {
            None
        };
        print_source_ast(&args, declared_lang.as_deref())?;
        return Ok(());
    }

    let show_headers = has_query && has_source;

    let mut declared_lang = None;
    if has_query {
        if show_headers {
            println!("# Query AST");
        }
        declared_lang = print_query_ast(&args)?;
    }

    if has_source {
        if show_headers {
            println!("\n# Source AST");
        }
        print_source_ast(&args, declared_lang.as_deref())?;
    }

    Ok(())
}

/// Reads query metadata needed by source AST rendering; no query AST is emitted.
fn query_declared_lang(args: &AstArgs) -> Result<Option<String>, CliError> {
    let loaded = load_query(args.query_path.as_deref(), args.query_text.as_deref())?;

    if loaded.sources.is_empty() {
        return Err(CliError::fatal("query cannot be empty"));
    }

    reconcile_lang(args.lang.as_deref(), loaded.shebang.lang.as_deref())?;
    Ok(loaded.shebang.lang)
}

/// Prints the query AST; returns the shebang-declared language, if any.
fn print_query_ast(args: &AstArgs) -> Result<Option<String>, CliError> {
    let loaded = load_query(args.query_path.as_deref(), args.query_text.as_deref())?;

    if loaded.sources.is_empty() {
        return Err(CliError::fatal("query cannot be empty"));
    }

    // Enforce -l/shebang agreement even when no source AST follows
    reconcile_lang(args.lang.as_deref(), loaded.shebang.lang.as_deref())?;

    let query = QueryBuilder::new(loaded.sources)
        .analyze()
        .map_err(|e| CliError::fatal(e.to_string()))?;

    if query.diagnostics().has_errors() || query.diagnostics().has_warnings() {
        eprint!(
            "{}",
            query
                .diagnostics()
                .render_colored(query.source_map(), args.color)
        );
    }

    let output = if args.raw {
        query.dump_cst_with_trivia(true)
    } else {
        query.dump_ast()
    };
    print!("{}", output);

    Ok(loaded.shebang.lang)
}

fn print_source_ast(args: &AstArgs, declared_lang: Option<&str>) -> CliResult {
    let source = run_common::load_source(
        args.source_text.as_deref(),
        args.source_path.as_deref(),
        args.query_path.as_deref(),
    )?;
    let lang = run_common::resolve_run_lang(
        args.lang.as_deref(),
        declared_lang,
        args.source_path.as_deref(),
    )?;
    let tree = lang.parse_source(&source);
    if args.json {
        let output = json!({
            "source": tree_to_json(&tree, &source, args.raw),
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&output).expect("source tree JSON serializes")
        );
        return Ok(());
    }

    print!("{}", dump_tree(&tree, &source, args.raw));

    Ok(())
}

/// One emission for the iterative tree dumper's work stack.
enum Step<'a> {
    /// Render a node: its indent, optional field, and `(kind …)` body.
    Node {
        node: tree_sitter::Node<'a>,
        field: Option<&'a str>,
        depth: usize,
    },
    /// Write a literal verbatim (newlines between children, the closing paren).
    Lit(&'static str),
}

/// Dump a parsed tree as indented S-expressions, iteratively.
///
/// The source tree is untrusted and can nest past any native-stack budget (a long
/// unary/parenthesis chain, say), so the walk uses an explicit work stack rather
/// than native recursion.
fn dump_tree(tree: &tree_sitter::Tree, source: &str, raw: bool) -> String {
    let mut out = String::new();
    let mut stack = vec![Step::Node {
        node: tree.root_node(),
        field: None,
        depth: 0,
    }];
    while let Some(step) = stack.pop() {
        let (node, field, depth) = match step {
            Step::Lit(s) => {
                out.push_str(s);
                continue;
            }
            Step::Node { node, field, depth } => (node, field, depth),
        };

        // Anonymous nodes are dropped unless `--raw`. Children are pre-filtered
        // below, so this only guards a (hypothetical) anonymous root.
        if !raw && !node.is_named() {
            continue;
        }

        for _ in 0..depth {
            out.push_str("  ");
        }
        if let Some(f) = field {
            let _ = write!(out, "{}: ", f);
        }
        let kind = node.kind();

        let children = collect_children(node, raw);
        if children.is_empty() {
            let text = node
                .utf8_text(source.as_bytes())
                .unwrap_or("<invalid utf8>");
            if text == kind {
                out.push_str("(\"");
                escape_string_into(&mut out, kind);
                out.push_str("\")");
            } else {
                let _ = write!(out, "({} \"", kind);
                escape_string_into(&mut out, text);
                out.push_str("\")");
            }
            continue;
        }

        let _ = write!(out, "({}", kind);
        // Queue, in source order, a newline + each child, then the closing paren.
        let mut deferred = Vec::with_capacity(children.len() * 2 + 1);
        for (child, child_field) in children {
            deferred.push(Step::Lit("\n"));
            deferred.push(Step::Node {
                node: child,
                field: child_field,
                depth: depth + 1,
            });
        }
        deferred.push(Step::Lit(")"));
        stack.extend(deferred.into_iter().rev());
    }
    out.push('\n');
    out
}

/// Collect a node's children, keeping anonymous ones only in `raw` mode, paired
/// with their field names.
fn collect_children<'a>(
    node: tree_sitter::Node<'a>,
    raw: bool,
) -> Vec<(tree_sitter::Node<'a>, Option<&'a str>)> {
    let mut cursor = node.walk();
    let mut result = Vec::new();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if raw || child.is_named() {
                result.push((child, cursor.field_name()));
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    result
}

fn escape_string_into(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c if c.is_control() => {
                let _ = write!(out, "\\u{{{:04x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
}
