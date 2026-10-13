use crate::{
    employees::{
        ActiveStatus, IdRequest, MAX_REVISION, RecordFilter, RevisionRequest, TimeOfDay,
        check_revision, mutation_result, normalize_optional_notes, require_not_archived,
        verify_metadata,
    },
    error::{AppError, AppResult, CommandResult, ErrorCode},
    metadata::{MetadataClock, clock_timestamp},
    shift_time::{ShiftTimeCalculation, calculate_shift_time, normalize_color},
    text::{compare_text_with_ids, normalize_required_text, stable_text_order_key},
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
#[serde(deny_unknown_fields)]
pub struct ShiftTemplateData {
    pub name: String,
    pub display_label: Option<String>,
    pub start_time: String,
    pub end_time: String,
    pub time_of_day: TimeOfDay,
    pub requires_recovery_after_shift: bool,
    pub is_night_shift: bool,
    pub color_hex: Option<String>,
    pub active_status: ActiveStatus,
    pub notes: Option<String>,
}

impl ShiftTemplateData {
    pub fn normalized(mut self) -> AppResult<(Self, ShiftTimeCalculation)> {
        self.name = normalize_required_text(&self.name)?;
        self.display_label = self
            .display_label
            .filter(|v| !v.trim().is_empty())
            .map(|v| normalize_required_text(&v))
            .transpose()?;
        self.notes = normalize_optional_notes(self.notes)?;
        self.color_hex = normalize_color(self.color_hex)?;
        let calculation = calculate_shift_time(&self.start_time, &self.end_time)?;
        Ok((self, calculation))
    }
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ShiftTemplate {
    pub id: String,
    pub seed_key: Option<String>,
    #[serde(flatten)]
    #[sqlx(flatten)]
    pub data: ShiftTemplateData,
    pub unpaid_lunch_minutes: i64,
    pub paid_minutes: i64,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
    pub revision: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateShiftTemplateRequest {
    pub template: ShiftTemplateData,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateShiftTemplateRequest {
    pub id: String,
    pub expected_revision: i64,
    pub template: ShiftTemplateData,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListShiftTemplatesRequest {
    #[serde(default)]
    pub filter: RecordFilter,
    pub search: Option<String>,
}

fn configuration_error() -> AppError {
    AppError::new(
        ErrorCode::ConfigurationInvalid,
        "A stored shift template requires investigation; it has not been changed.",
    )
}

pub fn verify_template(template: &ShiftTemplate) -> AppResult<ShiftTimeCalculation> {
    let (normalized, calculated) = template
        .data
        .clone()
        .normalized()
        .map_err(|_| configuration_error())?;
    if normalized != template.data
        || calculated.unpaid_lunch_minutes != template.unpaid_lunch_minutes
        || calculated.paid_minutes != template.paid_minutes
        || (template.archived_at.is_some() && template.data.active_status != ActiveStatus::Inactive)
        || template
            .seed_key
            .as_ref()
            .is_some_and(|key| !normalize_required_text(key).is_ok_and(|v| v == *key))
    {
        return Err(configuration_error());
    }
    verify_metadata(
        &template.created_at,
        &template.updated_at,
        &template.archived_at,
        template.revision,
    )?;
    Ok(calculated)
}

pub async fn read_template(tx: &mut Transaction<'_, Sqlite>, id: &str) -> AppResult<ShiftTemplate> {
    normalize_required_text(id)?;
    let template = sqlx::query_as::<_, ShiftTemplate>("SELECT * FROM shift_templates WHERE id = ?")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::NotFound,
                "The shift template could not be found.",
            )
        })?;
    verify_template(&template)?;
    Ok(template)
}
pub async fn get_template(pool: &SqlitePool, request: IdRequest) -> AppResult<ShiftTemplate> {
    let mut tx = pool.begin().await?;
    let row = read_template(&mut tx, &request.id).await?;
    tx.rollback().await?;
    Ok(row)
}
pub async fn list_templates(
    pool: &SqlitePool,
    request: ListShiftTemplatesRequest,
) -> AppResult<Vec<ShiftTemplate>> {
    let search = request
        .search
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(normalize_required_text)
        .transpose()?
        .map(|v| stable_text_order_key(&v));
    let (archive, active) = match request.filter {
        RecordFilter::Current => (Some(false), None),
        RecordFilter::Active => (Some(false), Some(ActiveStatus::Active)),
        RecordFilter::Inactive => (Some(false), Some(ActiveStatus::Inactive)),
        RecordFilter::Archived => (Some(true), None),
        RecordFilter::All => (None, None),
    };
    let mut tx = pool.begin().await?;
    let mut rows = sqlx::query_as::<_, ShiftTemplate>("SELECT * FROM shift_templates WHERE (?1 IS NULL OR (archived_at IS NOT NULL)=?1) AND (?2 IS NULL OR active_status=?2) ORDER BY name_order_key, id")
        .bind(archive).bind(active).fetch_all(&mut *tx).await?;
    tx.rollback().await?;
    for row in &rows {
        verify_template(row)?;
    }
    if let Some(search) = search {
        rows.retain(|r| {
            stable_text_order_key(&r.data.name).contains(&search)
                || r.data
                    .display_label
                    .as_ref()
                    .is_some_and(|v| stable_text_order_key(v).contains(&search))
        });
    }
    rows.sort_by(|a, b| compare_text_with_ids(&a.data.name, &a.id, &b.data.name, &b.id));
    Ok(rows)
}

async fn write_template(
    tx: &mut Transaction<'_, Sqlite>,
    row: &ShiftTemplate,
    create: bool,
) -> AppResult<()> {
    verify_template(row)?;
    let sql = if create {
        "INSERT INTO shift_templates (id,seed_key,name,name_order_key,display_label,start_time,end_time,time_of_day,unpaid_lunch_minutes,paid_minutes,requires_recovery_after_shift,is_night_shift,color_hex,active_status,notes,created_at,updated_at,archived_at,revision) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)"
    } else {
        // Seed identity and creation metadata are never rewritten by manager mutations.
        "UPDATE shift_templates SET name=?3,name_order_key=?4,display_label=?5,start_time=?6,end_time=?7,time_of_day=?8,unpaid_lunch_minutes=?9,paid_minutes=?10,requires_recovery_after_shift=?11,is_night_shift=?12,color_hex=?13,active_status=?14,notes=?15,updated_at=?17,archived_at=?18,revision=?19 WHERE id=?1 AND revision=?19-1"
    };
    let d = &row.data;
    let result = sqlx::query(sql)
        .bind(&row.id)
        .bind(&row.seed_key)
        .bind(&d.name)
        .bind(stable_text_order_key(&d.name))
        .bind(&d.display_label)
        .bind(&d.start_time)
        .bind(&d.end_time)
        .bind(d.time_of_day)
        .bind(row.unpaid_lunch_minutes)
        .bind(row.paid_minutes)
        .bind(d.requires_recovery_after_shift)
        .bind(d.is_night_shift)
        .bind(&d.color_hex)
        .bind(d.active_status)
        .bind(&d.notes)
        .bind(&row.created_at)
        .bind(&row.updated_at)
        .bind(&row.archived_at)
        .bind(row.revision)
        .execute(&mut **tx)
        .await?;
    if result.rows_affected() != 1 {
        return Err(AppError::new(
            ErrorCode::StaleRevision,
            "Reload the shift template before saving.",
        ));
    }
    Ok(())
}
pub async fn create_template(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: CreateShiftTemplateRequest,
) -> AppResult<ShiftTemplate> {
    let (data, calculation) = request.template.normalized()?;
    let timestamp = clock_timestamp(clock)?;
    let row = ShiftTemplate {
        id: uuid::Uuid::new_v4().to_string(),
        seed_key: None,
        data,
        unpaid_lunch_minutes: calculation.unpaid_lunch_minutes,
        paid_minutes: calculation.paid_minutes,
        created_at: timestamp.clone(),
        updated_at: timestamp,
        archived_at: None,
        revision: 1,
    };
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    write_template(&mut tx, &row, true).await?;
    tx.commit().await?;
    Ok(row)
}

/// One canonical transaction-scoped source dependency service. Employee exclusions
/// and future snapshot-backed historical schedule references do not block retirement.
#[derive(Clone, Copy)]
pub enum DependencySource {
    ShiftTemplate,
    Position,
    CoverageTemplate,
    HolidayCoverage,
}
pub struct SourceDependencies {
    pub count: i64,
    pub ids: Vec<String>,
    pub names: Vec<String>,
    pub omitted_source_id_count: usize,
}
pub async fn source_deactivation_dependencies(
    tx: &mut Transaction<'_, Sqlite>,
    source: DependencySource,
    id: &str,
) -> AppResult<SourceDependencies> {
    use crate::error::{DIAGNOSTIC_SOURCE_ID_BYTES, DIAGNOSTIC_SOURCE_ID_LIMIT};
    let source = match source {
        DependencySource::ShiftTemplate => "shift",
        DependencySource::Position => "position",
        DependencySource::CoverageTemplate => "coverage",
        DependencySource::HolidayCoverage => "holiday_coverage",
    };
    let count: i64 = sqlx::query_scalar("SELECT (SELECT count(*) FROM coverage_template_items WHERE active_status='active' AND archived_at IS NULL AND ((?1='shift' AND shift_template_id=?2) OR (?1='position' AND shift_position_id=?2) OR (?1='coverage' AND coverage_template_id=?2))) + (SELECT count(*) FROM holidays WHERE ?1 IN ('coverage','holiday_coverage') AND custom_coverage_template_id=?2 AND active_status='active' AND archived_at IS NULL)")
        .bind(source).bind(id).fetch_one(&mut **tx).await?;
    let rows: Vec<(String,String)> = sqlx::query_as("SELECT i.id,t.name || ' / ' || coalesce(i.display_label,p.display_label,p.name) AS name FROM coverage_template_items i JOIN coverage_templates t ON t.id=i.coverage_template_id JOIN shift_positions p ON p.id=i.shift_position_id WHERE i.active_status='active' AND i.archived_at IS NULL AND ((?1='shift' AND i.shift_template_id=?2) OR (?1='position' AND i.shift_position_id=?2) OR (?1='coverage' AND i.coverage_template_id=?2)) UNION ALL SELECT id,name FROM holidays WHERE ?1 IN ('coverage','holiday_coverage') AND custom_coverage_template_id=?2 AND active_status='active' AND archived_at IS NULL ORDER BY id LIMIT ?3")
        .bind(source).bind(id).bind(DIAGNOSTIC_SOURCE_ID_LIMIT as i64).fetch_all(&mut **tx).await?;
    let mut ids = Vec::with_capacity(rows.len());
    let mut names = Vec::with_capacity(rows.len());
    for (id, name) in rows {
        if id.len() > DIAGNOSTIC_SOURCE_ID_BYTES {
            return Err(configuration_error());
        }
        ids.push(id);
        names.push(name);
    }
    let omitted_source_id_count = usize::try_from(count)
        .map_err(|_| configuration_error())?
        .saturating_sub(ids.len());
    Ok(SourceDependencies {
        count,
        ids,
        names,
        omitted_source_id_count,
    })
}
pub async fn template_deactivation_dependencies(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
) -> AppResult<SourceDependencies> {
    source_deactivation_dependencies(tx, DependencySource::ShiftTemplate, id).await
}
pub async fn require_can_deactivate_source(
    tx: &mut Transaction<'_, Sqlite>,
    source: DependencySource,
    id: &str,
) -> AppResult<()> {
    let dependencies = match source {
        DependencySource::ShiftTemplate => template_deactivation_dependencies(tx, id).await?,
        _ => source_deactivation_dependencies(tx, source, id).await?,
    };
    if dependencies.count > 0 {
        let mut error = AppError::new(
            ErrorCode::ActiveDependencies,
            "Active coverage items or holidays use this source. Reassign, deactivate, or archive those dependencies before invalidating this source.",
        );
        error.context.id = Some(id.to_owned());
        error.context.dependency_ids = Some(dependencies.ids);
        error.context.dependency_names = Some(dependencies.names);
        error.context.dependency_count = Some(dependencies.count);
        error.context.omitted_source_id_count = Some(dependencies.omitted_source_id_count);
        return Err(error);
    }
    Ok(())
}
async fn require_can_deactivate(tx: &mut Transaction<'_, Sqlite>, id: &str) -> AppResult<()> {
    require_can_deactivate_source(tx, DependencySource::ShiftTemplate, id).await
}
fn advance_revision(row: &mut ShiftTemplate, clock: &dyn MetadataClock) -> AppResult<()> {
    row.revision = row
        .revision
        .checked_add(1)
        .filter(|v| *v <= MAX_REVISION)
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::InvalidRevision,
                "The revision limit has been reached.",
            )
        })?;
    row.updated_at = clock_timestamp(clock)?;
    Ok(())
}
pub async fn update_template(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: UpdateShiftTemplateRequest,
) -> CommandResult<ShiftTemplate> {
    mutation_result(
        async {
            let (data, calculated) = request.template.normalized()?;
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
            let mut row = read_template(&mut tx, &request.id).await?;
            check_revision(&row.id, row.revision, request.expected_revision)?;
            require_not_archived(&row.archived_at)?;
            if row.data == data {
                tx.rollback().await?;
                return Ok((row, false));
            }
            if row.data.active_status == ActiveStatus::Active
                && data.active_status == ActiveStatus::Inactive
            {
                require_can_deactivate(&mut tx, &row.id).await?;
            }
            if row.data.start_time != data.start_time || row.data.end_time != data.end_time {
                crate::coverage::validate_shift_change(&mut tx, &row.id, &data).await?;
            }
            row.data = data;
            row.unpaid_lunch_minutes = calculated.unpaid_lunch_minutes;
            row.paid_minutes = calculated.paid_minutes;
            advance_revision(&mut row, clock)?;
            write_template(&mut tx, &row, false).await?;
            tx.commit().await?;
            Ok((row, true))
        }
        .await,
    )
}
pub async fn archive_template(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: RevisionRequest,
) -> CommandResult<ShiftTemplate> {
    mutation_result(
        async {
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
            let mut row = read_template(&mut tx, &request.id).await?;
            check_revision(&row.id, row.revision, request.expected_revision)?;
            if row.archived_at.is_some() {
                tx.rollback().await?;
                return Ok((row, false));
            }
            require_can_deactivate(&mut tx, &row.id).await?;
            advance_revision(&mut row, clock)?;
            row.archived_at = Some(row.updated_at.clone());
            row.data.active_status = ActiveStatus::Inactive;
            write_template(&mut tx, &row, false).await?;
            tx.commit().await?;
            Ok((row, true))
        }
        .await,
    )
}

