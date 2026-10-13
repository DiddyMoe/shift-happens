//! Canonical bounded expansion for inclusive source dates and actual civil windows.
use crate::{
    error::{AppError, AppResult, ErrorCode},
    local_values::{
        ANALYSIS_SOURCE_LIMIT, ExpandedIntervalBudget, LocalDate, LocalDateTime, LocalInterval,
        LocalTime, input_limit_error,
    },
};

#[derive(Debug, Clone)]
pub struct EntryShape {
    first: LocalDate,
    last: LocalDate,
    start: LocalTime,
    end: LocalTime,
    end_offset: i64,
}
impl EntryShape {
    pub fn parse(
        start_date: &str,
        end_date: &str,
        full_day: bool,
        start_time: Option<&str>,
        end_time: Option<&str>,
    ) -> AppResult<Self> {
        let first = LocalDate::parse(start_date)?;
        let last = LocalDate::parse(end_date)?;
        if first > last {
            return Err(AppError::new(
                ErrorCode::InvalidInterval,
                "The end source date must be on or after the start source date.",
            ));
        }
        let (start, end, end_offset) = if full_day {
            if start_time.is_some() || end_time.is_some() {
                return Err(AppError::new(
                    ErrorCode::InvalidInterval,
                    "Full-day records must have no clock times.",
                ));
            }
            (LocalTime::parse("0000")?, LocalTime::parse("0000")?, 1)
        } else {
            let missing = || {
                AppError::new(
                    ErrorCode::InvalidInterval,
                    "Partial-day records need both distinct HHMM times.",
                )
            };
            let start = LocalTime::parse(start_time.ok_or_else(missing)?)?;
            let end = LocalTime::parse(end_time.ok_or_else(missing)?)?;
            if start == end {
                return Err(missing());
            }
            (start, end, i64::from(end < start))
        };
        // Validate the last endpoint even if a later evaluation selects a small earlier window.
        last.add_days(end_offset)?;
        Ok(Self {
            first,
            last,
            start,
            end,
            end_offset,
        })
    }
    pub fn on_date(&self, date: LocalDate) -> AppResult<LocalInterval> {
        LocalInterval::new(
            LocalDateTime::from_date_time(date, self.start, 0)?,
            LocalDateTime::from_date_time(date, self.end, self.end_offset)?,
        )
    }
    /// Count before materializing, retaining the complete intervals (never clipped).
    pub fn expand(
        &self,
        windows: &EvaluationWindows,
        budget: &mut ExpandedIntervalBudget,
    ) -> AppResult<Vec<LocalInterval>> {
        let ranges: Vec<_> = windows
            .source_days
            .iter()
            .filter_map(|&(a, b)| {
                let first = a.max(self.first);
                let last = b.min(self.last);
                (first <= last).then_some((first, last))
            })
            .collect();
        for &(first, last) in &ranges {
            budget.reserve_occurrences(usize::try_from(last.days_since(first) + 1).map_err(
                |_| input_limit_error("expanded_intervals", budget.limit, usize::MAX),
            )?)?;
        }
        let mut intervals = Vec::new();
        for (first, last) in ranges {
            let mut date = first;
            loop {
                intervals.push(self.on_date(date)?);
                if date == last {
                    break;
                }
                date = date.add_days(1)?;
            }
        }
        Ok(intervals)
    }
}

pub fn midnight(date: LocalDate) -> AppResult<LocalDateTime> {
    LocalDateTime::from_date_time(date, LocalTime::parse("0000")?, 0)
}
/// Civil days touched by a half-open interval; exact midnight excludes its end day.
pub fn touched_days(interval: LocalInterval) -> AppResult<(LocalDate, LocalDate)> {
    let end = interval.end();
    let last = if end == midnight(end.date())? {
        end.date().subtract_days(1)?
    } else {
        end.date()
    };
    Ok((interval.start().date(), last))
}

#[derive(Debug, Clone)]
pub struct EvaluationWindows {
    actual: Vec<LocalInterval>,
    source_days: Vec<(LocalDate, LocalDate)>,
}
impl EvaluationWindows {
    pub fn new(windows: &[LocalInterval]) -> AppResult<Self> {
        if windows.is_empty() {
            return Err(AppError::new(
                ErrorCode::InvalidInterval,
                "Provide an explicit actual evaluation window.",
            ));
        }
        if windows.len() > ANALYSIS_SOURCE_LIMIT {
            return Err(input_limit_error(
                "evaluation_windows",
                ANALYSIS_SOURCE_LIMIT,
                windows.len(),
            ));
        }
        let actual = coalesce(windows.to_vec())?;
        let mut ranges = actual
            .iter()
            .map(|&w| {
                let (first, last) = touched_days(w)?;
                // Checked carry-in: an unavailable preceding canonical date is a typed bounds error.
                Ok((first.subtract_days(1)?, last))
            })
            .collect::<AppResult<Vec<_>>>()?;
        ranges.sort_unstable();
        let mut source_days: Vec<(LocalDate, LocalDate)> = Vec::new();
        for (first, last) in ranges {
            if let Some(previous) = source_days.last_mut()
                && first.days_since(previous.1) <= 1
            {
                previous.1 = previous.1.max(last);
            } else {
                source_days.push((first, last));
            }
        }
        Ok(Self {
            actual,
            source_days,
        })
    }
    pub fn source_days(&self) -> &[(LocalDate, LocalDate)] {
        &self.source_days
    }
    pub fn require_contains(&self, candidate: LocalInterval) -> AppResult<()> {
        if self
            .actual
            .iter()
            .any(|w| w.start() <= candidate.start() && candidate.end() <= w.end())
        {
            Ok(())
        } else {
            Err(AppError::new(
                ErrorCode::ConfigurationInvalid,
                "The candidate is outside the bounded analysis snapshot's actual windows.",
            ))
        }
    }
}

pub fn coalesce(mut intervals: Vec<LocalInterval>) -> AppResult<Vec<LocalInterval>> {
    intervals.sort_unstable_by_key(|i| (i.start(), i.end()));
    let mut result: Vec<LocalInterval> = Vec::new();
    for interval in intervals {
        if let Some(previous) = result.last_mut()
            && interval.start() <= previous.end()
        {
            *previous = LocalInterval::new(previous.start(), previous.end().max(interval.end()))?;
        } else {
            result.push(interval);
        }
    }
    Ok(result)
}
