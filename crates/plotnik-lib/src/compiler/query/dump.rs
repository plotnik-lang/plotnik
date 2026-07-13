//! String dump methods for query inspection.

use crate::compiler::query::{BindOutcome, Query};

impl Query {
    pub fn dump_cst(&self) -> String {
        self.dump_cst_with_trivia(false)
    }

    pub fn dump_cst_with_trivia(&self, trivia: bool) -> String {
        self.printer().cst(true).with_trivia(trivia).dump()
    }

    pub fn dump_ast(&self) -> String {
        self.printer().dump()
    }

    pub fn dump_symbols(&self) -> String {
        self.printer().definitions_only(true).dump()
    }

    #[cfg(test)]
    pub(crate) fn dump_diagnostics(&self) -> String {
        self.diagnostics().render(self.source_map())
    }
}

impl BindOutcome {
    #[cfg(test)]
    pub(crate) fn dump_diagnostics(&self) -> String {
        self.diagnostics().render(self.source_map())
    }
}
