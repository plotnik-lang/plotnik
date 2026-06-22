//! Test-only dump methods for query inspection.

#[cfg(test)]
mod test_helpers {
    use crate::query::{GrammarBoundQuery, Query};

    impl Query {
        pub fn dump_cst(&self) -> String {
            self.printer().cst(true).dump()
        }

        pub fn dump_cst_full(&self) -> String {
            self.printer().cst(true).with_trivia(true).dump()
        }

        pub fn dump_ast(&self) -> String {
            self.printer().dump()
        }

        pub fn dump_with_arities(&self) -> String {
            self.printer().with_arities(true).dump()
        }

        pub fn dump_cst_with_arities(&self) -> String {
            self.printer().cst(true).with_arities(true).dump()
        }

        pub fn dump_symbols(&self) -> String {
            self.printer().definitions_only(true).dump()
        }

        pub fn dump_diagnostics(&self) -> String {
            self.diagnostics().render(self.source_map())
        }

        pub fn dump_diagnostics_raw(&self) -> String {
            self.diagnostics().render_raw(self.source_map())
        }
    }

    impl GrammarBoundQuery {
        pub fn dump_diagnostics(&self) -> String {
            self.diagnostics().render(self.source_map())
        }

        pub fn dump_diagnostics_raw(&self) -> String {
            self.diagnostics().render_raw(self.source_map())
        }
    }
}