struct Seed {
    key: &'static str,
    name: &'static str,
    start: &'static str,
    end: &'static str,
    category: TimeOfDay,
    lunch: i64,
    paid: i64,
    recovery: bool,
    night: bool,
    color: &'static str,
}
const SEEDS: &[Seed] = &[
    Seed {
        key: "shift_day_0645_1515",
        name: "Day 0645–1515",
        start: "0645",
        end: "1515",
        category: TimeOfDay::Day,
        lunch: 30,
        paid: 480,
        recovery: false,
        night: false,
        color: "#DBEAFE",
    },
    Seed {
        key: "shift_t3_0900_1730",
        name: "T3 0900–1730",
        start: "0900",
        end: "1730",
        category: TimeOfDay::Day,
        lunch: 30,
        paid: 480,
        recovery: false,
        night: false,
        color: "#CCFBF1",
    },
    Seed {
        key: "shift_evening_1500_2330",
        name: "Evening 1500–2330",
        start: "1500",
        end: "2330",
        category: TimeOfDay::Evening,
        lunch: 30,
        paid: 480,
        recovery: true,
        night: false,
        color: "#EDE9FE",
    },
    Seed {
        key: "shift_weekday_midnight_1900_0700",
        name: "Weekday Midnight / Night Shift",
        start: "1900",
        end: "0700",
        category: TimeOfDay::Night,
        lunch: 30,
        paid: 690,
        recovery: true,
        night: true,
        color: "#E5E7EB",
    },
    Seed {
        key: "shift_wednesday_t2_0730_1600",
        name: "Wednesday T2",
        start: "0730",
        end: "1600",
        category: TimeOfDay::Day,
        lunch: 30,
        paid: 480,
        recovery: false,
        night: false,
        color: "#BFDBFE",
    },
    Seed {
        key: "shift_wednesday_t5_1600_0030",
        name: "Wednesday T5",
        start: "1600",
        end: "0030",
        category: TimeOfDay::Evening,
        lunch: 30,
        paid: 480,
        recovery: true,
        night: false,
        color: "#DDD6FE",
    },
    Seed {
        key: "shift_ortho_or_1900_2300",
        name: "ORTHO & OR",
        start: "1900",
        end: "2300",
        category: TimeOfDay::Evening,
        lunch: 0,
        paid: 240,
        recovery: true,
        night: false,
        color: "#FCE7F3",
    },
    Seed {
        key: "shift_wednesday_midnight_0015_0700",
        name: "Wednesday Midnight / Night Shift",
        start: "0015",
        end: "0700",
        category: TimeOfDay::Night,
        lunch: 0,
        paid: 405,
        recovery: true,
        night: true,
        color: "#D1D5DB",
    },
    Seed {
        key: "shift_t8_1000_1400",
        name: "T8 1000–1400",
        start: "1000",
        end: "1400",
        category: TimeOfDay::Day,
        lunch: 0,
        paid: 240,
        recovery: false,
        night: false,
        color: "#D1FAE5",
    },
    Seed {
        key: "shift_t12_1230_2100",
        name: "T12 1230–2100",
        start: "1230",
        end: "2100",
        category: TimeOfDay::Evening,
        lunch: 30,
        paid: 480,
        recovery: true,
        night: false,
        color: "#FEF3C7",
    },
    Seed {
        key: "shift_weekend_midnight_1930_0700",
        name: "Weekend Midnight / Night Shift",
        start: "1930",
        end: "0700",
        category: TimeOfDay::Night,
        lunch: 30,
        paid: 660,
        recovery: true,
        night: true,
        color: "#CBD5E1",
    },
];

