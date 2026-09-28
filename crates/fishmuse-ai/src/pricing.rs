use time::{OffsetDateTime, Weekday, macros::datetime};

use crate::{AIUsage, ProviderId};

const TOKENS_PER_MILLION: u128 = 1_000_000;

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct MicroYuan(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Currency {
    Cny,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TokenRates {
    pub cached_input_per_million: MicroYuan,
    pub uncached_input_per_million: MicroYuan,
    pub output_per_million: MicroYuan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeakWindow {
    pub start_hour_utc: u8,
    pub end_hour_utc: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PriceSchedule {
    pub provider: ProviderId,
    pub model: String,
    pub effective_from: OffsetDateTime,
    pub currency: Currency,
    pub off_peak: TokenRates,
    pub peak: TokenRates,
    pub weekday_peak_windows_utc: Vec<PeakWindow>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CostEstimate {
    Known(MicroYuan),
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PricingError {
    ScheduleNotEffective,
    InvalidUsage,
    Overflow,
}

impl std::fmt::Display for PricingError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ScheduleNotEffective => {
                formatter.write_str("price schedule is not effective yet")
            }
            Self::InvalidUsage => formatter.write_str("AI usage is invalid"),
            Self::Overflow => formatter.write_str("AI cost calculation overflowed"),
        }
    }
}

impl std::error::Error for PricingError {}

#[must_use]
pub fn deepseek_flash_cny_schedule() -> PriceSchedule {
    PriceSchedule {
        provider: ProviderId::deepseek(),
        model: "deepseek-flash".to_owned(),
        effective_from: datetime!(2026-09-29 00:00 UTC),
        currency: Currency::Cny,
        off_peak: TokenRates {
            cached_input_per_million: MicroYuan(20_000),
            uncached_input_per_million: MicroYuan(1_000_000),
            output_per_million: MicroYuan(4_000_000),
        },
        peak: TokenRates {
            cached_input_per_million: MicroYuan(40_000),
            uncached_input_per_million: MicroYuan(2_000_000),
            output_per_million: MicroYuan(8_000_000),
        },
        // Beijing weekdays 09:00–12:00 and 14:00–18:00.
        weekday_peak_windows_utc: vec![
            PeakWindow {
                start_hour_utc: 1,
                end_hour_utc: 4,
            },
            PeakWindow {
                start_hour_utc: 6,
                end_hour_utc: 10,
            },
        ],
    }
}

pub fn estimate_cost(
    schedule: &PriceSchedule,
    usage: Option<&AIUsage>,
    at: OffsetDateTime,
) -> Result<CostEstimate, PricingError> {
    if at < schedule.effective_from {
        return Err(PricingError::ScheduleNotEffective);
    }
    let Some(usage) = usage else {
        return Ok(CostEstimate::Unknown);
    };
    let uncached = usage
        .input_tokens
        .checked_sub(usage.cached_input_tokens)
        .ok_or(PricingError::InvalidUsage)?;
    let rates = if is_peak(schedule, at) {
        schedule.peak
    } else {
        schedule.off_peak
    };
    let numerator = u128::from(usage.cached_input_tokens)
        .checked_mul(u128::from(rates.cached_input_per_million.0))
        .and_then(|value| {
            u128::from(uncached)
                .checked_mul(u128::from(rates.uncached_input_per_million.0))
                .and_then(|next| value.checked_add(next))
        })
        .and_then(|value| {
            u128::from(usage.output_tokens)
                .checked_mul(u128::from(rates.output_per_million.0))
                .and_then(|next| value.checked_add(next))
        })
        .ok_or(PricingError::Overflow)?;
    let rounded = numerator
        .checked_add(TOKENS_PER_MILLION - 1)
        .ok_or(PricingError::Overflow)?
        / TOKENS_PER_MILLION;
    let amount = u64::try_from(rounded).map_err(|_| PricingError::Overflow)?;
    Ok(CostEstimate::Known(MicroYuan(amount)))
}

fn is_peak(schedule: &PriceSchedule, at: OffsetDateTime) -> bool {
    if matches!(at.weekday(), Weekday::Saturday | Weekday::Sunday) {
        return false;
    }
    let hour = at.hour();
    schedule
        .weekday_peak_windows_utc
        .iter()
        .any(|window| hour >= window.start_hour_utc && hour < window.end_hour_utc)
}
