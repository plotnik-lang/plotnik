//! Token-level argument parsing for `query!`.
//!
//! No syntax crate: the argument grammar is a flat `key = value` list plus
//! one bare string literal (the query), which a hand-rolled walk over
//! [`TokenTree`]s covers. Everything that can go wrong reports through
//! [`ExpandError`] with a span, so the caller sees the offending argument
//! underlined.

use proc_macro::{Span, TokenStream, TokenTree};

/// A macro-expansion failure: rendered as `compile_error!` at `span`.
pub struct ExpandError {
    pub span: Span,
    pub message: String,
}

impl ExpandError {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }
}

pub enum QuerySource {
    Inline { text: String, span: Span },
    File { path: String, span: Span },
}

/// A limit argument as written; mirrors `plotnik_rt::Limit` without naming it
/// (this module stays token-only).
pub enum LimitArg {
    Auto,
    Unbounded,
    Of(u64),
}

pub struct MacroArgs {
    pub grammar: String,
    pub grammar_span: Span,
    pub query: QuerySource,
    /// The `crate = ::path` override, stringified; `None` means the default
    /// facade path.
    pub rt_crate: Option<String>,
    pub limits: LimitArgs,
}

#[derive(Default)]
pub struct LimitArgs {
    pub steps: Option<LimitArg>,
    pub memory: Option<LimitArg>,
    pub depth: Option<LimitArg>,
}

#[derive(Default)]
struct ArgSlots {
    grammar: Option<(String, Span)>,
    query: Option<QuerySource>,
    rt_crate: Option<String>,
    limits: LimitArgs,
}

impl ArgSlots {
    fn put_query(&mut self, source: QuerySource, span: Span) -> Result<(), ExpandError> {
        if self.query.is_some() {
            return Err(ExpandError::new(
                span,
                "the query is already given; `query!` takes one query string \
                 (or one `file = \"...\"`)",
            ));
        }
        self.query = Some(source);
        Ok(())
    }

    fn finish(self, call_span: Span) -> Result<MacroArgs, ExpandError> {
        let Some((grammar, grammar_span)) = self.grammar else {
            return Err(ExpandError::new(
                call_span,
                "missing `grammar = \"...\"`: name a dependency that ships a grammar \
                 (e.g. `grammar = \"tree-sitter-javascript\"`) or a grammar.json path",
            ));
        };
        let Some(query) = self.query else {
            return Err(ExpandError::new(
                call_span,
                "missing the query: pass a string literal or `file = \"...\"`",
            ));
        };

        Ok(MacroArgs {
            grammar,
            grammar_span,
            query,
            rt_crate: self.rt_crate,
            limits: self.limits,
        })
    }
}

pub fn parse(input: TokenStream) -> Result<MacroArgs, ExpandError> {
    let tokens: Vec<TokenTree> = input.into_iter().collect();
    let call_span = Span::call_site();
    let mut args = ArgSlots::default();

    let mut pos = 0;
    while pos < tokens.len() {
        match &tokens[pos] {
            // The one positional argument: the query string.
            TokenTree::Literal(lit) => {
                let (text, span) = string_value(&tokens[pos])?;
                args.put_query(QuerySource::Inline { text, span }, lit.span())?;
                pos += 1;
            }
            TokenTree::Ident(ident) => {
                let key = ident.to_string();
                let key_span = ident.span();
                expect_eq(&tokens, pos + 1, key_span, &key)?;
                pos += 2;
                match key.as_str() {
                    "grammar" => {
                        let value = take_string(&tokens, &mut pos, key_span, &key)?;
                        put(&mut args.grammar, value, key_span, &key)?;
                    }
                    "file" => {
                        let (path, span) = take_string(&tokens, &mut pos, key_span, &key)?;
                        args.put_query(QuerySource::File { path, span }, span)?;
                    }
                    "crate" => {
                        let value = take_path(&tokens, &mut pos, key_span)?;
                        put(&mut args.rt_crate, value, key_span, &key)?;
                    }
                    "steps" => {
                        let value = take_limit(&tokens, &mut pos, key_span, &key)?;
                        put(&mut args.limits.steps, value, key_span, &key)?;
                    }
                    "memory" => {
                        let value = take_limit(&tokens, &mut pos, key_span, &key)?;
                        put(&mut args.limits.memory, value, key_span, &key)?;
                    }
                    "depth" => {
                        let value = take_limit(&tokens, &mut pos, key_span, &key)?;
                        put(&mut args.limits.depth, value, key_span, &key)?;
                    }
                    other => {
                        return Err(ExpandError::new(
                            key_span,
                            format!(
                                "unknown argument `{other}`; `query!` accepts `grammar`, \
                                 `file`, `crate`, `steps`, `memory`, `depth`, and the \
                                 query string"
                            ),
                        ));
                    }
                }
            }
            other => {
                return Err(ExpandError::new(
                    other.span(),
                    "expected `key = value` or the query string here",
                ));
            }
        }

        // Between arguments: a comma, or the end. A trailing comma is fine.
        if pos < tokens.len() {
            match &tokens[pos] {
                TokenTree::Punct(p) if p.as_char() == ',' => pos += 1,
                other => {
                    return Err(ExpandError::new(other.span(), "expected `,` here"));
                }
            }
        }
    }
    args.finish(call_span)
}

