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

/// A parsed string literal argument and the token span diagnostics should
/// underline when that argument is responsible for an error.
pub struct StringArg {
    pub value: String,
    pub span: Span,
}

pub enum QuerySource {
    Inline { literal: StringArg },
    File { path: StringArg },
}

impl QuerySource {
    fn span(&self) -> Span {
        match self {
            Self::Inline { literal } => literal.span,
            Self::File { path } => path.span,
        }
    }
}

/// A limit argument as written; mirrors `plotnik_rt::Limit` without naming it
/// (this module stays token-only).
pub enum LimitArg {
    Auto,
    Unbounded,
    Of(u64),
}

pub struct MacroArgs {
    pub grammar: StringArg,
    pub query: QuerySource,
    /// The `crate = ::path` override, stringified; `None` means the default
    /// facade path.
    pub rt_crate: Option<String>,
    pub limits: LimitArgs,
}

#[derive(Default)]
pub struct LimitArgs {
    pub fuel: Option<LimitArg>,
    pub memory: Option<LimitArg>,
    pub depth: Option<LimitArg>,
}

impl LimitArgs {
    fn put(&mut self, key: LimitKey, value: LimitArg, span: Span) -> Result<(), ExpandError> {
        put(key.slot(self), value, span, key.name())
    }
}

#[derive(Clone, Copy)]
enum LimitKey {
    Fuel,
    Memory,
    Depth,
}

impl LimitKey {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "fuel" => Some(Self::Fuel),
            "memory" => Some(Self::Memory),
            "depth" => Some(Self::Depth),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Fuel => "fuel",
            Self::Memory => "memory",
            Self::Depth => "depth",
        }
    }

    /// Whether `unbounded` is a legal value. A recursive native value has no
    /// safe unbounded decode depth (`drop` alone overflows), so `depth` refuses
    /// it — reflected in the value-error wording.
    fn allows_unbounded(self) -> bool {
        !matches!(self, Self::Depth)
    }

    fn slot(self, args: &mut LimitArgs) -> &mut Option<LimitArg> {
        match self {
            Self::Fuel => &mut args.fuel,
            Self::Memory => &mut args.memory,
            Self::Depth => &mut args.depth,
        }
    }
}

#[derive(Default)]
struct ArgSlots {
    grammar: Option<StringArg>,
    query: Option<QuerySource>,
    rt_crate: Option<String>,
    limits: LimitArgs,
}

impl ArgSlots {
    fn put_query(&mut self, source: QuerySource) -> Result<(), ExpandError> {
        if self.query.is_some() {
            return Err(ExpandError::new(
                source.span(),
                "the query is already given; `query!` takes one query string \
                 (or one `file = \"...\"`)",
            ));
        }
        self.query = Some(source);
        Ok(())
    }

    fn finish(self, call_span: Span) -> Result<MacroArgs, ExpandError> {
        let Some(grammar) = self.grammar else {
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
            query,
            rt_crate: self.rt_crate,
            limits: self.limits,
        })
    }
}

struct ArgCursor {
    tokens: Vec<TokenTree>,
    pos: usize,
}

impl ArgCursor {
    fn new(input: TokenStream) -> Self {
        Self {
            tokens: input.into_iter().collect(),
            pos: 0,
        }
    }

