
//! Builder-pattern printer for rendering diagnostics.

use std::fmt::Write;

use annotate_snippets::{AnnotationKind, Group, Level, Patch, Renderer, Snippet};
use rowan::TextRange;

use super::collection::Diagnostics;
use super::message::Severity;

/// Builder for rendering diagnostics with various options.
pub struct DiagnosticsPrinter<'d, 's> {
    diagnostics: &'d Diagnostics,
    source: Option<&'s str>,
    path: Option<&'s str>,
    colored: bool,
}

impl<'d, 's> DiagnosticsPrinter<'d, 's> {
    pub fn new(diagnostics: &'d Diagnostics) -> Self {
        Self {
            diagnostics,
            source: None,
            path: None,
            colored: false,
        }
    }

    pub fn source(mut self, source: &'s str) -> Self {
        self.source = Some(source);
        self
    }

    pub fn path(mut self, path: &'s str) -> Self {
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
        let Some(source) = self.source else {
            return self.format_plain(w);
        };

        if self.diagnostics.is_empty() {
            return Ok(());
        }

        let renderer = if self.colored {
            Renderer::styled()
        } else {
            Renderer::plain()
        };

        for (i, diag) in self.diagnostics.iter().enumerate() {
            let range = adjust_range(diag.range, source.len());

            let mut snippet = Snippet::source(source).line_start(1).annotation(
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
                        .span(adjust_range(related.range, source.len()))
                        .label(&related.message),
                );
            }

            let level = severity_to_level(diag.severity);
            let title_group = level.primary_title(&diag.message).element(snippet);

            let mut report: Vec<Group> = vec![title_group];

            if let Some(fix) = &diag.fix {
                report.push(
                    Level::HELP.secondary_title(&fix.description).element(
                        Snippet::source(source)
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

    fn format_plain(&self, w: &mut impl Write) -> std::fmt::Result {
        for (i, diag) in self.diagnostics.iter().enumerate() {
            if i > 0 {
                w.write_char('\n')?;
            }
            write!(w, "{}", diag)?;
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

impl Diagnostics {
    pub fn printer(&self) -> DiagnosticsPrinter<'_, '_> {
        DiagnosticsPrinter::new(self)
    }
}
