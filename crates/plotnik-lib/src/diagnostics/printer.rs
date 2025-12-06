//! Builder-pattern printer for rendering diagnostics.

use std::fmt::Write;

use annotate_snippets::{AnnotationKind, Group, Level, Patch, Renderer, Snippet};
use rowan::TextRange;

use super::message::{DiagnosticMessage, Severity};

pub struct DiagnosticsPrinter<'a> {
    diagnostics: DiagnosticsSlice<'a>,
    source: &'a str,
    path: Option<&'a str>,
    colored: bool,
}

enum DiagnosticsSlice<'a> {
    Borrowed(&'a [DiagnosticMessage]),
    Refs(Vec<&'a DiagnosticMessage>),
}

impl<'a> DiagnosticsSlice<'a> {
    fn iter(&self) -> impl Iterator<Item = &DiagnosticMessage> {
        match self {
            DiagnosticsSlice::Borrowed(slice) => DiagnosticsIter::Borrowed(slice.iter()),
            DiagnosticsSlice::Refs(vec) => DiagnosticsIter::Refs(vec.iter()),
        }
    }
}

enum DiagnosticsIter<'a, 'b> {
    Borrowed(std::slice::Iter<'a, DiagnosticMessage>),
    Refs(std::slice::Iter<'b, &'a DiagnosticMessage>),
}

impl<'a, 'b> Iterator for DiagnosticsIter<'a, 'b> {
    type Item = &'a DiagnosticMessage;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            DiagnosticsIter::Borrowed(iter) => iter.next(),
            DiagnosticsIter::Refs(iter) => iter.next().copied(),
        }
    }
}

impl<'a> DiagnosticsPrinter<'a> {
    pub(crate) fn new(diagnostics: &'a [DiagnosticMessage], source: &'a str) -> Self {
        Self {
            diagnostics: DiagnosticsSlice::Borrowed(diagnostics),
            source,
            path: None,
            colored: false,
        }
    }

    pub(crate) fn from_refs(diagnostics: &[&'a DiagnosticMessage], source: &'a str) -> Self {
        Self {
            diagnostics: DiagnosticsSlice::Refs(diagnostics.to_vec()),
            source,
            path: None,
            colored: false,
        }
    }

    pub fn path(mut self, path: &'a str) -> Self {
        self.path = Some(path);
        self
    }

    pub fn colored(mut self, value: bool) -> Self {
        self.colored = value;
        self
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        self.format(&mut out).expect("String write never fails");
        out
    }

    pub fn format(&self, w: &mut impl Write) -> std::fmt::Result {
        let renderer = if self.colored {
            Renderer::styled()
        } else {
            Renderer::plain()
        };

        for (i, diag) in self.diagnostics.iter().enumerate() {
            let range = adjust_range(diag.range, self.source.len());

            let mut snippet = Snippet::source(self.source).line_start(1).annotation(
                AnnotationKind::Primary
                    .span(range.clone())
                    .label(&diag.message),
            );

            if let Some(p) = self.path {
                snippet = snippet.path(p);
            }

            for related in &diag.related {
                snippet = snippet.annotation(
                    AnnotationKind::Context
                        .span(adjust_range(related.range, self.source.len()))
                        .label(&related.message),
                );
            }

            let level = severity_to_level(diag.severity());
            let title_group = level.primary_title(&diag.message).element(snippet);

            let mut report: Vec<Group> = vec![title_group];

            if let Some(fix) = &diag.fix {
                report.push(
                    Level::HELP.secondary_title(&fix.description).element(
                        Snippet::source(self.source)
                            .line_start(1)
                            .patch(Patch::new(range, &fix.replacement)),
                    ),
                );
            }

            if i > 0 {
                w.write_char('\n')?;
            }
            write!(w, "{}", renderer.render(&report))?;
        }

        Ok(())
    }
}

fn severity_to_level(severity: Severity) -> Level<'static> {
    match severity {
        Severity::Error => Level::ERROR,
        Severity::Warning => Level::WARNING,
    }
}

fn adjust_range(range: TextRange, limit: usize) -> std::ops::Range<usize> {
    let start: usize = range.start().into();
    let end: usize = range.end().into();

    if start == end {
        return start..(start + 1).min(limit);
    }

    start..end
}
