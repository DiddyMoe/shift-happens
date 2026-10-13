use crate::{
    error::{AppError, AppResult, ErrorCode},
    local_values::LocalTime,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ShiftTimeCalculation {
    pub elapsed_minutes: i64,
    pub unpaid_lunch_minutes: i64,
    pub paid_minutes: i64,
    pub overnight: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewShiftTimeRequest {
    pub start_time: String,
    pub end_time: String,
}

fn invalid() -> AppError {
    AppError::new(
        ErrorCode::InvalidShiftTemplate,
        "Use distinct start and end times less than 24 hours apart.",
    )
}

/// Template duration only. Operational coverage supplies its own explicit offsets.
/// All paid-time consumers reuse this calculation; no DST/UTC conversion occurs.
pub fn calculate_shift_time(start: &str, end: &str) -> AppResult<ShiftTimeCalculation> {
    let start = LocalTime::parse(start)?;
    let end = LocalTime::parse(end)?;
    if start == end {
        return Err(invalid());
    }
    let overnight = end < start;
    let elapsed_minutes = end
        .minutes_since_midnight()
        .checked_sub(start.minutes_since_midnight())
        .and_then(|v| v.checked_add(if overnight { 1440 } else { 0 }))
        .filter(|v| (1..=1439).contains(v))
        .ok_or_else(invalid)?;
    let unpaid_lunch_minutes = if elapsed_minutes >= 480 { 30 } else { 0 };
    let paid_minutes = elapsed_minutes
        .checked_sub(unpaid_lunch_minutes)
        .filter(|v| (1..=1439).contains(v))
        .ok_or_else(invalid)?;
    Ok(ShiftTimeCalculation {
        elapsed_minutes,
        unpaid_lunch_minutes,
        paid_minutes,
        overnight,
    })
}

pub fn normalize_color(value: Option<String>) -> AppResult<Option<String>> {
    value
        .map(|v| {
            let bytes = v.as_bytes();
            // Do not trim: whitespace is not part of an accepted color shape.
            if bytes.len() != 7 || bytes[0] != b'#' || !bytes[1..].iter().all(u8::is_ascii_hexdigit)
            {
                return Err(AppError::new(
                    ErrorCode::InvalidColor,
                    "Use a six-digit hex color such as #DBEAFE, or leave color empty.",
                ));
            }
            Ok(v.to_ascii_uppercase())
        })
        .transpose()
}
