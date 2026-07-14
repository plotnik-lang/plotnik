use crate::compiler::analyze::grammar::DEFAULT_SATISFIABILITY_WORK_BUDGET;
use crate::compiler::parse::{DEFAULT_FUEL, DEFAULT_MAX_DEPTH, ParseConfig};

#[derive(Debug, Clone, Copy)]
pub(crate) struct CompilerLimits {
    parse: ParseLimits,
    references: ReferenceLimits,
    satisfiability: SatisfiabilityLimits,
}

impl Default for CompilerLimits {
    fn default() -> Self {
        Self {
            parse: ParseLimits {
                fuel: DEFAULT_FUEL,
                max_depth: DEFAULT_MAX_DEPTH,
            },
            references: ReferenceLimits {
                max_depth: DEFAULT_MAX_DEPTH,
            },
            satisfiability: SatisfiabilityLimits {
                automaton_max_depth: DEFAULT_MAX_DEPTH,
                work_budget: DEFAULT_SATISFIABILITY_WORK_BUDGET,
            },
        }
    }
}

impl CompilerLimits {
    pub(crate) fn parse(self) -> ParseLimits {
        self.parse
    }

    pub(crate) fn references(self) -> ReferenceLimits {
        self.references
    }

    pub(crate) fn satisfiability(self) -> SatisfiabilityLimits {
        self.satisfiability
    }

    pub(crate) fn with_parse_fuel(mut self, fuel: u32) -> Self {
        self.parse.fuel = fuel;
        self
    }

    pub(crate) fn with_parse_max_depth(mut self, max_depth: u32) -> Self {
        self.parse.max_depth = max_depth;
        self
    }

    pub(crate) fn with_reference_max_depth(mut self, max_depth: u32) -> Self {
        self.references.max_depth = max_depth;
        self
    }

    pub(crate) fn with_satisfiability_automaton_max_depth(mut self, max_depth: u32) -> Self {
        self.satisfiability.automaton_max_depth = max_depth;
        self
    }

    pub(crate) fn with_satisfiability_work_budget(mut self, work_budget: u64) -> Self {
        self.satisfiability.work_budget = work_budget;
        self
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ParseLimits {
    fuel: u32,
    max_depth: u32,
}

impl ParseLimits {
    pub(crate) fn config(self) -> ParseConfig {
        ParseConfig {
            fuel: self.fuel,
            max_depth: self.max_depth,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ReferenceLimits {
    pub(crate) max_depth: u32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SatisfiabilityLimits {
    pub(crate) automaton_max_depth: u32,
    pub(crate) work_budget: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_setters_are_stage_specific() {
        let limits = CompilerLimits::default()
            .with_parse_max_depth(11)
            .with_reference_max_depth(22)
            .with_satisfiability_automaton_max_depth(33);

        assert_eq!(limits.parse.max_depth, 11);
        assert_eq!(limits.references.max_depth, 22);
        assert_eq!(limits.satisfiability.automaton_max_depth, 33);
    }
}