/// The caller owns the initializer transaction. Retained seeds are validated, never reset.
pub async fn seed_shift_templates(
    tx: &mut Transaction<'_, Sqlite>,
    clock: &dyn MetadataClock,
) -> AppResult<()> {
    let timestamp = clock_timestamp(clock)?;
    for seed in SEEDS {
        let (data, calculation) = ShiftTemplateData {
            name: seed.name.to_owned(),
            display_label: None,
            start_time: seed.start.to_owned(),
            end_time: seed.end.to_owned(),
            time_of_day: seed.category,
            requires_recovery_after_shift: seed.recovery,
            is_night_shift: seed.night,
            color_hex: Some(seed.color.to_owned()),
            active_status: ActiveStatus::Active,
            notes: None,
        }
        .normalized()?;
        if calculation.unpaid_lunch_minutes != seed.lunch || calculation.paid_minutes != seed.paid {
            return Err(configuration_error());
        }
        if let Some(retained) =
            sqlx::query_as::<_, ShiftTemplate>("SELECT * FROM shift_templates WHERE seed_key = ?")
                .bind(seed.key)
                .fetch_optional(&mut **tx)
                .await?
        {
            verify_template(&retained)?;
            continue;
        }
        let row = ShiftTemplate {
            id: uuid::Uuid::new_v4().to_string(),
            seed_key: Some(seed.key.to_owned()),
            data,
            unpaid_lunch_minutes: calculation.unpaid_lunch_minutes,
            paid_minutes: calculation.paid_minutes,
            created_at: timestamp.clone(),
            updated_at: timestamp.clone(),
            archived_at: None,
            revision: 1,
        };
        write_template(tx, &row, true).await?;
    }
    Ok(())
}
pub async fn validate_templates(tx: &mut Transaction<'_, Sqlite>) -> AppResult<()> {
    let rows = sqlx::query_as::<_, ShiftTemplate>(
        "SELECT * FROM shift_templates ORDER BY name_order_key, id",
    )
    .fetch_all(&mut **tx)
    .await?;
    for row in &rows {
        verify_template(row)?;
    }
    Ok(())
}
