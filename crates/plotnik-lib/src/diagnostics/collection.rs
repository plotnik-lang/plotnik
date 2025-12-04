//! Diagnostics collection for accumulating compiler messages.

use super::message::{DiagnosticMessage, Severity};

/// Collection of diagnostic messages from parsing and analysis.
#[derive(Debug, Clone, Default)]
pub struct Diagnostics(Vec<DiagnosticMessage>);

impl Diagnostics {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn push(&mut self, msg: DiagnosticMessage) {
        self.0.push(msg);
    }

    pub fn extend(&mut self, iter: impl IntoIterator<Item = DiagnosticMessage>) {
        self.0.extend(iter);
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &DiagnosticMessage> {
        self.0.iter()
    }

    pub fn has_errors(&self) -> bool {
        self.0.iter().any(|d| d.is_error())
    }

    pub fn has_warnings(&self) -> bool {
        self.0.iter().any(|d| d.is_warning())
    }

    pub fn as_slice(&self) -> &[DiagnosticMessage] {
        &self.0
    }

    pub fn into_vec(self) -> Vec<DiagnosticMessage> {
        self.0
    }

    pub fn error_count(&self) -> usize {
        self.0.iter().filter(|d| d.is_error()).count()
    }

    pub fn warning_count(&self) -> usize {
        self.0.iter().filter(|d| d.is_warning()).count()
    }

    pub fn filter_by_severity(&self, severity: Severity) -> Vec<&DiagnosticMessage> {
        self.0.iter().filter(|d| d.severity == severity).collect()
    }
}

impl IntoIterator for Diagnostics {
    type Item = DiagnosticMessage;
    type IntoIter = std::vec::IntoIter<DiagnosticMessage>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a Diagnostics {
    type Item = &'a DiagnosticMessage;
    type IntoIter = std::slice::Iter<'a, DiagnosticMessage>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl FromIterator<DiagnosticMessage> for Diagnostics {
    fn from_iter<T: IntoIterator<Item = DiagnosticMessage>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}