    fn current(&self) -> Option<&TokenTree> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) {
        self.pos += 1;
    }

    fn expect_eq_after_key(&mut self, key_span: Span, key: &str) -> Result<(), ExpandError> {
        match self.tokens.get(self.pos + 1) {
            Some(TokenTree::Punct(p)) if p.as_char() == '=' => {
                self.pos += 2;
                Ok(())
            }
            _ => Err(ExpandError::new(
                key_span,
                format!("expected `{key} = ...`"),
            )),
        }
    }

    fn consume_separator(&mut self) -> Result<(), ExpandError> {
        let Some(token) = self.current() else {
            return Ok(());
        };
        match token {
            TokenTree::Punct(p) if p.as_char() == ',' => {
                self.advance();
                Ok(())
            }
            other => Err(ExpandError::new(other.span(), "expected `,` here")),
        }
    }

    fn take_string(&mut self, key_span: Span, key: &str) -> Result<StringArg, ExpandError> {
        let Some(token) = self.current().cloned() else {
            return Err(ExpandError::new(
                key_span,
                format!("`{key}` needs a string value"),
            ));
        };
        self.advance();
        string_value(&token)
    }

    /// `crate = ::some::path` — collect tokens up to the next top-level comma
    /// and splice them verbatim into generated code. Absolute paths only: the
    /// text is spliced into several nested modules, where a relative path
    /// would resolve differently in each.
    fn take_path(&mut self, key_span: Span) -> Result<String, ExpandError> {
        let mut path = String::new();
        while let Some(token) = self.current() {
            if let TokenTree::Punct(p) = token
                && p.as_char() == ','
            {
                break;
            }
            path.push_str(&token.to_string());
            self.advance();
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

    fn take_limit(&mut self, key_span: Span, key: LimitKey) -> Result<LimitArg, ExpandError> {
        let Some(token) = self.current().cloned() else {
            return Err(ExpandError::new(
                key_span,
                format!(
                    "`{}` needs a value: {}",
                    key.name(),
                    accepted_limit_values(key)
                ),
            ));
        };
        self.advance();
        match &token {
            TokenTree::Ident(ident) if ident.to_string() == "auto" => Ok(LimitArg::Auto),
            TokenTree::Ident(ident) if ident.to_string() == "unbounded" => {
                if key.allows_unbounded() {
                    return Ok(LimitArg::Unbounded);
                }
                Err(ExpandError::new(
                    key_span,
                    "`depth = unbounded` is not supported; use `depth = auto` or an integer",
                ))
            }
            TokenTree::Literal(_) => take_integer_limit(&token, key),
            other => Err(invalid_limit_value(other.span(), key)),
        }
    }
}

pub fn parse(input: TokenStream) -> Result<MacroArgs, ExpandError> {
    let mut cursor = ArgCursor::new(input);
    let call_span = Span::call_site();
    let mut args = ArgSlots::default();

    while let Some(token) = cursor.current().cloned() {
        match &token {
            // The one positional argument: the query string.
            TokenTree::Literal(_) => {
                let literal = string_value(&token)?;
                args.put_query(QuerySource::Inline { literal })?;
                cursor.advance();
            }
            TokenTree::Ident(ident) => {
                let key = ident.to_string();
                let key_span = ident.span();
                cursor.expect_eq_after_key(key_span, &key)?;
                match key.as_str() {
                    "grammar" => {
                        let value = cursor.take_string(key_span, &key)?;
                        put(&mut args.grammar, value, key_span, &key)?;
                    }
                    "file" => {
                        let path = cursor.take_string(key_span, &key)?;
                        args.put_query(QuerySource::File { path })?;
                    }
                    "crate" => {
                        let value = cursor.take_path(key_span)?;
                        put(&mut args.rt_crate, value, key_span, &key)?;
                    }
                    other => {
                        let Some(limit_key) = LimitKey::parse(other) else {
                            return Err(ExpandError::new(
                                key_span,
                                format!(
                                    "unknown argument `{other}`; `query!` accepts `grammar`, \
                                     `file`, `crate`, `fuel`, `memory`, `depth`, and the \
                                     query string"
                                ),
                            ));
                        };
                        let value = cursor.take_limit(key_span, limit_key)?;
                        args.limits.put(limit_key, value, key_span)?;
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
        cursor.consume_separator()?;
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

fn string_value(token: &TokenTree) -> Result<StringArg, ExpandError> {
    let text = token.to_string();
    match litrs::StringLit::parse(text) {
        Ok(lit) => Ok(StringArg {
            value: lit.value().to_string(),
            span: token.span(),
        }),
        Err(_) => Err(ExpandError::new(
            token.span(),
            "expected a string literal here",
        )),
    }
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

fn take_integer_limit(token: &TokenTree, key: LimitKey) -> Result<LimitArg, ExpandError> {
    let text = token.to_string();
    let Ok(lit) = litrs::IntegerLit::parse(text) else {
        return Err(invalid_limit_value(token.span(), key));
    };
    let Some(value) = lit.value::<u64>() else {
        return Err(invalid_limit_value(token.span(), key));
    };
    Ok(LimitArg::Of(value))
}

/// The value forms a limit accepts, as an error-message fragment. `depth` omits
/// `unbounded` because it refuses that value (see [`LimitKey::allows_unbounded`]).
fn accepted_limit_values(key: LimitKey) -> &'static str {
    if key.allows_unbounded() {
        "an unsigned integer, `auto`, or `unbounded`"
    } else {
        "an unsigned integer or `auto`"
    }
}

fn invalid_limit_value(span: Span, key: LimitKey) -> ExpandError {
    ExpandError::new(
        span,
        format!("`{}` needs {}", key.name(), accepted_limit_values(key)),
    )
}
