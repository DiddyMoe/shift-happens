//! Workplace civil values. No timezone conversion or implicit overnight repair.
use crate::error::{AppError, AppResult, ErrorCode};
use serde::{Deserialize, Serialize};
use time::{Date, Duration, Month, PrimitiveDateTime, Time};

pub const LOCAL_DATE_FORMAT: &str = "YYYY-MM-DD";
pub const LOCAL_TIME_FORMAT: &str = "HHMM";
pub const LOCAL_DATETIME_FORMAT: &str = "YYYY-MM-DDTHH:MM:SS";
pub const LOCAL_WORKPLACE_TIME_POLICY: &str = "wall_clock";
// Shared physical-source and expanded-occurrence ceilings for one analysis snapshot.
pub const SCHEDULE_DAY_LIMIT: usize = 370;
pub const ANALYSIS_SOURCE_LIMIT: usize = 250_000;
pub const ANALYSIS_EMPLOYEE_LIMIT: usize = 1_000;
pub const GENERATION_CANDIDATE_LIMIT: usize = 2_000_000;

pub struct ExpandedIntervalBudget {
    pub limit: usize,
    pub count: usize,
}

impl Default for ExpandedIntervalBudget {
    fn default() -> Self {
        Self {
            limit: ANALYSIS_SOURCE_LIMIT,
            count: 0,
        }
    }
}
impl ExpandedIntervalBudget {
    pub fn reserve_occurrence(&mut self) -> AppResult<()> {
        self.reserve_occurrences(1)
    }
    pub fn reserve_occurrences(&mut self, count: usize) -> AppResult<()> {
        let observed = self
            .count
            .checked_add(count)
            .ok_or_else(|| input_limit_error("expanded_intervals", self.limit, usize::MAX))?;
        if observed > self.limit {
            return Err(input_limit_error(
                "expanded_intervals",
                self.limit,
                observed,
            ));
        }
        self.count = observed;
        Ok(())
    }
}

pub fn input_limit_error(kind: &'static str, limit: usize, observed: usize) -> AppError {
    let mut error = AppError::new(
        ErrorCode::InputLimitExceeded,
        "The scheduling input exceeds its safe analysis limit. Reduce the relevant range or source records.",
    );
    error.context.limit_kind = Some(kind);
    error.context.limit = Some(limit);
    error.context.observed_count = Some(observed);
    error
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LocalDate(Date);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LocalTime(Time);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LocalDateTime(PrimitiveDateTime);

fn invalid(code: ErrorCode) -> AppError {
    AppError::new(code, "Use a valid canonical workplace date or time.")
}

fn digits(bytes: &[u8]) -> bool {
    bytes.iter().all(u8::is_ascii_digit)
}
fn number(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0, |n, b| n * 10 + u32::from(b - b'0'))
}

impl LocalDate {
    pub fn from_parts(year: i64, month: i64, day: i64) -> AppResult<Self> {
        if !(1..=9999).contains(&year) {
            return Err(invalid(ErrorCode::InvalidDate));
        }
        let month = u8::try_from(month)
            .ok()
            .and_then(|v| Month::try_from(v).ok())
            .ok_or_else(|| invalid(ErrorCode::InvalidDate))?;
        let day = u8::try_from(day).map_err(|_| invalid(ErrorCode::InvalidDate))?;
        Date::from_calendar_date(year as i32, month, day)
            .map(Self)
            .map_err(|_| invalid(ErrorCode::InvalidDate))
    }

    pub fn year(self) -> i64 {
        i64::from(self.0.year())
    }

    pub fn month(self) -> i64 {
        i64::from(u8::from(self.0.month()))
    }

    pub fn parse(value: &str) -> AppResult<Self> {
        let b = value.as_bytes();
        if b.len() != 10
            || b[4] != b'-'
            || b[7] != b'-'
            || !digits(&b[0..4])
            || !digits(&b[5..7])
            || !digits(&b[8..10])
        {
            return Err(invalid(ErrorCode::InvalidDate));
        }
        let year = number(&b[0..4]) as i32;
        if year == 0 {
            return Err(invalid(ErrorCode::InvalidDate));
        }
        let month =
            Month::try_from(number(&b[5..7]) as u8).map_err(|_| invalid(ErrorCode::InvalidDate))?;
        let date = Date::from_calendar_date(year, month, number(&b[8..10]) as u8)
            .map_err(|_| invalid(ErrorCode::InvalidDate))?;
        Ok(Self(date))
    }

