//! Builder-pattern printer for rendering diagnostics.

use std::fmt::Write;

use annotate_snippets::{AnnotationKind, Group, Level, Patch, Renderer, Snippet};
use rowan::TextRange;

use super::SourceMap;
use super::message::{DiagnosticMessage, Severity};

pub struct DiagnosticsPrinter<'a> {
    diagnostics: Vec<DiagnosticMessage>,
    sources: &'a SourceMap,
    colored: bool,
}

impl<'a> DiagnosticsPrinter<'a> {
    pub(crate) fn new(diagnostics: Vec<DiagnosticMessage>, sources: &'a SourceMap) -> Self {
        Self {
            diagnostics,
            sources,
            colored: false,
        }
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
            let primary_content = self.sources.content(diag.source);
            let range = adjust_range(diag.range, primary_content.len());

            let mut primary_snippet = Snippet::source(primary_content).line_start(1);
            if let Some(name) = self.source_path(diag.source) {
                primary_snippet = primary_snippet.path(name);
            }
            primary_snippet =
                primary_snippet.annotation(AnnotationKind::Primary.span(range.clone()));

            // Collect same-file and cross-file related info separately
            let mut cross_file_snippets = Vec::new();

            for related in &diag.related {
                if related.span.source == diag.source {
                    // Same file: add annotation to primary snippet
                    primary_snippet = primary_snippet.annotation(
                        AnnotationKind::Context
                            .span(adjust_range(related.span.range, primary_content.len()))
                            .label(&related.message),
                    );
                } else {
                    // Different file: create separate snippet
                    let related_content = self.sources.content(related.span.source);
                    let mut snippet = Snippet::source(related_content).line_start(1);
                    if let Some(name) = self.source_path(related.span.source) {
                        snippet = snippet.path(name);
                    }
                    snippet = snippet.annotation(
                        AnnotationKind::Context
                            .span(adjust_range(related.span.range, related_content.len()))
                            .label(&related.message),
                    );
                    cross_file_snippets.push(snippet);
                }
            }

            let level = severity_to_level(diag.severity());
            let mut title_group = level.primary_title(&diag.message).element(primary_snippet);

            for snippet in cross_file_snippets {
                title_group = title_group.element(snippet);
            }

            let mut report: Vec<Group> = vec![title_group];

            if let Some(fix) = &diag.fix {
                report.push(
                    Level::HELP.secondary_title(&fix.description).element(
                        Snippet::source(primary_content)
                            .line_start(1)
                            .patch(Patch::new(range, &fix.replacement)),
                    ),
                );
            }

            for hint in &diag.hints {
                report.push(Group::with_title(Level::HELP.secondary_title(hint)));
            }

            if i > 0 {
                w.write_str("\n\n")?;
            }
            write!(w, "{}", renderer.render(&report))?;
        }

        Ok(())
    }

    fn source_path(&self, source: crate::query::SourceId) -> Option<&'a str> {
        self.sources.path(source)
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
