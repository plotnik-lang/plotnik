use std::collections::HashMap;
use std::path::{Path, PathBuf};

use proc_macro::{Delimiter, Group, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree};

use plotnik_compiler::{CompiledQuery, QueryBuilder, RustCodegenConfig, SourceMap, SourcePath};
use plotnik_runtime::Limit;

use crate::args::{self, ExpandError, LimitArg, QuerySource};
use crate::grammar_source;

struct QueryInput {
    source_map: SourceMap,
    span: Span,
}

struct RebuildAnchors {
    lines: Vec<String>,
}

impl RebuildAnchors {
    fn new() -> Self {
        Self { lines: Vec::new() }
    }

    fn query_file(&mut self, path: &str) {
        self.lines
            .push(format!("const _: &str = ::core::include_str!({path:?});"));
    }

    fn grammar(
        &mut self,
        spec: &grammar_source::GrammarSpec<'_>,
        as_written: &str,
        resolved: &Path,
    ) {
        self.lines.push(format!(
            "const _: &[u8] = ::core::include_bytes!({:?});",
            anchor_path(spec, as_written, resolved)
        ));
    }

    fn generated_source(&self) -> String {
        self.lines.join("\n")
    }

    fn lines(&self) -> impl Iterator<Item = &str> {
        self.lines.iter().map(String::as_str)
    }
}

pub fn expand(input: TokenStream) -> TokenStream {
    let fingerprint = crc32fast::hash(input.to_string().as_bytes());

    // Rust tracks these generated includes as dependencies, including on failure,
    // so editing a query file with compilation errors retriggers expansion.
    let mut anchors = RebuildAnchors::new();

    match try_expand(input, &mut anchors) {
        Ok(code) => assemble(fingerprint, &anchors, &code),
        Err(error) => {
            let mut out = compile_error(&error);
            out.extend(parse_generated(&anchors.generated_source()));
            out
        }
    }
}

fn try_expand(input: TokenStream, anchors: &mut RebuildAnchors) -> Result<String, ExpandError> {
    let args = args::parse(input)?;
    let base_dir = invoking_dir();
    let query = load_query_input(&args.query, base_dir.as_deref(), anchors)?;

    let spec = grammar_source::parse_spec(&args.grammar.value);
    let loaded_grammar = grammar_source::load(&spec, base_dir.as_deref())
        .map_err(|message| ExpandError::new(args.grammar.span, message))?;
    anchors.grammar(&spec, &args.grammar.value, &loaded_grammar.path);

    let query_span = query.span;
    let compiled = compile_query(query, &loaded_grammar.grammar)?;
    reject_colliding_entry_names(&compiled, query_span)?;

    let config = RustCodegenConfig::new()
        .runtime_crate("::plotnik::rt")
        .serde(cfg!(feature = "serde"))
        .decode_depth(decode_depth(&args.depth));
    let emission = compiled
        .emit(config)
        .map_err(|error| ExpandError::new(query_span, error.to_string()))?;
    if !emission.diagnostics().is_empty() {
        let rendered = emission
            .diagnostics()
            .render_colored(compiled.source_map(), false);
        let message = rendered.strip_prefix("error: ").unwrap_or(&rendered);
        return Err(ExpandError::new(query_span, message));
    }
    emission
        .into_artifact()
        .map(|output| output.into_source())
        .ok_or_else(|| ExpandError::new(query_span, "query did not emit a Rust module"))
}

fn compile_query(
    query: QueryInput,
    grammar: &plotnik_compiler::grammar::Grammar,
) -> Result<CompiledQuery, ExpandError> {
    let span = query.span;
    let compiled = QueryBuilder::new(query.source_map)
        .compile(grammar)
        .map_err(|error| ExpandError::new(span, error.to_string()))?;
    let diagnostics = compiled.diagnostics();
    if diagnostics.has_errors() || diagnostics.has_warnings() {
        let rendered = diagnostics.render_colored(compiled.source_map(), false);
        let message = rendered.strip_prefix("error: ").unwrap_or(&rendered);
        return Err(ExpandError::new(span, message));
    }

    Ok(compiled)
}

fn reject_colliding_entry_names(compiled: &CompiledQuery, span: Span) -> Result<(), ExpandError> {
    let mut entry_names: HashMap<String, String> = HashMap::new();
    for def in compiled.entrypoint_names() {
        let entry = plotnik_compiler::matcher_journal_fn_name(&def);
        if let Some(previous) = entry_names.insert(entry.clone(), def.clone()) {
            return Err(ExpandError::new(
                span,
                format!(
                    "definitions `{previous}` and `{def}` collide in generated code: \
                     both would be spelled `{entry}`; rename one so their snake_case \
                     forms differ"
                ),
            ));
        }
    }
    Ok(())
}

fn load_query_input(
    source: &QuerySource,
    base_dir: Option<&Path>,
    anchors: &mut RebuildAnchors,
) -> Result<QueryInput, ExpandError> {
    match source {
        QuerySource::Inline { literal } => Ok(QueryInput {
            source_map: SourceMap::from_inline(&literal.value),
            span: literal.span,
        }),
        QuerySource::File { path } => {
            let resolved = grammar_source::resolve_relative(&path.value, base_dir)
                .map_err(|message| ExpandError::new(path.span, message))?;
            let content = std::fs::read_to_string(&resolved).map_err(|error| {
                ExpandError::new(
                    path.span,
                    format!("failed to read `{}`: {error}", path.value),
                )
            })?;
            anchors.query_file(&path.value);

            let mut source_map = SourceMap::new();
            source_map.add_file(SourcePath::new(&path.value), &content);
            Ok(QueryInput {
                source_map,
                span: path.span,
            })
        }
    }
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

fn invoking_dir() -> Option<PathBuf> {
    let file = Span::call_site().local_file()?;
    Some(file.parent()?.to_path_buf())
}

fn decode_depth(arg: &Option<LimitArg>) -> Limit {
    match arg {
        None | Some(LimitArg::Auto) => Limit::Auto,
        Some(LimitArg::Of(value)) => Limit::Of(*value),
    }
}

fn assemble(fingerprint: u32, anchors: &RebuildAnchors, code: &str) -> TokenStream {
    let mod_name = format!("__plotnik_{fingerprint:08x}");
    let mut text = String::new();
    text.push_str(&format!("mod {mod_name} {{\n"));
    for anchor in anchors.lines() {
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
