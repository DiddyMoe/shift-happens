use crate::error::{AppError, AppResult, ErrorCode};
use time::{OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};

pub trait MetadataClock: Send + Sync {
    fn now(&self) -> AppResult<String>;
}

pub struct SystemMetadataClock;

impl MetadataClock for SystemMetadataClock {
    fn now(&self) -> AppResult<String> {
        format_metadata_instant(OffsetDateTime::now_utc())
    }
}

fn invalid_metadata() -> AppError {
    AppError::new(
        ErrorCode::InvalidMetadata,
        "The stored metadata timestamp is invalid.",
    )
}

/// Accept RFC 3339 whole seconds and up to nine fractional digits. Never truncate.
pub fn parse_metadata_instant(value: &str) -> AppResult<OffsetDateTime> {
    if let Some((_, fraction)) = value.split_once('.') {
        let precision = fraction.bytes().take_while(u8::is_ascii_digit).count();
        if precision == 0 || precision > 9 {
            return Err(invalid_metadata());
        }
    }
    let instant = OffsetDateTime::parse(value, &Rfc3339).map_err(|_| invalid_metadata())?;
    // time's RFC3339 parser accepts a leap second by clamping it. Do not lose precision.
    if value.as_bytes().get(17..19) == Some(b"60") {
        return Err(invalid_metadata());
    }
    Ok(instant)
}

pub fn format_metadata_instant(instant: OffsetDateTime) -> AppResult<String> {
    let utc = instant
        .checked_to_offset(UtcOffset::UTC)
        .ok_or_else(invalid_metadata)?;
    if !(1..=9999).contains(&utc.year()) {
        return Err(invalid_metadata());
    }
    Ok(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:09}Z",
        utc.year(),
        u8::from(utc.month()),
        utc.day(),
        utc.hour(),
        utc.minute(),
        utc.second(),
        utc.nanosecond()
    ))
}

pub fn compare_metadata_instants(left: &str, right: &str) -> AppResult<std::cmp::Ordering> {
    Ok(parse_metadata_instant(left)?.cmp(&parse_metadata_instant(right)?))
}

pub fn clock_timestamp(clock: &dyn MetadataClock) -> AppResult<String> {
    // Keep all clock implementations subject to the canonical write format.
    let value = clock.now()?;
    if format_metadata_instant(parse_metadata_instant(&value)?)? != value {
        return Err(invalid_metadata());
    }
    Ok(value)
}
