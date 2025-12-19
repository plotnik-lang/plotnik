//! Test-only dump methods for query inspection.

#[cfg(test)]
mod test_helpers {
    use crate::Query;

    impl Query<'_> {
        pub fn dump_cst(&self) -> String {
            self.printer().raw(true).dump()
        }

        pub fn dump_cst_full(&self) -> String {
            self.printer().raw(true).with_trivia(true).dump()
        }

        pub fn dump_ast(&self) -> String {
            self.printer().dump()
        }

        pub fn dump_with_arities(&self) -> String {
            self.printer().with_arities(true).dump()
        }

        pub fn dump_cst_with_arities(&self) -> String {
            self.printer().raw(true).with_arities(true).dump()
        }

        pub fn dump_symbols(&self) -> String {
            self.printer().only_symbols(true).dump()
        }

        pub fn dump_diagnostics(&self) -> String {
            self.diagnostics().render_filtered(self.source())
        }

        pub fn dump_diagnostics_raw(&self) -> String {
            self.diagnostics().render(self.source())
        }
    }
}
