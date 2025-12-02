#[cfg(test)]
mod test_helpers {
    use crate::Query;
    use crate::ast::{RenderOptions, render_diagnostics};

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

        pub fn dump_symbols(&self) -> String {
            self.printer().only_symbols(true).dump()
        }

        pub fn dump_errors(&self) -> String {
            render_diagnostics(self.source, &self.errors, None, RenderOptions::plain())
        }
    }
}