    pub fn format(self) -> String {
        format!(
            "{:04}-{:02}-{:02}",
            self.0.year(),
            u8::from(self.0.month()),
            self.0.day()
        )
    }

    pub fn add_days(self, days: i64) -> AppResult<Self> {
        // Bound before constructing Duration so even i64::MIN/MAX is safe.
        let min = Date::from_calendar_date(1, Month::January, 1)
            .map_err(|_| invalid(ErrorCode::OutOfRange))?;
        let max = Date::from_calendar_date(9999, Month::December, 31)
            .map_err(|_| invalid(ErrorCode::OutOfRange))?;
        if days < (min - self.0).whole_days() || days > (max - self.0).whole_days() {
            return Err(invalid(ErrorCode::OutOfRange));
        }
        self.0
            .checked_add(Duration::days(days))
            .map(Self)
            .ok_or_else(|| invalid(ErrorCode::OutOfRange))
    }

    pub fn subtract_days(self, days: i64) -> AppResult<Self> {
        self.add_days(
            days.checked_neg()
                .ok_or_else(|| invalid(ErrorCode::OutOfRange))?,
        )
    }

    pub fn weekday(self) -> Weekday {
        match self.0.weekday() {
            time::Weekday::Sunday => Weekday::Sunday,
            time::Weekday::Monday => Weekday::Monday,
            time::Weekday::Tuesday => Weekday::Tuesday,
            time::Weekday::Wednesday => Weekday::Wednesday,
            time::Weekday::Thursday => Weekday::Thursday,
            time::Weekday::Friday => Weekday::Friday,
            time::Weekday::Saturday => Weekday::Saturday,
        }
    }

    pub fn days_since(self, other: Self) -> i64 {
        (self.0 - other.0).whole_days()
    }
}

impl LocalTime {
    pub fn minutes_since_midnight(self) -> i64 {
        i64::from(self.0.hour()) * 60 + i64::from(self.0.minute())
    }
    pub fn parse(value: &str) -> AppResult<Self> {
        let b = value.as_bytes();
        if b.len() != 4 || !digits(b) {
            return Err(invalid(ErrorCode::InvalidTime));
        }
        Time::from_hms(number(&b[..2]) as u8, number(&b[2..]) as u8, 0)
            .map(Self)
            .map_err(|_| invalid(ErrorCode::InvalidTime))
    }
    pub fn format(self) -> String {
        format!("{:02}{:02}", self.0.hour(), self.0.minute())
    }
}

impl LocalDateTime {
    pub fn date(self) -> LocalDate {
        LocalDate(self.0.date())
    }
    pub fn parse(value: &str) -> AppResult<Self> {
        let b = value.as_bytes();
        if b.len() != 19
            || !value.is_ascii()
            || b[10] != b'T'
            || b[13] != b':'
            || b[16] != b':'
            || !digits(&b[17..])
        {
            return Err(invalid(ErrorCode::InvalidDatetime));
        }
        let date = LocalDate::parse(&value[..10])?;
        let clock = LocalTime::parse(&format!("{}{}", &value[11..13], &value[14..16]))?;
        let time = Time::from_hms(clock.0.hour(), clock.0.minute(), number(&b[17..]) as u8)
            .map_err(|_| invalid(ErrorCode::InvalidDatetime))?;
        Ok(Self(PrimitiveDateTime::new(date.0, time)))
    }
    pub fn from_date_time(date: LocalDate, time: LocalTime, day_offset: i64) -> AppResult<Self> {
        Ok(Self(PrimitiveDateTime::new(
            date.add_days(day_offset)?.0,
            time.0,
        )))
    }
    pub fn format(self) -> String {
        format!(
            "{}T{:02}:{:02}:{:02}",
            LocalDate(self.0.date()).format(),
            self.0.hour(),
            self.0.minute(),
            self.0.second()
        )
    }
}

