//! Iterative, metered JSON rendering of query results.
//!
//! VM documents and generated results render through one writer, so both
//! produce the same bytes for the same result: records list fields in schema
//! order, an absent option is `null`, a variant is `{"$tag": ..., "$data":
//! {...}}` with `$data` only for payload cases, and a node is
//! `{"kind", "text", "span": [start, end]}`.
//!
//! Rendering never recurses on the result shape. Fuel is charged to the
//! result's operation account before output is produced, and the fuel permit
//! is released around every sink call, so a sink may render another result.
//! A failure can leave partial output in the sink.

use std::fmt;

use tree_sitter::Node;

use crate::LimitExceeded;
use crate::alloc::{Account, Buffer, Meter};

/// Bytes collected before one sink call.
const CHUNK: usize = 4096;

/// ANSI escapes around output tokens; all empty for plain output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    /// Object keys.
    pub key: &'static str,
    /// String values.
    pub string: &'static str,
    /// Punctuation and `null`.
    pub dim: &'static str,
    pub reset: &'static str,
}

impl Palette {
    pub const PLAIN: Self = Self {
        key: "",
        string: "",
        dim: "",
        reset: "",
    };

    pub const ANSI: Self = Self {
        key: "\x1b[34m",
        string: "\x1b[32m",
        dim: "\x1b[2m",
        reset: "\x1b[0m",
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    /// Two-space indentation, one value per line.
    pub pretty: bool,
    pub palette: Palette,
}

impl Style {
    pub const PRETTY: Self = Self {
        pretty: true,
        palette: Palette::PLAIN,
    };

    pub const COMPACT: Self = Self {
        pretty: false,
        palette: Palette::PLAIN,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonError {
    Limit(LimitExceeded),
    /// The sink refused output.
    Sink,
    /// A node's byte range is not a UTF-8 slice of the supplied source.
    Source {
        start: usize,
        end: usize,
    },
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsonError::Limit(error) => error.fmt(f),
            JsonError::Sink => f.write_str("the JSON sink refused output"),
            JsonError::Source { start, end } => write!(
                f,
                "node span {start}..{end} is not a UTF-8 slice of the supplied source"
            ),
        }
    }
}

impl std::error::Error for JsonError {}

impl From<LimitExceeded> for JsonError {
    fn from(error: LimitExceeded) -> Self {
        JsonError::Limit(error)
    }
}

/// The JSON structure of one generated result value.
///
/// Generated modules implement this for their result types. Leaves and
/// containers come from the impls in this module.
pub trait Json {
    fn shape(&self) -> Shape<'_>;
}

/// The named fields of a record or of a variant case payload.
pub trait Fields {
    fn count(&self) -> usize;
    fn field(&self, index: usize) -> (&'static str, &dyn Json);
}

/// The elements of a list.
pub trait Items {
    fn count(&self) -> usize;
    fn item(&self, index: usize) -> &dyn Json;
}

pub enum Shape<'a> {
    Null,
    Bool(bool),
    Text(&'a str),
    Node(Node<'a>),
    Record(&'a dyn Fields),
    Variant {
        tag: &'static str,
        data: Option<&'a dyn Fields>,
    },
    List(&'a dyn Items),
}

impl Json for Node<'_> {
    fn shape(&self) -> Shape<'_> {
        Shape::Node(*self)
    }
}

impl Json for &str {
    fn shape(&self) -> Shape<'_> {
        Shape::Text(self)
    }
}

impl Json for bool {
    fn shape(&self) -> Shape<'_> {
        Shape::Bool(*self)
    }
}

impl<T: Json> Json for Option<T> {
    fn shape(&self) -> Shape<'_> {
        match self {
            Some(value) => value.shape(),
            None => Shape::Null,
        }
    }
}

impl<T: Json> Json for &T {
    fn shape(&self) -> Shape<'_> {
        (**self).shape()
    }
}

