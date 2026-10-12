use super::AnalysisLimit;

#[derive(Default)]
pub(super) struct Budget {
    remaining: u64,
}

impl Budget {
    pub(super) fn new(remaining: u64) -> Self {
        Self { remaining }
    }

    pub(super) fn remaining(&self) -> u64 {
        self.remaining
    }

    pub(super) fn spend(&mut self, work: u64) -> Result<(), AnalysisLimit> {
        if work > self.remaining {
            self.remaining = 0;
            return Err(AnalysisLimit::WorkBudget);
        }

        self.remaining -= work;
        Ok(())
    }

    pub(super) fn charge(&mut self) -> Result<(), AnalysisLimit> {
        self.spend(1)
    }
}
