use crate::ast::{ErrorStage, SyntaxError, render_errors};

use super::Query;

impl Query<'_> {
    pub fn errors(&self) -> &[SyntaxError] {
        &self.errors
    }

    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn errors_for_stage(&self, stage: ErrorStage) -> Vec<&SyntaxError> {
        self.errors.iter().filter(|e| e.stage == stage).collect()
    }

    pub fn has_parse_errors(&self) -> bool {
        self.errors.iter().any(|e| e.stage == ErrorStage::Parse)
    }

    pub fn has_validate_errors(&self) -> bool {
        self.errors.iter().any(|e| e.stage == ErrorStage::Validate)
    }

    pub fn has_resolve_errors(&self) -> bool {
        self.errors.iter().any(|e| e.stage == ErrorStage::Resolve)
    }

    pub fn has_escape_errors(&self) -> bool {
        self.errors.iter().any(|e| e.stage == ErrorStage::Escape)
    }

    pub fn dump_errors(&self) -> String {
        render_errors(self.source, &self.errors, None)
    }

    pub fn dump_errors_for_stage(&self, stage: ErrorStage) -> String {
        let filtered: Vec<_> = self.errors_for_stage(stage).into_iter().cloned().collect();
        render_errors(self.source, &filtered, None)
    }

    pub fn dump_errors_grouped(&self) -> String {
        let mut out = String::new();
        for stage in [
            ErrorStage::Parse,
            ErrorStage::Validate,
            ErrorStage::Resolve,
            ErrorStage::Escape,
        ] {
            let stage_errors: Vec<_> = self.errors_for_stage(stage).into_iter().cloned().collect();
            if !stage_errors.is_empty() {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&format!("=== {} errors ===\n", stage));
                out.push_str(&render_errors(self.source, &stage_errors, None));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use crate::ast::ErrorStage;
    use crate::query::Query;

    #[test]
    fn error_stage_filtering() {
        let q = Query::new("(unclosed");
        assert!(q.has_parse_errors());
        assert!(!q.has_resolve_errors());
        assert!(!q.has_escape_errors());
        assert_eq!(q.errors_for_stage(ErrorStage::Parse).len(), 1);

        let q = Query::new("(call (Undefined))");
        assert!(!q.has_parse_errors());
        assert!(q.has_resolve_errors());
        assert!(!q.has_escape_errors());
        assert_eq!(q.errors_for_stage(ErrorStage::Resolve).len(), 1);

        let q = Query::new("[A: (a) (b)]");
        assert!(!q.has_parse_errors());
        assert!(q.has_validate_errors());
        assert!(!q.has_resolve_errors());
        assert!(!q.has_escape_errors());
        assert_eq!(q.errors_for_stage(ErrorStage::Validate).len(), 1);

        let q = Query::new("Expr = (call (Expr))");
        assert!(!q.has_parse_errors());
        assert!(!q.has_validate_errors());
        assert!(!q.has_resolve_errors());
        assert!(q.has_escape_errors());
        assert_eq!(q.errors_for_stage(ErrorStage::Escape).len(), 1);

        let q = Query::new("Expr = (call (Expr)) (unclosed");
        assert!(q.has_parse_errors());
        assert!(!q.has_resolve_errors());
        assert!(q.has_escape_errors());
    }

    #[test]
    fn dump_errors_grouped() {
        let q = Query::new("Expr = (call (Expr)) (unclosed");
        let grouped = q.dump_errors_grouped();
        assert!(grouped.contains("=== parse errors ==="));
        assert!(grouped.contains("=== escape errors ==="));
        assert!(!grouped.contains("=== resolve errors ==="));
    }
}
