use std::{sync::Arc, thread};

use fishmuse_ai::{
    AIUsage, AtomicSpendLedger, BudgetDecision, CostEstimate, CostPolicy, MicroYuan, PricingError,
    deepseek_flash_cny_schedule, estimate_cost,
};
use time::macros::datetime;

fn usage(input: u64, cached: u64, output: u64) -> AIUsage {
    AIUsage {
        input_tokens: input,
        cached_input_tokens: cached,
        output_tokens: output,
    }
}

#[test]
fn calculates_cache_hit_miss_and_output_at_off_peak_rates() {
    let schedule = deepseek_flash_cny_schedule();
    let at = datetime!(2026-09-30 00:00 UTC);

    assert_eq!(
        estimate_cost(&schedule, Some(&usage(1_000_000, 1_000_000, 0)), at),
        Ok(CostEstimate::Known(MicroYuan(20_000)))
    );
    assert_eq!(
        estimate_cost(&schedule, Some(&usage(1_000_000, 0, 0)), at),
        Ok(CostEstimate::Known(MicroYuan(1_000_000)))
    );
    assert_eq!(
        estimate_cost(&schedule, Some(&usage(0, 0, 1_000_000)), at),
        Ok(CostEstimate::Known(MicroYuan(4_000_000)))
    );
}

#[test]
fn selects_peak_rates_at_exact_utc_boundaries() {
    let schedule = deepseek_flash_cny_schedule();

    assert_eq!(
        estimate_cost(
            &schedule,
            Some(&usage(1_000_000, 0, 0)),
            datetime!(2026-09-30 00:59 UTC)
        ),
        Ok(CostEstimate::Known(MicroYuan(1_000_000)))
    );
    assert_eq!(
        estimate_cost(
            &schedule,
            Some(&usage(1_000_000, 0, 0)),
            datetime!(2026-09-30 01:00 UTC)
        ),
        Ok(CostEstimate::Known(MicroYuan(2_000_000)))
    );
    assert_eq!(
        estimate_cost(
            &schedule,
            Some(&usage(1_000_000, 0, 0)),
            datetime!(2026-09-30 03:59 UTC)
        ),
        Ok(CostEstimate::Known(MicroYuan(2_000_000)))
    );
    assert_eq!(
        estimate_cost(
            &schedule,
            Some(&usage(1_000_000, 0, 0)),
            datetime!(2026-09-30 04:00 UTC)
        ),
        Ok(CostEstimate::Known(MicroYuan(1_000_000)))
    );
    assert_eq!(
        estimate_cost(
            &schedule,
            Some(&usage(0, 0, 1_000_000)),
            datetime!(2026-09-30 06:00 UTC)
        ),
        Ok(CostEstimate::Known(MicroYuan(8_000_000)))
    );
    assert_eq!(
        estimate_cost(
            &schedule,
            Some(&usage(0, 0, 1_000_000)),
            datetime!(2026-09-30 10:00 UTC)
        ),
        Ok(CostEstimate::Known(MicroYuan(4_000_000)))
    );
    assert_eq!(
        estimate_cost(
            &schedule,
            Some(&usage(1_000_000, 0, 0)),
            datetime!(2026-10-03 02:00 UTC)
        ),
        Ok(CostEstimate::Known(MicroYuan(1_000_000)))
    );
}

#[test]
fn handles_unknown_invalid_and_not_yet_effective_usage() {
    let schedule = deepseek_flash_cny_schedule();
    assert_eq!(
        estimate_cost(&schedule, None, datetime!(2026-09-30 00:00 UTC)),
        Ok(CostEstimate::Unknown)
    );
    assert_eq!(
        estimate_cost(
            &schedule,
            Some(&usage(3, 4, 0)),
            datetime!(2026-09-30 00:00 UTC)
        ),
        Err(PricingError::InvalidUsage)
    );
    assert_eq!(
        estimate_cost(
            &schedule,
            Some(&usage(1, 0, 0)),
            datetime!(2026-09-28 23:59 UTC)
        ),
        Err(PricingError::ScheduleNotEffective)
    );
}

#[test]
fn rounds_fractional_microyuan_up_conservatively() {
    let schedule = deepseek_flash_cny_schedule();
    assert_eq!(
        estimate_cost(
            &schedule,
            Some(&usage(1, 1, 0)),
            datetime!(2026-09-30 00:00 UTC)
        ),
        Ok(CostEstimate::Known(MicroYuan(1)))
    );
}

#[test]
fn hard_stop_only_blocks_explicit_live_tests() {
    let policy = CostPolicy::default();
    assert_eq!(policy.warning_at, MicroYuan(10_000_000));
    assert_eq!(policy.hard_stop_at, MicroYuan(20_000_000));
    assert_eq!(
        policy.decide(MicroYuan(9_999_999), true),
        BudgetDecision::Allow
    );
    assert_eq!(
        policy.decide(MicroYuan(10_000_000), true),
        BudgetDecision::AllowWithWarning {
            spent: MicroYuan(10_000_000)
        }
    );
    assert_eq!(
        policy.decide(MicroYuan(20_000_000), true),
        BudgetDecision::Block {
            spent: MicroYuan(20_000_000)
        }
    );
    assert_eq!(
        policy.decide(MicroYuan(20_000_000), false),
        BudgetDecision::AllowWithWarning {
            spent: MicroYuan(20_000_000)
        }
    );
}

#[test]
fn concurrent_spend_accumulation_is_atomic() {
    let ledger = Arc::new(AtomicSpendLedger::default());
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let ledger = Arc::clone(&ledger);
            thread::spawn(move || {
                for _ in 0..1_000 {
                    ledger.record(MicroYuan(125)).expect("no overflow");
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().expect("worker");
    }
    assert_eq!(ledger.spent(), MicroYuan(1_000_000));
}