impl<T: Json> Json for &[T] {
    fn shape(&self) -> Shape<'_> {
        Shape::List(self)
    }
}

impl<T: Json> Items for &[T] {
    fn count(&self) -> usize {
        <[T]>::len(self)
    }

    fn item(&self, index: usize) -> &dyn Json {
        &self[index]
    }
}

/// Render a generated result value.
pub(crate) fn write_value(
    account: &Account,
    value: &dyn Json,
    source: &str,
    style: Style,
    sink: &mut dyn fmt::Write,
) -> Result<(), JsonError> {
    let mut out = Writer::new(account, style, sink);
    let result = render_value(&mut out, value, source);
    out.finish(result)
}

#[derive(Clone, Copy)]
enum Pending<'a> {
    Fields(&'a dyn Fields),
    Items(&'a dyn Items),
    /// A variant's `$data` object closes before the variant's own object.
    CloseVariant,
}

#[derive(Clone, Copy)]
struct Open<'a> {
    container: Pending<'a>,
    next: usize,
    len: usize,
}

fn render_value<'a>(
    out: &mut Writer<'_, '_>,
    root: &'a dyn Json,
    source: &str,
) -> Result<(), JsonError> {
    let mut stack: Buffer<Open<'a>> = Buffer::new();
    let mut value = Some(root);
    loop {
        if let Some(current) = value.take() {
            out.charge(1)?;
            match out.meter().callback(|| current.shape()) {
                Shape::Null => out.null()?,
                Shape::Bool(value) => out.bool(value)?,
                Shape::Text(text) => out.string(text)?,
                Shape::Node(node) => out.node(node, source)?,
                Shape::Record(fields) => {
                    out.open_object()?;
                    let len = out.meter().callback(|| fields.count());
                    stack.push(out.meter(), Open::fields(fields, len))?;
                }
                Shape::Variant { tag, data } => {
                    out.open_object()?;
                    out.key("$tag")?;
                    out.string(tag)?;
                    if let Some(fields) = data {
                        out.key("$data")?;
                        out.open_object()?;
                        stack.push(out.meter(), Open::close_variant())?;
                        let len = out.meter().callback(|| fields.count());
                        stack.push(out.meter(), Open::fields(fields, len))?;
                    } else {
                        out.close()?;
                    }
                }
                Shape::List(items) => {
                    out.open_array()?;
                    let len = out.meter().callback(|| items.count());
                    stack.push(out.meter(), Open::items(items, len))?;
                }
            }
        }

        let Some(top) = stack.last_mut() else {
            return Ok(());
        };
        if top.next == top.len {
            stack.pop();
            out.close()?;
            continue;
        }
        let index = top.next;
        top.next += 1;
        match top.container {
            Pending::Fields(fields) => {
                let (name, field) = out.meter().callback(|| fields.field(index));
                out.key(name)?;
                value = Some(field);
            }
            Pending::Items(items) => value = Some(out.meter().callback(|| items.item(index))),
            Pending::CloseVariant => unreachable!("a variant closer has no children"),
        }
    }
}

impl<'a> Open<'a> {
    fn fields(fields: &'a dyn Fields, len: usize) -> Self {
        Self {
            container: Pending::Fields(fields),
            next: 0,
            len,
        }
    }

    fn items(items: &'a dyn Items, len: usize) -> Self {
        Self {
            container: Pending::Items(items),
            next: 0,
            len,
        }
    }

