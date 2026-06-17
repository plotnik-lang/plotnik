//! The diagnostics sink paired with the source it attributes reports to.

use rowan::TextRange;

use crate::SourceId;
use crate::diagnostics::{DiagnosticBuilder, DiagnosticKind, Diagnostics};

/// A diagnostics sink bound to the source the current walk attributes to.
///
/// Bundling source and sink drops the `source_id` that each `report` would
/// otherwise thread by hand, and keeps the active source from desyncing — it
/// changes only through `swap_source`.
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

    /// Swap the active source, returning the previous one. Prefer the visitor-side
    /// `with_source` helpers, which pair the swap with its restore.
    pub(crate) fn swap_source(&mut self, source: SourceId) -> SourceId {
        std::mem::replace(&mut self.source, source)
    }
}