fn put<T>(slot: &mut Option<T>, value: T, span: Span, key: &str) -> Result<(), ExpandError> {
    if slot.is_some() {
        return Err(ExpandError::new(
            span,
            format!("duplicate argument `{key}`"),
        ));
    }
    *slot = Some(value);
    Ok(())
}

fn expect_eq(
    tokens: &[TokenTree],
    pos: usize,
    key_span: Span,
    key: &str,
) -> Result<(), ExpandError> {
    match tokens.get(pos) {
        Some(TokenTree::Punct(p)) if p.as_char() == '=' => Ok(()),
        _ => Err(ExpandError::new(
            key_span,
            format!("expected `{key} = ...`"),
        )),
    }
}

fn take_string(
    tokens: &[TokenTree],
    pos: &mut usize,
    key_span: Span,
    key: &str,
) -> Result<(String, Span), ExpandError> {
    let Some(token) = tokens.get(*pos) else {
        return Err(ExpandError::new(
            key_span,
            format!("`{key}` needs a string value"),
        ));
    };
    *pos += 1;
    string_value(token)
}

fn string_value(token: &TokenTree) -> Result<(String, Span), ExpandError> {
    let text = token.to_string();
    match litrs::StringLit::parse(text) {
        Ok(lit) => Ok((lit.value().to_string(), token.span())),
        Err(_) => Err(ExpandError::new(
            token.span(),
            "expected a string literal here",
        )),
    }
}

/// `crate = ::some::path` — collect tokens up to the next top-level comma and
/// splice them verbatim into generated code. Absolute paths only: the text is
/// spliced into several nested modules, where a relative path would resolve
/// differently in each.
fn take_path(tokens: &[TokenTree], pos: &mut usize, key_span: Span) -> Result<String, ExpandError> {
    let mut path = String::new();
    while let Some(token) = tokens.get(*pos) {
        if let TokenTree::Punct(p) = token
            && p.as_char() == ','
        {
            break;
        }
        path.push_str(&token.to_string());
        *pos += 1;
    }
    if path.is_empty() {
        return Err(ExpandError::new(key_span, "`crate` needs a path value"));
    }
    if !is_absolute_path(&path) {
        return Err(ExpandError::new(
            key_span,
            format!(
                "`crate = {path}` must be an absolute module path like \
                 `::my_facade::rt` or `::plotnik_rt`"
            ),
        ));
    }
    Ok(path)
}

/// The collected `crate` text is spliced verbatim into generated code, so it
/// must be exactly a `(::segment)+` path — nothing else lexes its way in.
fn is_absolute_path(text: &str) -> bool {
    let mut rest = text;
    let mut any = false;
    while let Some(after) = rest.strip_prefix("::") {
        rest = strip_ident(after);
        if rest.len() == after.len() {
            return false;
        }
        any = true;
    }
    any && rest.is_empty()
}

/// Strip one leading identifier (optionally `r#`-raw); on no identifier,
/// return the input unchanged so the caller sees zero progress.
fn strip_ident(text: &str) -> &str {
    let body = text.strip_prefix("r#").unwrap_or(text);
    let mut chars = body.char_indices();
    match chars.next() {
        Some((_, c)) if c == '_' || c.is_alphabetic() => {}
        _ => return text,
    }
    let end = chars
        .find(|&(_, c)| !(c == '_' || c.is_alphanumeric()))
        .map_or(body.len(), |(index, _)| index);
    &body[end..]
}

fn take_limit(
    tokens: &[TokenTree],
    pos: &mut usize,
    key_span: Span,
    key: &str,
) -> Result<LimitArg, ExpandError> {
    let Some(token) = tokens.get(*pos) else {
        return Err(ExpandError::new(
            key_span,
            format!("`{key}` needs a value: an integer, `auto`, or `unbounded`"),
        ));
    };
    *pos += 1;
    match token {
        TokenTree::Ident(ident) if ident.to_string() == "auto" => Ok(LimitArg::Auto),
        TokenTree::Ident(ident) if ident.to_string() == "unbounded" => Ok(LimitArg::Unbounded),
        TokenTree::Literal(_) => {
            let text = token.to_string();
            let value = litrs::IntegerLit::parse(text)
                .ok()
                .and_then(|lit| lit.value::<u64>());
            match value {
                Some(value) => Ok(LimitArg::Of(value)),
                None => Err(ExpandError::new(
                    token.span(),
                    format!("`{key}` needs an unsigned integer, `auto`, or `unbounded`"),
                )),
            }
        }
        other => Err(ExpandError::new(
            other.span(),
            format!("`{key}` needs an unsigned integer, `auto`, or `unbounded`"),
        )),
    }
}
