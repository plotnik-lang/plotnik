const INDENT: &str = "    ";

#[derive(Clone, Debug)]
pub(crate) struct Sink {
    text: String,
    indent: usize,
}

impl Sink {
    pub(crate) fn new() -> Self {
        Self {
            text: String::new(),
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

    pub(crate) fn plain(&self) -> &str {
        &self.text
    }
}

impl Default for Sink {
    fn default() -> Self {
        Self::new()
    }
}
