const INDENT: &str = "    ";

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
    indent: usize,
}

impl<T> Sink<T> {
    pub(crate) fn new() -> Self {
        Self {
            text: String::new(),
            tags: Vec::new(),
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
    }

    pub(crate) fn plain(&self) -> &str {
        &self.text
    }

    pub(crate) fn tags(&self) -> &[TaggedRange<T>] {
        &self.tags
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
