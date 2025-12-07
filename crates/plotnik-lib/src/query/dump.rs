//! Test-only dump methods for query inspection.

#[cfg(test)]
mod test_helpers {
    use crate::Query;
    use crate::infer::tyton;

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

        pub fn dump_with_cardinalities(&self) -> String {
            self.printer().with_cardinalities(true).dump()
        }

        pub fn dump_cst_with_cardinalities(&self) -> String {
            self.printer().raw(true).with_cardinalities(true).dump()
        }

        pub fn dump_symbols(&self) -> String {
            self.printer().only_symbols(true).dump()
        }

        pub fn dump_diagnostics(&self) -> String {
            self.diagnostics().render_filtered(self.source)
        }

        pub fn dump_diagnostics_raw(&self) -> String {
            self.diagnostics_raw().render(self.source)
        }

        pub fn dump_types(&self) -> String {
            tyton::emit(&self.type_table)
        }
    }
}
