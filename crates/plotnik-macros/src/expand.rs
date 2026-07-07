//! `query!` expansion: resolve the grammar, run the compiler pipeline, and
//! splice the generated module into the caller.
//!
//! The expansion is wrapped in a fingerprint-named module and glob
//! re-exported, so several `query!` invocations can share one enclosing
//! module without their internals (the `rt` alias, `mod matcher`) colliding:
//!
//! ```text
//! mod __plotnik_1a2b3c4d {
//!     const _: &str = ::core::include_str!("queries/q.ptk"); // rebuild anchors
//!     const _: &[u8] = ::core::include_bytes!("/…/grammar.json");
//!     /* the generated query module */
//! }
//! #[allow(unused_imports)]
//! pub use self::__plotnik_1a2b3c4d::*;
//! ```

use std::path::{Path, PathBuf};

use proc_macro::{Delimiter, Group, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree};

use plotnik_lib::grammar::Grammar;
use plotnik_lib::grammar::raw::RawGrammar;
use plotnik_lib::{MatcherConfig, QueryBuilder, SourceMap, SourcePath};
use plotnik_rt::{Limit, RuntimeLimitSpec};

use crate::args::{self, ExpandError, LimitArg, QuerySource};
use crate::grammar_source;

pub fn expand(input: TokenStream) -> TokenStream {
    // The wrapper-module name comes from the raw input: same invocation, same
    // name (deterministic output); different queries in one module never
    // collide. Two identical invocations in one module collide here — and on
    // every generated type — which is the right error.
    let fingerprint = crc32fast::hash(input.to_string().as_bytes());

    // Rebuild anchors for everything read from disk, emitted on success *and*
    // failure: a broken query file must retrigger expansion when edited.
    let mut anchors: Vec<String> = Vec::new();

    match try_expand(input, &mut anchors) {
        Ok(code) => assemble(fingerprint, &anchors, &code),
        Err(error) => {
            let mut out = compile_error(&error);
            out.extend(parse_generated(&anchors.join("\n")));
            out
        }
    }
}

fn try_expand(input: TokenStream, anchors: &mut Vec<String>) -> Result<String, ExpandError> {
    let args = args::parse(input)?;
    let base_dir = invoking_dir();

    // The query text, with diagnostics attributed to the file when there is one.
    let (source_map, query_span) = match &args.query {
        QuerySource::Inline { text, span } => (SourceMap::from_inline(text), *span),
        QuerySource::File { path, span } => {
            let resolved = grammar_source::resolve_relative(path, base_dir.as_deref())
                .map_err(|message| ExpandError::new(*span, message))?;
            let content = std::fs::read_to_string(&resolved).map_err(|error| {
                ExpandError::new(*span, format!("failed to read `{path}`: {error}"))
            })?;
            anchors.push(format!("const _: &str = ::core::include_str!({path:?});"));
            let mut map = SourceMap::new();
            map.add_file(SourcePath::new(path), &content);
            (map, *span)
        }
    };

    let spec = grammar_source::parse_spec(&args.grammar);
    let resolved = grammar_source::resolve(&spec, base_dir.as_deref())
        .map_err(|message| ExpandError::new(args.grammar_span, message))?;
    anchors.push(format!(
        "const _: &[u8] = ::core::include_bytes!({:?});",
        anchor_path(&spec, &args.grammar, &resolved.path)
    ));

    let raw = RawGrammar::from_json(&resolved.json).map_err(|error| {
        ExpandError::new(
            args.grammar_span,
            format!("invalid grammar `{}`: {error}", resolved.path.display()),
        )
    })?;
    let grammar = Grammar::from_raw(&raw).map_err(|error| {
        ExpandError::new(
            args.grammar_span,
            format!("invalid grammar `{}`: {error}", resolved.path.display()),
        )
    })?;

    // Mirror `plotnik check --strict`: a compile-time-committed query must be
    // clean — proc macros have no warning channel, so warnings fail too.
    let compiled = QueryBuilder::new(source_map)
        .with_strict_lints(true)
        .compile(&grammar)
        .map_err(|error| ExpandError::new(query_span, error.to_string()))?;
    let diagnostics = compiled.diagnostics();
    if diagnostics.has_errors() || diagnostics.has_warnings() {
        return Err(ExpandError::new(
            query_span,
            diagnostics.render_colored(compiled.source_map(), false),
        ));
    }

    let config = MatcherConfig::new()
        .rt_crate(args.rt_crate.unwrap_or_else(|| "::plotnik::rt".to_string()))
        .serde(cfg!(feature = "serde"))
        .limits(RuntimeLimitSpec {
            steps: limit(args.steps),
            memory: limit(args.memory),
        });
    Ok(compiled
        .to_rust_matcher(config)
        .expect("a diagnostics-clean query generates a module"))
}

/// The `include_bytes!` path for the grammar anchor. A path argument is
/// re-emitted as written (rustc resolves it against the invoking file, same
/// as our read); a package grammar uses the absolute checkout path.
fn anchor_path(
    spec: &grammar_source::GrammarSpec<'_>,
    as_written: &str,
    resolved: &Path,
) -> String {
    match spec {
        grammar_source::GrammarSpec::Path(_) => as_written.to_string(),
        grammar_source::GrammarSpec::Package { .. } => resolved.display().to_string(),
    }
}

/// The directory of the file containing the invocation — the base every
/// relative path resolves against, exactly like `include_str!`.
fn invoking_dir() -> Option<PathBuf> {
    let file = Span::call_site().local_file()?;
    Some(file.parent()?.to_path_buf())
}

fn limit(arg: Option<LimitArg>) -> Limit {
    match arg {
        None | Some(LimitArg::Auto) => Limit::Auto,
        Some(LimitArg::Unbounded) => Limit::Unbounded,
        Some(LimitArg::Of(value)) => Limit::Of(value),
    }
}

fn assemble(fingerprint: u32, anchors: &[String], code: &str) -> TokenStream {
    let mod_name = format!("__plotnik_{fingerprint:08x}");
    let mut text = String::new();
    text.push_str(&format!("mod {mod_name} {{\n"));
    for anchor in anchors {
        text.push_str(anchor);
        text.push('\n');
    }
    text.push_str(code);
    text.push_str("}\n");
    text.push_str(&format!(
        "#[allow(unused_imports)]\npub use self::{mod_name}::*;\n"
    ));
    parse_generated(&text)
}

fn parse_generated(text: &str) -> TokenStream {
    text.parse()
        .expect("generated module lexes as a token stream")
}

/// `compile_error! { "…" }` with every token spanned onto the offending
/// argument, so the caller sees the right thing underlined.
fn compile_error(error: &ExpandError) -> TokenStream {
    let mut literal = Literal::string(&error.message);
    literal.set_span(error.span);
    let mut group = Group::new(
        Delimiter::Brace,
        TokenStream::from_iter([TokenTree::Literal(literal)]),
    );
    group.set_span(error.span);
    let mut bang = Punct::new('!', Spacing::Alone);
    bang.set_span(error.span);
    TokenStream::from_iter([
        TokenTree::Ident(Ident::new("compile_error", error.span)),
        TokenTree::Punct(bang),
        TokenTree::Group(group),
    ])
}
