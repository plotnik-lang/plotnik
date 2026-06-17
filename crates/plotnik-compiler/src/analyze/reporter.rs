use rowan::TextRange;

use crate::SourceId;
use crate::diagnostics::{DiagnosticBuilder, DiagnosticKind, Diagnostics};

pub(crate) struct Reporter<'d> {
    source: SourceId,
    diag: &'d mut Diagnostics,
}

impl<'d> Reporter<'d> {
    pub(crate) fn new(source: SourceId, diag: &'d mut Diagnostics) -> Self {
        Self { source, diag }
    }

    pub(crate) fn source(&self) -> SourceId {
        self.source
    }

    pub(crate) fn report(
        &mut self,
        kind: DiagnosticKind,
        range: TextRange,
    ) -> DiagnosticBuilder<'_> {
        self.diag.report(self.source, kind, range)
    }

    pub(crate) fn swap_source(&mut self, source: SourceId) -> SourceId {
        std::mem::replace(&mut self.source, source)
    }
}