/// Exact whole scheduled wall-clock minutes; reject a nonintegral minute gap.
pub fn wall_clock_minutes(start: LocalDateTime, end: LocalDateTime) -> AppResult<i64> {
    let seconds = (end.0 - start.0).whole_seconds();
    if seconds % 60 != 0 {
        return Err(invalid(ErrorCode::InvalidInterval));
    }
    Ok(seconds / 60)
}

#[derive(Debug, Clone, Copy)]
pub struct LocalInterval {
    start: LocalDateTime,
    end: LocalDateTime,
}

impl LocalInterval {
    pub fn start(self) -> LocalDateTime {
        self.start
    }
    pub fn end(self) -> LocalDateTime {
        self.end
    }
    pub fn new(start: LocalDateTime, end: LocalDateTime) -> AppResult<Self> {
        if start >= end {
            return Err(invalid(ErrorCode::InvalidInterval));
        }
        Ok(Self { start, end })
    }
    pub fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Weekday {
    Sunday,
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
}

impl Weekday {
    pub fn parse(value: &str) -> AppResult<Self> {
        match value {
            "sunday" => Ok(Self::Sunday),
            "monday" => Ok(Self::Monday),
            "tuesday" => Ok(Self::Tuesday),
            "wednesday" => Ok(Self::Wednesday),
            "thursday" => Ok(Self::Thursday),
            "friday" => Ok(Self::Friday),
            "saturday" => Ok(Self::Saturday),
            _ => Err(invalid(ErrorCode::InvalidDate)),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sunday => "sunday",
            Self::Monday => "monday",
            Self::Tuesday => "tuesday",
            Self::Wednesday => "wednesday",
            Self::Thursday => "thursday",
            Self::Friday => "friday",
            Self::Saturday => "saturday",
        }
    }
    pub fn english_label(self) -> &'static str {
        match self {
            Self::Sunday => "Sunday",
            Self::Monday => "Monday",
            Self::Tuesday => "Tuesday",
            Self::Wednesday => "Wednesday",
            Self::Thursday => "Thursday",
            Self::Friday => "Friday",
            Self::Saturday => "Saturday",
        }
    }

    fn index(self) -> i64 {
        self as i64
    }
}

pub struct LocalDateRange {
    pub start: LocalDate,
    pub end_exclusive: LocalDate,
}

pub fn workweek_range(date: LocalDate, start_weekday: Weekday) -> AppResult<LocalDateRange> {
    let start =
        date.subtract_days((date.weekday().index() - start_weekday.index()).rem_euclid(7))?;
    Ok(LocalDateRange {
        start,
        end_exclusive: start.add_days(7)?,
    })
}

pub fn anchored_two_week_range(date: LocalDate, anchor: LocalDate) -> AppResult<LocalDateRange> {
    let days = (date.0 - anchor.0).whole_days();
    let start = anchor.add_days(
        days.div_euclid(14)
            .checked_mul(14)
            .ok_or_else(|| invalid(ErrorCode::OutOfRange))?,
    )?;
    Ok(LocalDateRange {
        start,
        end_exclusive: start.add_days(14)?,
    })
}

/// One shared physical-row budget for all rule tables selected in a snapshot.
pub struct SourceRowBudget {
    pub limit: usize,
    pub count: usize,
}
impl Default for SourceRowBudget {
    fn default() -> Self {
        Self {
            limit: ANALYSIS_SOURCE_LIMIT,
            count: 0,
        }
    }
}
impl SourceRowBudget {
    pub fn reserve(&mut self, count: i64) -> AppResult<()> {
        let count = usize::try_from(count)
            .map_err(|_| input_limit_error("source_rows", self.limit, usize::MAX))?;
        let observed = self
            .count
            .checked_add(count)
            .ok_or_else(|| input_limit_error("source_rows", self.limit, usize::MAX))?;
        if observed > self.limit {
            return Err(input_limit_error("source_rows", self.limit, observed));
        }
        self.count = observed;
        Ok(())
    }
}
