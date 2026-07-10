//! A deterministic text sink with indentation, styles, and semantic ranges.
//!
//! Emitters write their text once. ANSI styling is retained as zero-width
//! events, while semantic tags retain ranges in the unstyled text. Callers can
//! then render plain text, colored text, or a source map without duplicating
//! the language renderer.

use super::ansi::{StyleChange, StyleEvent, render};

const INDENT: &str = "    ";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Style {
    Blue,
    Green,
    Dim,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TaggedRange<T> {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) tag: T,
}

#[derive(Clone, Debug)]
pub(crate) struct Sink<T> {
    text: String,
    tags: Vec<TaggedRange<T>>,
    styles: Vec<StyleEvent>,
    indent: usize,
}

impl<T> Sink<T> {
    pub(crate) fn new() -> Self {
        Self {
            text: String::new(),
            tags: Vec::new(),
            styles: Vec::new(),
            indent: 0,
        }
    }

    pub(crate) fn push(&mut self, text: &str) {
        self.text.push_str(text);
    }

    pub(crate) fn line(&mut self, text: &str) {
        if !text.is_empty() {
            self.text.push_str(&INDENT.repeat(self.indent));
            self.text.push_str(text);
        }
        self.text.push('\n');
    }

    /// Write every line at the current indentation, leaving blank lines empty.
    /// A trailing newline in `text` remains one trailing newline in the sink.
    pub(crate) fn lines(&mut self, text: &str) {
        for line in text.lines() {
            self.line(line);
        }
    }

    pub(crate) fn indented<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.indent += 1;
        let result = f(self);
        self.indent -= 1;
        result
    }

    pub(crate) fn set_style(&mut self, style: Style) {
        self.styles.push(StyleEvent {
            offset: self.text.len(),
            change: StyleChange::Set(style),
        });
    }

    pub(crate) fn reset_style(&mut self) {
        self.styles.push(StyleEvent {
            offset: self.text.len(),
            change: StyleChange::Reset,
        });
    }

    pub(crate) fn styled(&mut self, style: Style, text: &str) {
        self.set_style(style);
        self.push(text);
        self.reset_style();
    }

    pub(crate) fn tagged<R>(&mut self, tag: T, f: impl FnOnce(&mut Self) -> R) -> R {
        let start = self.text.len();
        let index = self.tags.len();
        self.tags.push(TaggedRange {
            start,
            end: start,
            tag,
        });
        let result = f(self);
        self.tags[index].end = self.text.len();
        result
    }

    pub(crate) fn append(&mut self, mut other: Self) {
        let offset = self.text.len();
        self.text.push_str(&other.text);
        self.tags.extend(other.tags.drain(..).map(|mut range| {
            range.start += offset;
            range.end += offset;
            range
        }));
        self.styles.extend(other.styles.drain(..).map(|mut event| {
            event.offset += offset;
            event
        }));
    }

    pub(crate) fn plain(&self) -> &str {
        &self.text
    }

    pub(crate) fn tags(&self) -> &[TaggedRange<T>] {
        &self.tags
    }

    pub(crate) fn render(&self, colors: crate::core::Colors) -> String {
        render(&self.text, &self.styles, colors)
    }
}

impl<T> Default for Sink<T> {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn indentation(level: usize) -> String {
    INDENT.repeat(level)
}