    fn close_variant() -> Self {
        Self {
            container: Pending::CloseVariant,
            next: 0,
            len: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct Level {
    array: bool,
    /// Some value was placed in this container.
    filled: bool,
    /// A key placed the separator for the next value.
    keyed: bool,
}

/// Token-level JSON output: separators, indentation, colors, escaping, and
/// chunked delivery to the sink.
pub(crate) struct Writer<'a, 'w> {
    account: &'a Account,
    meter: Option<Meter<'a>>,
    style: Style,
    sink: &'w mut dyn fmt::Write,
    chunk: [u8; CHUNK],
    len: usize,
    levels: Buffer<Level>,
}

impl<'a, 'w> Writer<'a, 'w> {
    pub(crate) fn new(account: &'a Account, style: Style, sink: &'w mut dyn fmt::Write) -> Self {
        Self {
            account,
            meter: Some(account.meter()),
            style,
            sink,
            chunk: [0; CHUNK],
            len: 0,
            levels: Buffer::new(),
        }
    }

    pub(crate) fn meter(&mut self) -> &mut Meter<'a> {
        self.meter
            .as_mut()
            .expect("the writer holds the fuel permit")
    }

    pub(crate) fn charge(&mut self, units: u64) -> Result<(), LimitExceeded> {
        self.meter().charge(units)
    }

    /// Flush remaining output, unless rendering already failed.
    pub(crate) fn finish(mut self, result: Result<(), JsonError>) -> Result<(), JsonError> {
        result?;
        self.flush()
    }

    pub(crate) fn null(&mut self) -> Result<(), JsonError> {
        self.begin_value()?;
        let dim = self.style.palette.dim;
        self.styled(dim, "null")
    }

    pub(crate) fn bool(&mut self, value: bool) -> Result<(), JsonError> {
        self.begin_value()?;
        self.put(if value { "true" } else { "false" })
    }

    pub(crate) fn string(&mut self, value: &str) -> Result<(), JsonError> {
        self.begin_value()?;
        self.quoted(self.style.palette.string, value)
    }

    pub(crate) fn node(&mut self, node: Node<'_>, source: &str) -> Result<(), JsonError> {
        let (start, end) = (node.start_byte(), node.end_byte());
        let text = source
            .get(start..end)
            .ok_or(JsonError::Source { start, end })?;
        self.open_object()?;
        self.key("kind")?;
        self.string(node.kind())?;
        self.key("text")?;
        self.string(text)?;
        self.key("span")?;
        self.begin_value()?;
        let dim = self.style.palette.dim;
        self.styled(dim, "[")?;
        self.number(start)?;
        self.styled(dim, ", ")?;
        self.number(end)?;
        self.styled(dim, "]")?;
        self.close()
    }

    pub(crate) fn open_object(&mut self) -> Result<(), JsonError> {
        self.open("{")
    }

    pub(crate) fn open_array(&mut self) -> Result<(), JsonError> {
        self.open("[")
    }

    pub(crate) fn key(&mut self, name: &str) -> Result<(), JsonError> {
        self.separate()?;
        let palette = self.style.palette;
        self.quoted(palette.key, name)?;
        self.styled(palette.dim, ":")?;
        if self.style.pretty {
            self.put(" ")?;
        }
        self.levels
            .last_mut()
            .expect("a key belongs to an open object")
            .keyed = true;
        Ok(())
    }

    /// Close the innermost object or array.
    pub(crate) fn close(&mut self) -> Result<(), JsonError> {
        let level = self.levels.pop().expect("close matches an open value");
        let closer = if level.array { "]" } else { "}" };
        if level.filled && self.style.pretty {
            self.newline()?;
        }
        let dim = self.style.palette.dim;
        self.styled(dim, closer)
    }

    fn open(&mut self, opener: &str) -> Result<(), JsonError> {
        self.begin_value()?;
        let dim = self.style.palette.dim;
        self.styled(dim, opener)?;
        let level = Level {
            array: opener == "[",
            filled: false,
            keyed: false,
        };
        let meter = self
            .meter
            .as_mut()
            .expect("the writer holds the fuel permit");
        self.levels.push(meter, level)?;
        Ok(())
    }

    /// Place the separator before a value: array elements get one here,
    /// object fields got one from their key.
    fn begin_value(&mut self) -> Result<(), JsonError> {
        let Some(level) = self.levels.last_mut() else {
            return Ok(());
        };
        if level.keyed {
            level.keyed = false;
            return Ok(());
        }
        self.separate()
    }

    fn separate(&mut self) -> Result<(), JsonError> {
        let level = self
            .levels
            .last_mut()
            .expect("a separated value has a container");
        let filled = std::mem::replace(&mut level.filled, true);
        if filled {
            let dim = self.style.palette.dim;
            self.styled(dim, ",")?;
        }
        if self.style.pretty {
            self.newline()?;
        }
        Ok(())
    }

    fn newline(&mut self) -> Result<(), JsonError> {
        self.put("\n")?;
        for _ in 0..self.levels.len() {
            self.put("  ")?;
        }
        Ok(())
    }

    fn number(&mut self, value: usize) -> Result<(), JsonError> {
        let mut digits = [0u8; 20];
        let mut at = digits.len();
        let mut rest = value;
        loop {
            at -= 1;
            digits[at] = b'0' + (rest % 10) as u8;
            rest /= 10;
            if rest == 0 {
                break;
            }
        }
        let text = std::str::from_utf8(&digits[at..]).expect("ASCII digits");
        self.put(text)
    }

    fn quoted(&mut self, color: &'static str, value: &str) -> Result<(), JsonError> {
        self.put(color)?;
        self.put("\"")?;
        self.escaped(value)?;
        self.put("\"")?;
        let reset = self.style.palette.reset;
        self.put(reset)
    }

    fn styled(&mut self, color: &'static str, token: &str) -> Result<(), JsonError> {
        self.put(color)?;
        self.put(token)?;
        let reset = self.style.palette.reset;
        self.put(reset)
    }

    fn escaped(&mut self, value: &str) -> Result<(), JsonError> {
        self.meter().charge_bytes(value.len() as u64)?;
        let mut plain = 0;
        for (at, byte) in value.bytes().enumerate() {
            let escape = match byte {
                b'"' => "\\\"",
                b'\\' => "\\\\",
                b'\n' => "\\n",
                b'\r' => "\\r",
                b'\t' => "\\t",
                0x08 => "\\b",
                0x0c => "\\f",
                0x00..=0x1f => "",
                _ => continue,
            };
            self.put(&value[plain..at])?;
            if escape.is_empty() {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                let code = [
                    b'\\',
                    b'u',
                    b'0',
                    b'0',
                    HEX[usize::from(byte >> 4)],
                    HEX[usize::from(byte & 0xf)],
                ];
                self.put(std::str::from_utf8(&code).expect("ASCII escape"))?;
            } else {
                self.put(escape)?;
            }
            plain = at + 1;
        }
        self.put(&value[plain..])
    }

    fn put(&mut self, text: &str) -> Result<(), JsonError> {
        if text.is_empty() {
            return Ok(());
        }
        self.meter().charge_bytes(text.len() as u64)?;

        let mut rest = text;
        while !rest.is_empty() {
            if self.len == CHUNK {
                self.flush()?;
            }
            let room = CHUNK - self.len;
            let mut take = rest.len().min(room);
            while !rest.is_char_boundary(take) {
                take -= 1;
            }
            if take == 0 {
                self.flush()?;
                continue;
            }
            self.chunk[self.len..self.len + take].copy_from_slice(&rest.as_bytes()[..take]);
            self.len += take;
            rest = &rest[take..];
        }
        Ok(())
    }

    /// Hand buffered output to the sink without holding the fuel permit.
    fn flush(&mut self) -> Result<(), JsonError> {
        if self.len == 0 {
            return Ok(());
        }
        let text = std::str::from_utf8(&self.chunk[..self.len])
            .expect("the chunk is filled at character boundaries");
        self.meter = None;
        let written = self.sink.write_str(text);
        self.meter = Some(self.account.meter());
        self.len = 0;
        written.map_err(|_| JsonError::Sink)
    }
}
