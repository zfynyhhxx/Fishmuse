use std::sync::atomic::{AtomicU64, Ordering};

use crate::MicroYuan;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CostPolicy {
    pub warning_at: MicroYuan,
    pub hard_stop_at: MicroYuan,
}

impl Default for CostPolicy {
    fn default() -> Self {
        Self {
            warning_at: MicroYuan(10_000_000),
            hard_stop_at: MicroYuan(20_000_000),
        }
    }
}

impl CostPolicy {
    #[must_use]
    pub fn decide(self, spent: MicroYuan, live_test: bool) -> BudgetDecision {
        if live_test && spent >= self.hard_stop_at {
            BudgetDecision::Block { spent }
        } else if spent >= self.warning_at {
            BudgetDecision::AllowWithWarning { spent }
        } else {
            BudgetDecision::Allow
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BudgetDecision {
    Allow,
    AllowWithWarning { spent: MicroYuan },
    Block { spent: MicroYuan },
}

#[derive(Debug, Default)]
pub struct AtomicSpendLedger {
    spent: AtomicU64,
}

impl AtomicSpendLedger {
    pub fn record(&self, cost: MicroYuan) -> Result<MicroYuan, SpendOverflow> {
        self.spent
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_add(cost.0)
            })
            .map(|previous| MicroYuan(previous + cost.0))
            .map_err(|_| SpendOverflow)
    }

    #[must_use]
    pub fn spent(&self) -> MicroYuan {
        MicroYuan(self.spent.load(Ordering::Acquire))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpendOverflow;

impl std::fmt::Display for SpendOverflow {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AI spend ledger overflow")
    }
}

impl std::error::Error for SpendOverflow {}
