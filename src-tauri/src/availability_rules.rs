//! Read-only availability/PTO eligibility. Complete source intervals survive expansion.
use crate::{
    availability_pto::{
        AvailabilityEntry, AvailabilityType, PtoEntry, PtoStatus, verify_availability, verify_pto,
    },
    employees::{ActiveStatus, read_employee},
    entry_intervals::{EvaluationWindows, coalesce, midnight, touched_days},
    error::{
        AppError, AppResult, DIAGNOSTIC_LIMIT, DIAGNOSTIC_SOURCE_ID_BYTES,
        DIAGNOSTIC_SOURCE_ID_LIMIT, ErrorCode,
    },
    local_values::{
        ANALYSIS_EMPLOYEE_LIMIT, ANALYSIS_SOURCE_LIMIT, ExpandedIntervalBudget, LocalDateTime,
        LocalInterval, SourceRowBudget, input_limit_error,
    },
    rule_facts::MaterialFacts,
};
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::collections::{BTreeMap, BTreeSet};

pub use crate::error::AvailabilityRuleCode as RuleCode;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    Hard,
    Soft,
}
#[derive(Debug, Serialize)]
pub struct EligibilityFinding {
    pub code: RuleCode,
    pub severity: FindingSeverity,
    pub warning_id: String,
    pub source_ids: Vec<String>,
    pub omitted_source_id_count: usize,
}
#[derive(Debug, Clone, Serialize)]
pub struct AvailabilityFacts {
    pub has_available_overlap: bool,
    pub has_full_available_coverage: bool,
    pub has_relevant_preferred_entries: bool,
    pub has_preferred_overlap: bool,
    // There is no recurrence column in any supported schema, so these facts are complete.
    // Callers still consume an explicit completeness fact rather than infer it from findings.
    pub positive_availability_incomplete: bool,
}
/// Allocation-free rank facts. Diagnostic material is assembled only for retained
/// findings after selection, never for every generator candidate.
pub struct AvailabilityAssessment {
    pub facts: AvailabilityFacts,
    pub approved_pto: bool,
    pub hard_block: bool,
    pub requested_pto: bool,
}
#[derive(Debug, Serialize)]
pub struct EligibilityEvaluation {
    pub employee_id: String,
    pub facts: AvailabilityFacts,
    pub findings: Vec<EligibilityFinding>,
    pub omitted_diagnostic_count: usize,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SourceKind {
    Availability(AvailabilityType),
    Pto(PtoStatus),
}
#[derive(Debug)]
struct SourceInterval {
    id: String,
    kind: SourceKind,
    interval: LocalInterval,
}
/// `sources` keep complete provenance for material and diagnostics. The coalesced
/// per-kind unions answer every rank fact with one binary search per candidate.
#[derive(Default, Debug)]
struct EmployeeIntervals {
    sources: Vec<SourceInterval>,
    available_union: Vec<LocalInterval>,
    preferred_union: Vec<LocalInterval>,
    hard_union: Vec<LocalInterval>,
    approved_pto_union: Vec<LocalInterval>,
    requested_pto_union: Vec<LocalInterval>,
}
/// Immutable index reused for all candidates in the merged explicit windows.
#[derive(Debug)]
pub struct AvailabilityPtoIndex {
    windows: EvaluationWindows,
    employees: BTreeMap<String, EmployeeIntervals>,
}
impl AvailabilityPtoIndex {
    /// Complete available coverage is material to the PRN rule; unrelated hard
    /// sources and display text do not alter that warning's identity.
    pub(crate) fn positive_material(
        &self,
        employee: &str,
        candidate: LocalInterval,
        material: &mut MaterialFacts,
    ) -> AppResult<serde_json::Value> {
        self.windows.require_contains(candidate)?;
        let mut coverage = Vec::new();
        let mut omitted = 0usize;
        if let Some(target) = self.employees.get(employee) {
            for interval in &target.available_union {
                if interval.overlaps(candidate) {
                    let start = interval.start().max(candidate.start());
                    let end = interval.end().min(candidate.end());
                    material.interval(LocalInterval::new(start, end)?);
                    if coverage.len() < DIAGNOSTIC_SOURCE_ID_LIMIT {
                        coverage
                            .push(serde_json::json!({"start":start.format(), "end":end.format()}));
                    } else {
                        omitted += 1;
                    }
                }
            }
        }
        Ok(
            serde_json::json!({"available_coverage":coverage,"omitted_coverage_interval_count":omitted,"assignment_start":candidate.start().format(),"assignment_end":candidate.end().format()}),
        )
    }
    pub(crate) fn display_material(
        &self,
        employee: &str,
        candidate: LocalInterval,
        code: RuleCode,
    ) -> AppResult<serde_json::Value> {
        let mut intervals = Vec::new();
        let mut omitted = 0usize;
        for source in self.relevant_sources(employee, candidate)? {
            let applies = match code {
                RuleCode::ApprovedPtoConflict => {
                    source.kind == SourceKind::Pto(PtoStatus::Approved)
                        && source.interval.overlaps(candidate)
                }
                RuleCode::AvailabilityHardConflict => {
                    matches!(
                        source.kind,
                        SourceKind::Availability(
                            AvailabilityType::HardBlock | AvailabilityType::Unavailable
                        )
                    ) && source.interval.overlaps(candidate)
                }
                RuleCode::RequestedPtoOverlap => {
                    source.kind == SourceKind::Pto(PtoStatus::Requested)
                        && source.interval.overlaps(candidate)
                }
                RuleCode::PreferredAvailabilityNotMet => {
                    source.kind == SourceKind::Availability(AvailabilityType::Preferred)
                        && source.interval.end() > midnight(candidate.start().date())?
                }
                RuleCode::ExplicitAvailabilityMissing => {
                    source.kind == SourceKind::Availability(AvailabilityType::Available)
                        && source.interval.overlaps(candidate)
                }
            };
            if applies {
                if intervals.len() < DIAGNOSTIC_SOURCE_ID_LIMIT {
                    intervals.push(serde_json::json!({"source_id":source.id,"start":source.interval.start().format(),"end":source.interval.end().format()}));
                } else {
                    omitted += 1;
                }
            }
        }
        Ok(serde_json::json!({"source_intervals":intervals,"omitted_interval_count":omitted}))
    }
    pub fn append_hard_material(
        &self,
        employee: &str,
        candidate: LocalInterval,
        include_positive: bool,
        material: &mut MaterialFacts,
    ) -> AppResult<()> {
        for source in self.relevant_sources(employee, candidate)? {
            if !source.interval.overlaps(candidate) {
                continue;
            }
            let kind = match source.kind {
                SourceKind::Pto(PtoStatus::Approved) => "approved_pto",
                SourceKind::Availability(AvailabilityType::Unavailable) => "unavailable",
                SourceKind::Availability(AvailabilityType::HardBlock) => "hard_block",
                SourceKind::Availability(AvailabilityType::Available) if include_positive => {
                    "available"
                }
                _ => continue,
            };
            material.text(kind);
            material.text(&source.id);
            material.interval(source.interval);
        }
        Ok(())
    }
    fn relevant_sources(
        &self,
        employee: &str,
        candidate: LocalInterval,
    ) -> AppResult<&[SourceInterval]> {
        self.windows.require_contains(candidate)?;
        let (first, last) = touched_days(candidate)?;
        let lookback = midnight(first.subtract_days(1)?)?;
        let Some(target) = self.employees.get(employee) else {
            return Ok(&[]);
        };
        let begin = target
            .sources
            .partition_point(|s| s.interval.start() < lookback);
        let end = target
            .sources
            .partition_point(|s| s.interval.start().date() <= last);
        Ok(&target.sources[begin..end])
    }
    /// Equal to scanning `relevant_sources` for every fact. Every source interval
    /// lasts at most one civil day (`EntryShape::on_date`), so a source overlapping
    /// the candidate, or ending after its first midnight and starting by its last
    /// touched day, always lies in that lookback window. A positive-length interval
    /// overlaps a coalesced union exactly when it overlaps one of the union's members.
    pub fn assess(
        &self,
        employee: &str,
        candidate: LocalInterval,
    ) -> AppResult<AvailabilityAssessment> {
        // Same checks, day arithmetic and errors as `relevant_sources`.
        self.windows.require_contains(candidate)?;
        let (first, last) = touched_days(candidate)?;
        midnight(first.subtract_days(1)?)?;
        let first_midnight = midnight(first)?;
        let mut result = AvailabilityAssessment {
            facts: AvailabilityFacts {
                has_available_overlap: false,
                has_full_available_coverage: false,
                has_relevant_preferred_entries: false,
                has_preferred_overlap: false,
                positive_availability_incomplete: false,
            },
            approved_pto: false,
            hard_block: false,
            requested_pto: false,
        };
        let Some(t) = self.employees.get(employee) else {
            return Ok(result);
        };
        let overlaps_any = |union: &[LocalInterval]| {
            first_ending_after(union, candidate.start())
                .is_some_and(|u| u.start() < candidate.end())
        };
        result.facts.has_available_overlap = overlaps_any(&t.available_union);
        result.facts.has_relevant_preferred_entries =
            first_ending_after(&t.preferred_union, first_midnight)
                .is_some_and(|u| u.start().date() <= last);
        result.facts.has_preferred_overlap = overlaps_any(&t.preferred_union);
        result.hard_block = overlaps_any(&t.hard_union);
        result.approved_pto = overlaps_any(&t.approved_pto_union);
        result.requested_pto = overlaps_any(&t.requested_pto_union);
        let end = t
            .available_union
            .partition_point(|i| i.start() <= candidate.start());
        result.facts.has_full_available_coverage =
            end > 0 && t.available_union[end - 1].end() >= candidate.end();
        Ok(result)
    }
    /// Stream complete canonical material into the shared identity, with bounded
    /// display provenance. Supported storage has no recurrence or legacy columns.
    pub fn material_sources(
        &self,
        employee: &str,
        candidate: LocalInterval,
        code: RuleCode,
        material: &mut MaterialFacts,
    ) -> AppResult<(Vec<String>, usize)> {
        let mut sources: Vec<_> = self
            .relevant_sources(employee, candidate)?
            .iter()
            .filter(|s| match code {
                RuleCode::RequestedPtoOverlap => {
                    s.kind == SourceKind::Pto(PtoStatus::Requested)
                        && s.interval.overlaps(candidate)
                }
                RuleCode::PreferredAvailabilityNotMet => {
                    s.kind == SourceKind::Availability(AvailabilityType::Preferred)
                        && midnight(candidate.start().date())
                            .is_ok_and(|first| s.interval.end() > first)
                }
                _ => false,
            })
            .collect();
        sources.sort_unstable_by_key(|s| (&s.id, s.interval.start(), s.interval.end(), s.kind));
        let mut ids = Vec::new();
        let mut previous = None;
        let mut omitted = 0usize;
        for source in sources {
            if source.id.len() > DIAGNOSTIC_SOURCE_ID_BYTES {
                return Err(duplicate_source());
            }
            material.text(&source.id);
            material.interval(source.interval);
            if previous != Some(source.id.as_str()) {
                if ids.len() < DIAGNOSTIC_SOURCE_ID_LIMIT {
                    ids.push(source.id.clone());
                } else {
                    omitted = omitted.checked_add(1).ok_or_else(duplicate_source)?;
                }
                previous = Some(source.id.as_str());
            }
        }
        Ok((ids, omitted))
    }
    /// Share `budget` with RestrictionIndex::build in the same analysis snapshot.
    pub fn build(
        availability: &[AvailabilityEntry],
        pto: &[PtoEntry],
        windows: &[LocalInterval],
        budget: &mut ExpandedIntervalBudget,
    ) -> AppResult<Self> {
        let count = availability
            .len()
            .checked_add(pto.len())
            .ok_or_else(|| input_limit_error("source_rows", ANALYSIS_SOURCE_LIMIT, usize::MAX))?;
        if count > ANALYSIS_SOURCE_LIMIT {
            return Err(input_limit_error(
                "source_rows",
                ANALYSIS_SOURCE_LIMIT,
                count,
            ));
        }
        let windows = EvaluationWindows::new(windows)?;
        let mut employees: BTreeMap<String, EmployeeIntervals> = BTreeMap::new();
        let mut seen = BTreeSet::new();
        for row in availability {
            verify_availability(row)?;
            if !seen.insert((0, row.id.as_str())) {
                return Err(duplicate_source());
            }
            if row.archived_at.is_some() || row.data.active_status != ActiveStatus::Active {
                continue;
            }
            if !employees.contains_key(&row.data.employee_id)
                && employees.len() >= ANALYSIS_EMPLOYEE_LIMIT
            {
                return Err(input_limit_error(
                    "employee_rows",
                    ANALYSIS_EMPLOYEE_LIMIT,
                    employees.len() + 1,
                ));
            }
            let target = employees.entry(row.data.employee_id.clone()).or_default();
            for interval in row.data.shape()?.expand(&windows, budget)? {
                target.sources.push(SourceInterval {
                    id: row.id.clone(),
                    kind: SourceKind::Availability(row.data.entry_type),
                    interval,
                });
            }
        }
        for row in pto {
            verify_pto(row)?;
            if !seen.insert((1, row.id.as_str())) {
                return Err(duplicate_source());
            }
            if row.archived_at.is_some()
                || row.data.active_status != ActiveStatus::Active
                || !matches!(row.data.status, PtoStatus::Requested | PtoStatus::Approved)
            {
                continue;
            }
            if !employees.contains_key(&row.data.employee_id)
                && employees.len() >= ANALYSIS_EMPLOYEE_LIMIT
            {
                return Err(input_limit_error(
                    "employee_rows",
                    ANALYSIS_EMPLOYEE_LIMIT,
                    employees.len() + 1,
                ));
            }
            let target = employees.entry(row.data.employee_id.clone()).or_default();
            for interval in row.data.shape()?.expand(&windows, budget)? {
                target.sources.push(SourceInterval {
                    id: row.id.clone(),
                    kind: SourceKind::Pto(row.data.status),
                    interval,
                });
            }
        }
        for target in employees.values_mut() {
            target.sources.sort_unstable_by(|a, b| {
                (a.interval.start(), a.interval.end(), a.kind, &a.id).cmp(&(
                    b.interval.start(),
                    b.interval.end(),
                    b.kind,
                    &b.id,
                ))
            });
            let union = |kinds: &[SourceKind]| {
                coalesce(
                    target
                        .sources
                        .iter()
                        .filter(|s| kinds.contains(&s.kind))
                        .map(|s| s.interval)
                        .collect(),
                )
            };
            target.available_union =
                union(&[SourceKind::Availability(AvailabilityType::Available)])?;
            target.preferred_union =
                union(&[SourceKind::Availability(AvailabilityType::Preferred)])?;
            target.hard_union = union(&[
                SourceKind::Availability(AvailabilityType::HardBlock),
                SourceKind::Availability(AvailabilityType::Unavailable),
            ])?;
            target.approved_pto_union = union(&[SourceKind::Pto(PtoStatus::Approved)])?;
            target.requested_pto_union = union(&[SourceKind::Pto(PtoStatus::Requested)])?;
        }
        Ok(Self { windows, employees })
    }
    pub fn evaluate(
        &self,
        employee_id: &str,
        candidate: LocalInterval,
        requires_explicit_availability: bool,
    ) -> AppResult<EligibilityEvaluation> {
        self.windows.require_contains(candidate)?;
        let (first, last) = touched_days(candidate)?;
        let first_midnight = midnight(first)?;
        let lookback = midnight(first.subtract_days(1)?)?;
        let target = self.employees.get(employee_id);
        let mut relevant = Vec::new();
        if let Some(target) = target {
            let begin = target
                .sources
                .partition_point(|s| s.interval.start() < lookback);
            let end = target
                .sources
                .partition_point(|s| s.interval.start().date() <= last);
            for source in &target.sources[begin..end] {
                let selected = match source.kind {
                    SourceKind::Availability(AvailabilityType::Preferred) => {
                        source.interval.end() > first_midnight
                    }
                    _ => source.interval.overlaps(candidate),
                };
                if selected {
                    relevant.push(source);
                }
            }
        }
        // Canonical source identity order also permits bounded streaming provenance.
        relevant.sort_unstable_by_key(|s| (&s.id, s.interval.start(), s.interval.end(), s.kind));
        let available: Vec<_> = relevant
            .iter()
            .copied()
            .filter(|s| s.kind == SourceKind::Availability(AvailabilityType::Available))
            .collect();
        let preferred: Vec<_> = relevant
            .iter()
            .copied()
            .filter(|s| s.kind == SourceKind::Availability(AvailabilityType::Preferred))
            .collect();
        let facts = self.assess(employee_id, candidate)?.facts;
        let mut findings = Vec::new();
        for (code, severity, kinds) in [
            (
                RuleCode::ApprovedPtoConflict,
                FindingSeverity::Hard,
                vec![SourceKind::Pto(PtoStatus::Approved)],
            ),
            (
                RuleCode::AvailabilityHardConflict,
                FindingSeverity::Hard,
                vec![
                    SourceKind::Availability(AvailabilityType::Unavailable),
                    SourceKind::Availability(AvailabilityType::HardBlock),
                ],
            ),
            (
                RuleCode::RequestedPtoOverlap,
                FindingSeverity::Soft,
                vec![SourceKind::Pto(PtoStatus::Requested)],
            ),
        ] {
            let sources: Vec<_> = relevant
                .iter()
                .copied()
                .filter(|s| kinds.contains(&s.kind))
                .collect();
            if !sources.is_empty() {
                findings.push(finding(code, severity, employee_id, candidate, &sources)?);
            }
        }
        if requires_explicit_availability && !facts.has_full_available_coverage {
            findings.push(finding(
                RuleCode::ExplicitAvailabilityMissing,
                FindingSeverity::Hard,
                employee_id,
                candidate,
                &available,
            )?);
        }
        if facts.has_relevant_preferred_entries && !facts.has_preferred_overlap {
            findings.push(finding(
                RuleCode::PreferredAvailabilityNotMet,
                FindingSeverity::Soft,
                employee_id,
                candidate,
                &preferred,
            )?);
        }
        // This service has at most five aggregated findings; never truncate authoritative warnings.
        if findings.len() > DIAGNOSTIC_LIMIT {
            return Err(input_limit_error(
                "diagnostics",
                DIAGNOSTIC_LIMIT,
                findings.len(),
            ));
        }
        findings.sort_unstable_by_key(|f| f.code);
        Ok(EligibilityEvaluation {
            employee_id: employee_id.to_owned(),
            facts,
            findings,
            omitted_diagnostic_count: 0,
        })
    }
}
/// First coalesced interval ending after `instant`. Coalesced unions are sorted,
/// disjoint and non-touching, so their ends strictly increase.
fn first_ending_after(union: &[LocalInterval], instant: LocalDateTime) -> Option<LocalInterval> {
    union
        .get(union.partition_point(|u| u.end() <= instant))
        .copied()
}
fn duplicate_source() -> AppError {
    AppError::new(
        ErrorCode::ConfigurationInvalid,
        "Source IDs must be unique within each table in an analysis snapshot.",
    )
}
fn finding(
    code: RuleCode,
    severity: FindingSeverity,
    employee: &str,
    candidate: LocalInterval,
    sources: &[&SourceInterval],
) -> AppResult<EligibilityFinding> {
    let mut facts = MaterialFacts::new("availability_interval", code.as_str());
    facts.text(employee);
    facts.interval(candidate);
    // The index order is canonical. Hash every material interval before limiting display provenance.
    let mut source_ids = Vec::new();
    let mut previous_id: Option<&str> = None;
    let mut omitted_source_id_count = 0;
    for source in sources {
        if source.id.len() > DIAGNOSTIC_SOURCE_ID_BYTES {
            return Err(AppError::new(
                ErrorCode::ConfigurationInvalid,
                "A source ID exceeds the supported byte limit.",
            ));
        }
        facts.text(&source.id);
        facts.interval(source.interval);
        if previous_id != Some(source.id.as_str()) {
            if source_ids.len() < DIAGNOSTIC_SOURCE_ID_LIMIT {
                source_ids.push(source.id.clone());
            } else {
                omitted_source_id_count += 1;
            }
            previous_id = Some(source.id.as_str());
        }
    }
    Ok(EligibilityFinding {
        code,
        severity,
        warning_id: facts.finish(),
        source_ids,
        omitted_source_id_count,
    })
}

/// Owned rows loaded in the caller's consistent transaction; release it before expansion.
pub struct AvailabilityPtoSources {
    pub availability: Vec<AvailabilityEntry>,
    pub pto: Vec<PtoEntry>,
}
pub async fn load_sources(
    tx: &mut Transaction<'_, Sqlite>,
    employee_ids: &[String],
    windows: &[LocalInterval],
    budget: &mut SourceRowBudget,
) -> AppResult<AvailabilityPtoSources> {
    if employee_ids.len() > ANALYSIS_EMPLOYEE_LIMIT {
        return Err(input_limit_error(
            "employee_rows",
            ANALYSIS_EMPLOYEE_LIMIT,
            employee_ids.len(),
        ));
    }
    let windows = EvaluationWindows::new(windows)?;
    let mut availability = BTreeMap::new();
    let mut pto = BTreeMap::new();
    let ids: BTreeSet<_> = employee_ids.iter().collect();
    for id in ids {
        for &(first, last) in windows.source_days() {
            // Bounded keyset batches; a spanning row selected by separate windows
            // counts once before retention. No oversized source collection is returned.
            let mut after: Option<String> = None;
            loop {
                let rows = sqlx::query_as::<_, AvailabilityEntry>("SELECT * FROM availability_entries WHERE employee_id=?1 AND active_status='active' AND archived_at IS NULL AND start_date<=?2 AND end_date>=?3 AND (?4 IS NULL OR id>?4) ORDER BY id LIMIT 128")
                    .bind(id).bind(last.format()).bind(first.format()).bind(&after).fetch_all(&mut **tx).await?;
                if rows.is_empty() {
                    break;
                }
                for row in rows {
                    after = Some(row.id.clone());
                    if !availability.contains_key(&row.id) {
                        budget.reserve(1)?;
                        availability.insert(row.id.clone(), row);
                    }
                }
            }
            let mut after: Option<String> = None;
            loop {
                let rows = sqlx::query_as::<_, PtoEntry>("SELECT * FROM pto_entries WHERE employee_id=?1 AND active_status='active' AND archived_at IS NULL AND status IN ('requested','approved') AND start_date<=?2 AND end_date>=?3 AND (?4 IS NULL OR id>?4) ORDER BY id LIMIT 128")
                    .bind(id).bind(last.format()).bind(first.format()).bind(&after).fetch_all(&mut **tx).await?;
                if rows.is_empty() {
                    break;
                }
                for row in rows {
                    after = Some(row.id.clone());
                    if !pto.contains_key(&row.id) {
                        budget.reserve(1)?;
                        pto.insert(row.id.clone(), row);
                    }
                }
            }
        }
    }
    Ok(AvailabilityPtoSources {
        availability: availability.into_values().collect(),
        pto: pto.into_values().collect(),
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluateAvailabilityRequest {
    pub employee_id: String,
    pub actual_start: String,
    pub actual_end: String,
}
pub async fn evaluate_availability(
    pool: &SqlitePool,
    request: EvaluateAvailabilityRequest,
) -> AppResult<EligibilityEvaluation> {
    let candidate = LocalInterval::new(
        LocalDateTime::parse(&request.actual_start)?,
        LocalDateTime::parse(&request.actual_end)?,
    )?;
    let mut tx = pool.begin().await?;
    let employee = read_employee(&mut tx, &request.employee_id).await?;
    if employee.archived_at.is_some() || employee.data.active_status != ActiveStatus::Active {
        return Err(AppError::new(
            ErrorCode::InvalidEmployee,
            "Choose an active, non-archived employee to evaluate availability.",
        ));
    }
    let sources = load_sources(
        &mut tx,
        std::slice::from_ref(&employee.id),
        &[candidate],
        &mut SourceRowBudget::default(),
    )
    .await?;
    tx.rollback().await?;
    // Per-request CPU work owns its bounded snapshot and does not keep a connection open.
    tauri::async_runtime::spawn_blocking(move || {
        let index = AvailabilityPtoIndex::build(
            &sources.availability,
            &sources.pto,
            &[candidate],
            &mut ExpandedIntervalBudget::default(),
        )?;
        index.evaluate(
            &employee.id,
            candidate,
            employee.data.requires_explicit_availability,
        )
    })
    .await
    .map_err(|_| {
        AppError::new(
            ErrorCode::ConfigurationInvalid,
            "The availability analysis could not complete.",
        )
    })?
}
