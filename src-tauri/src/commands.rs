use crate::{
    availability_pto, availability_rules, coverage,
    database::{self, AppState, DatabaseHealth},
    employee_restrictions::{
        self, CreateRestrictionRequest, EmployeeRestriction, ListRestrictionsRequest,
        UpdateRestrictionRequest,
    },
    employees::{
        self, CreateEmployeeRequest, Employee, EmploymentDefaults, EmploymentType, IdRequest,
        ListEmployeesRequest, ResetDefaultsPreview, RevisionRequest, UpdateEmployeeRequest,
    },
    error::CommandResult,
    generation, holidays, position_restrictions, positions, schedule_assignments, schedules,
    settings::{self, SettingView, UpdateManagerSettingRequest},
    shift_templates::{
        self, CreateShiftTemplateRequest, ListShiftTemplatesRequest, ShiftTemplate,
        UpdateShiftTemplateRequest,
    },
    shift_time::{self, PreviewShiftTimeRequest, ShiftTimeCalculation},
    template_restrictions::{
        self, CreateTemplateRestrictionRequest, ListTemplateRestrictionsRequest,
        TemplateRestriction, UpdateTemplateRestrictionRequest,
    },
};
use tauri::Manager;

// These are the pre-existing explicit local operations, now behind the same
// single-instance gate as GUI startup. No new public diagnostic mode is added.
pub(crate) fn run_local_cli() -> bool {
    match std::env::args().nth(1).as_deref() {
        Some("--export-schedule") => crate::export_cli::execute(std::env::args().skip(2)),
        Some("--backup-database") => crate::backup_cli::execute(std::env::args().skip(2)),
        Some("--validate-schedule") => crate::validation::execute_cli(std::env::args().skip(2)),
        Some("--generation-diagnostic") => {
            crate::generation_diagnostic::execute(std::env::args().skip(2))
        }
        Some("--sqlite-engine-info") => {
            match tauri::async_runtime::block_on(database::loaded_sqlite_engine()) {
                Ok(engine) => {
                    let migrations: Vec<_> = database::MIGRATOR.iter().map(|m| serde_json::json!({
                        "version": m.version,
                        "checksum": m.checksum.iter().map(|b| format!("{b:02x}")).collect::<String>()
                    })).collect();
                    println!(
                        "{}",
                        serde_json::json!({ "application_version": env!("CARGO_PKG_VERSION"),
                        "engine": engine, "sqlx_and_backup_match": true, "migrations": migrations })
                    );
                }
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(1);
                }
            }
        }
        _ => return false,
    }
    true
}

/// Logs a failed command (codes only) and returns its wire result unchanged.
fn logged<T>(command: &'static str, result: CommandResult<T>) -> Result<CommandResult<T>, ()> {
    crate::diagnostics::command(command, &result);
    Ok(result)
}
/// Backup and restore commands log every decided outcome, not only failures.
fn file_operation<T>(
    command: &'static str,
    result: CommandResult<T>,
) -> Result<CommandResult<T>, ()> {
    crate::diagnostics::file_operation(command, &result);
    Ok(result)
}

#[tauri::command]
fn get_startup_status(
    context: tauri::State<'_, crate::startup::StartupContext>,
) -> CommandResult<crate::startup::StartupReport> {
    CommandResult::Success {
        data: context.report.clone(),
        warnings: vec![],
    }
}

#[tauri::command]
async fn save_preservation_backup(
    context: tauri::State<'_, crate::startup::StartupContext>,
    runtime: tauri::State<'_, crate::backup::BackupRuntime>,
    request: crate::backup::ManualBackupRequest,
) -> Result<CommandResult<crate::backup::BackupFile>, ()> {
    if request.destination_path.is_none() {
        return Ok(CommandResult::Canceled);
    }
    let Some(source) = &context.source else {
        return file_operation(
            "save_preservation_backup",
            CommandResult::Blocked {
                error: crate::error::AppError::new(
                    crate::error::ErrorCode::BackupSourceInvalid,
                    "This startup source is unavailable for safe preservation. Close the app and preserve its existing files.",
                ),
            },
        );
    };
    file_operation(
        "save_preservation_backup",
        crate::backup::preservation_backup(source, &runtime, request).await,
    )
}

#[tauri::command]
async fn build_print_dto(
    state: tauri::State<'_, AppState>,
    request: crate::exports::PrintRequest,
) -> Result<CommandResult<crate::export_snapshot::ExportSnapshot>, ()> {
    logged(
        "build_print_dto",
        crate::exports::print_snapshot(&state.paths, state.clock.as_ref(), request).await,
    )
}
#[tauri::command]
async fn export_schedule_xlsx(
    state: tauri::State<'_, AppState>,
    runtime: tauri::State<'_, crate::backup::BackupRuntime>,
    request: crate::exports::ExportRequest,
) -> Result<CommandResult<crate::exports::ExportFile>, ()> {
    logged(
        "export_schedule_xlsx",
        crate::exports::export(
            &state.paths,
            state.clock.clone(),
            runtime.destinations.clone(),
            request,
            crate::export_snapshot::ExportFormat::Xlsx,
        )
        .await,
    )
}
#[tauri::command]
async fn export_schedule_pdf(
    state: tauri::State<'_, AppState>,
    runtime: tauri::State<'_, crate::backup::BackupRuntime>,
    request: crate::exports::ExportRequest,
) -> Result<CommandResult<crate::exports::ExportFile>, ()> {
    logged(
        "export_schedule_pdf",
        crate::exports::export(
            &state.paths,
            state.clock.clone(),
            runtime.destinations.clone(),
            request,
            crate::export_snapshot::ExportFormat::Pdf,
        )
        .await,
    )
}

#[tauri::command]
async fn get_backup_screen(
    app: tauri::AppHandle,
) -> Result<CommandResult<crate::backup::BackupScreen>, ()> {
    let result = if let Some(state) = app.try_state::<AppState>() {
        crate::backup::screen(Some(&state.pool), state.clock.as_ref()).await
    } else if let Some(context) = app.try_state::<crate::startup::StartupContext>()
        && let Some(source) = &context.source
    {
        crate::backup::screen(None, source.clock.as_ref()).await
    } else {
        Err(crate::error::AppError::new(
            crate::error::ErrorCode::BackupSourceInvalid,
            "No classified application database is available for backup.",
        ))
    };
    logged(
        "get_backup_screen",
        match result {
            Ok((data, warnings)) => CommandResult::Success { data, warnings },
            Err(error) => CommandResult::Failure { error },
        },
    )
}

#[tauri::command]
async fn create_manual_backup(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, crate::backup::BackupRuntime>,
    request: crate::backup::ManualBackupRequest,
) -> Result<CommandResult<crate::backup::BackupFile>, ()> {
    if request.destination_path.is_none() {
        return Ok(CommandResult::Canceled);
    }
    let result = if let Some(state) = app.try_state::<AppState>() {
        crate::backup::manual_backup(
            state.paths.main.clone(),
            Some(&state.pool),
            state.clock.clone(),
            &runtime,
            request,
        )
        .await
    } else {
        CommandResult::Blocked {
            error: crate::error::AppError::new(
                crate::error::ErrorCode::BackupSourceInvalid,
                "No classified application database is available for backup.",
            ),
        }
    };
    file_operation("create_manual_backup", result)
}

fn restore_paths(app: &tauri::AppHandle) -> Option<crate::database::DatabasePaths> {
    if let Some(state) = app.try_state::<AppState>() {
        return Some(state.paths.clone());
    }
    app.try_state::<crate::startup::StartupContext>()
        .and_then(|context| context.paths.clone())
}
fn restore_unavailable<T>() -> CommandResult<T> {
    CommandResult::Blocked {
        error: crate::error::AppError::new(
            crate::error::ErrorCode::RestoreFailed,
            "The application data folder is unavailable, so a backup cannot be restored.",
        ),
    }
}

#[tauri::command]
async fn stage_database_restore(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, crate::backup::BackupRuntime>,
    request: crate::restore::StageRestoreRequest,
) -> Result<CommandResult<crate::restore::RestorePreview>, ()> {
    if request.source_path.is_none() {
        return Ok(CommandResult::Canceled);
    }
    let Some(paths) = restore_paths(&app) else {
        return file_operation("stage_database_restore", restore_unavailable());
    };
    let state = app.try_state::<AppState>();
    let clock: std::sync::Arc<dyn crate::metadata::MetadataClock> = match &state {
        Some(state) => state.clock.clone(),
        None => std::sync::Arc::new(crate::metadata::SystemMetadataClock),
    };
    file_operation(
        "stage_database_restore",
        crate::restore::stage(&paths, state.is_some(), clock, &runtime, request).await,
    )
}

#[tauri::command]
async fn apply_database_restore(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, crate::backup::BackupRuntime>,
    request: crate::restore::ApplyRestoreRequest,
) -> Result<CommandResult<crate::restore::RestoreScheduled>, ()> {
    let Some(paths) = restore_paths(&app) else {
        return file_operation("apply_database_restore", restore_unavailable());
    };
    let state = app.try_state::<AppState>();
    let clock: std::sync::Arc<dyn crate::metadata::MetadataClock> = match &state {
        Some(state) => state.clock.clone(),
        None => std::sync::Arc::new(crate::metadata::SystemMetadataClock),
    };
    let outcome = crate::restore::apply(
        &paths,
        state.as_ref().map(|state| &state.pool),
        clock,
        &runtime,
        request,
    )
    .await;
    if let Some(guard) = outcome.restart {
        // Let this response reach the window first. A restart requested from a
        // command runs the normal exit path, which releases the single-instance
        // mutex before the next process starts and applies the restore.
        let handle = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(750));
            handle.request_restart();
            drop(guard);
        });
    }
    file_operation("apply_database_restore", outcome.result)
}

#[tauri::command]
// Tauri requires Result for asynchronous borrowed State. The outer Ok transports
// the tagged wire result unchanged; service errors remain inside CommandResult.
async fn get_settings(
    state: tauri::State<'_, AppState>,
) -> Result<CommandResult<Vec<SettingView>>, ()> {
    logged(
        "get_settings",
        settings::get_settings(&state.pool).await.into(),
    )
}

#[tauri::command]
async fn update_manager_setting(
    state: tauri::State<'_, AppState>,
    request: UpdateManagerSettingRequest,
) -> Result<CommandResult<SettingView>, ()> {
    logged(
        "update_manager_setting",
        settings::update_manager_setting(&state.pool, state.clock.as_ref(), request).await,
    )
}

#[tauri::command]
async fn get_database_health(
    state: tauri::State<'_, AppState>,
) -> Result<CommandResult<DatabaseHealth>, ()> {
    logged(
        "get_database_health",
        database::get_database_health(&state.paths).await.into(),
    )
}

// Extend this single handler and build.rs's supported application manifest together.
#[tauri::command]
fn get_employment_defaults(
    _state: tauri::State<'_, AppState>,
    employment_type: EmploymentType,
) -> CommandResult<EmploymentDefaults> {
    CommandResult::Success {
        data: employees::employment_defaults(employment_type),
        warnings: Vec::new(),
    }
}
#[tauri::command]
async fn create_employee(
    state: tauri::State<'_, AppState>,
    request: CreateEmployeeRequest,
) -> Result<CommandResult<Employee>, ()> {
    logged(
        "create_employee",
        employees::create_employee(&state.pool, state.clock.as_ref(), request)
            .await
            .into(),
    )
}
#[tauri::command]
async fn get_employee(
    state: tauri::State<'_, AppState>,
    request: IdRequest,
) -> Result<CommandResult<Employee>, ()> {
    logged(
        "get_employee",
        employees::get_employee(&state.pool, request).await.into(),
    )
}
#[tauri::command]
async fn list_employees(
    state: tauri::State<'_, AppState>,
    request: ListEmployeesRequest,
) -> Result<CommandResult<Vec<Employee>>, ()> {
    logged(
        "list_employees",
        employees::list_employees(&state.pool, request).await.into(),
    )
}
#[tauri::command]
async fn update_employee(
    state: tauri::State<'_, AppState>,
    request: UpdateEmployeeRequest,
) -> Result<CommandResult<Employee>, ()> {
    logged(
        "update_employee",
        employees::update_employee(&state.pool, state.clock.as_ref(), request).await,
    )
}
#[tauri::command]
async fn archive_employee(
    state: tauri::State<'_, AppState>,
    request: RevisionRequest,
) -> Result<CommandResult<Employee>, ()> {
    logged(
        "archive_employee",
        employees::archive_employee(&state.pool, state.clock.as_ref(), request).await,
    )
}
#[tauri::command]
async fn preview_employment_defaults(
    state: tauri::State<'_, AppState>,
    request: RevisionRequest,
) -> Result<CommandResult<ResetDefaultsPreview>, ()> {
    logged(
        "preview_employment_defaults",
        employees::preview_employment_defaults(&state.pool, request)
            .await
            .into(),
    )
}
#[tauri::command]
async fn reset_employment_defaults(
    state: tauri::State<'_, AppState>,
    request: RevisionRequest,
) -> Result<CommandResult<Employee>, ()> {
    logged(
        "reset_employment_defaults",
        employees::reset_employment_defaults(&state.pool, state.clock.as_ref(), request).await,
    )
}
#[tauri::command]
async fn create_employee_restriction(
    state: tauri::State<'_, AppState>,
    request: CreateRestrictionRequest,
) -> Result<CommandResult<EmployeeRestriction>, ()> {
    logged(
        "create_employee_restriction",
        employee_restrictions::create_restriction(&state.pool, state.clock.as_ref(), request)
            .await
            .into(),
    )
}
#[tauri::command]
async fn list_employee_restrictions(
    state: tauri::State<'_, AppState>,
    request: ListRestrictionsRequest,
) -> Result<CommandResult<Vec<EmployeeRestriction>>, ()> {
    logged(
        "list_employee_restrictions",
        employee_restrictions::list_restrictions(&state.pool, request)
            .await
            .into(),
    )
}
#[tauri::command]
async fn update_employee_restriction(
    state: tauri::State<'_, AppState>,
    request: UpdateRestrictionRequest,
) -> Result<CommandResult<EmployeeRestriction>, ()> {
    logged(
        "update_employee_restriction",
        employee_restrictions::update_restriction(&state.pool, state.clock.as_ref(), request).await,
    )
}
#[tauri::command]
async fn archive_employee_restriction(
    state: tauri::State<'_, AppState>,
    request: RevisionRequest,
) -> Result<CommandResult<EmployeeRestriction>, ()> {
    logged(
        "archive_employee_restriction",
        employee_restrictions::archive_restriction(&state.pool, state.clock.as_ref(), request)
            .await,
    )
}

#[tauri::command]
fn preview_shift_time(
    _state: tauri::State<'_, AppState>,
    request: PreviewShiftTimeRequest,
) -> CommandResult<ShiftTimeCalculation> {
    shift_time::calculate_shift_time(&request.start_time, &request.end_time).into()
}
#[tauri::command]
async fn create_shift_template(
    state: tauri::State<'_, AppState>,
    request: CreateShiftTemplateRequest,
) -> Result<CommandResult<ShiftTemplate>, ()> {
    logged(
        "create_shift_template",
        shift_templates::create_template(&state.pool, state.clock.as_ref(), request)
            .await
            .into(),
    )
}
#[tauri::command]
async fn get_shift_template(
    state: tauri::State<'_, AppState>,
    request: IdRequest,
) -> Result<CommandResult<ShiftTemplate>, ()> {
    logged(
        "get_shift_template",
        shift_templates::get_template(&state.pool, request)
            .await
            .into(),
    )
}
#[tauri::command]
async fn list_shift_templates(
    state: tauri::State<'_, AppState>,
    request: ListShiftTemplatesRequest,
) -> Result<CommandResult<Vec<ShiftTemplate>>, ()> {
    logged(
        "list_shift_templates",
        shift_templates::list_templates(&state.pool, request)
            .await
            .into(),
    )
}
#[tauri::command]
async fn update_shift_template(
    state: tauri::State<'_, AppState>,
    request: UpdateShiftTemplateRequest,
) -> Result<CommandResult<ShiftTemplate>, ()> {
    logged(
        "update_shift_template",
        shift_templates::update_template(&state.pool, state.clock.as_ref(), request).await,
    )
}
#[tauri::command]
async fn archive_shift_template(
    state: tauri::State<'_, AppState>,
    request: RevisionRequest,
) -> Result<CommandResult<ShiftTemplate>, ()> {
    logged(
        "archive_shift_template",
        shift_templates::archive_template(&state.pool, state.clock.as_ref(), request).await,
    )
}
#[tauri::command]
async fn create_employee_shift_template_restriction(
    state: tauri::State<'_, AppState>,
    request: CreateTemplateRestrictionRequest,
) -> Result<CommandResult<TemplateRestriction>, ()> {
    logged(
        "create_employee_shift_template_restriction",
        template_restrictions::create_restriction(&state.pool, state.clock.as_ref(), request)
            .await
            .into(),
    )
}
#[tauri::command]
async fn get_employee_shift_template_restriction(
    state: tauri::State<'_, AppState>,
    request: IdRequest,
) -> Result<CommandResult<TemplateRestriction>, ()> {
    logged(
        "get_employee_shift_template_restriction",
        template_restrictions::get_restriction(&state.pool, request)
            .await
            .into(),
    )
}
#[tauri::command]
async fn list_employee_shift_template_restrictions(
    state: tauri::State<'_, AppState>,
    request: ListTemplateRestrictionsRequest,
) -> Result<CommandResult<Vec<TemplateRestriction>>, ()> {
    logged(
        "list_employee_shift_template_restrictions",
        template_restrictions::list_restrictions(&state.pool, request)
            .await
            .into(),
    )
}
#[tauri::command]
async fn update_employee_shift_template_restriction(
    state: tauri::State<'_, AppState>,
    request: UpdateTemplateRestrictionRequest,
) -> Result<CommandResult<TemplateRestriction>, ()> {
    logged(
        "update_employee_shift_template_restriction",
        template_restrictions::update_restriction(&state.pool, state.clock.as_ref(), request).await,
    )
}
#[tauri::command]
async fn archive_employee_shift_template_restriction(
    state: tauri::State<'_, AppState>,
    request: RevisionRequest,
) -> Result<CommandResult<TemplateRestriction>, ()> {
    logged(
        "archive_employee_shift_template_restriction",
        template_restrictions::archive_restriction(&state.pool, state.clock.as_ref(), request)
            .await,
    )
}

#[tauri::command]
async fn create_shift_position(
    state: tauri::State<'_, AppState>,
    request: positions::CreatePositionRequest,
) -> Result<CommandResult<positions::Position>, ()> {
    logged(
        "create_shift_position",
        positions::create_position(&state.pool, state.clock.as_ref(), request)
            .await
            .into(),
    )
}
#[tauri::command]
async fn get_shift_position(
    state: tauri::State<'_, AppState>,
    request: IdRequest,
) -> Result<CommandResult<positions::Position>, ()> {
    logged(
        "get_shift_position",
        positions::get_position(&state.pool, request).await.into(),
    )
}
#[tauri::command]
async fn list_shift_positions(
    state: tauri::State<'_, AppState>,
    request: positions::ListPositionsRequest,
) -> Result<CommandResult<Vec<positions::Position>>, ()> {
    logged(
        "list_shift_positions",
        positions::list_positions(&state.pool, request).await.into(),
    )
}
#[tauri::command]
async fn update_shift_position(
    state: tauri::State<'_, AppState>,
    request: positions::UpdatePositionRequest,
) -> Result<CommandResult<positions::Position>, ()> {
    logged(
        "update_shift_position",
        positions::update_position(&state.pool, state.clock.as_ref(), request).await,
    )
}
#[tauri::command]
async fn archive_shift_position(
    state: tauri::State<'_, AppState>,
    request: RevisionRequest,
) -> Result<CommandResult<positions::Position>, ()> {
    logged(
        "archive_shift_position",
        positions::archive_position(&state.pool, state.clock.as_ref(), request).await,
    )
}
#[tauri::command]
async fn create_coverage_template(
    state: tauri::State<'_, AppState>,
    request: coverage::CreateCoverageTemplateRequest,
) -> Result<CommandResult<coverage::CoverageTemplate>, ()> {
    logged(
        "create_coverage_template",
        coverage::create_template(&state.pool, state.clock.as_ref(), request)
            .await
            .into(),
    )
}
#[tauri::command]
async fn get_coverage_template(
    state: tauri::State<'_, AppState>,
    request: IdRequest,
) -> Result<CommandResult<coverage::CoverageTemplate>, ()> {
    logged(
        "get_coverage_template",
        coverage::get_template(&state.pool, request).await.into(),
    )
}
#[tauri::command]
async fn list_coverage_templates(
    state: tauri::State<'_, AppState>,
    request: coverage::ListCoverageTemplatesRequest,
) -> Result<CommandResult<Vec<coverage::CoverageTemplate>>, ()> {
    logged(
        "list_coverage_templates",
        coverage::list_templates(&state.pool, request).await.into(),
    )
}
#[tauri::command]
async fn update_coverage_template(
    state: tauri::State<'_, AppState>,
    request: coverage::UpdateCoverageTemplateRequest,
) -> Result<CommandResult<coverage::CoverageTemplate>, ()> {
    logged(
        "update_coverage_template",
        coverage::update_template(&state.pool, state.clock.as_ref(), request).await,
    )
}
#[tauri::command]
async fn archive_coverage_template(
    state: tauri::State<'_, AppState>,
    request: RevisionRequest,
) -> Result<CommandResult<coverage::CoverageTemplate>, ()> {
    logged(
        "archive_coverage_template",
        coverage::archive_template(&state.pool, state.clock.as_ref(), request).await,
    )
}
#[tauri::command]
async fn create_coverage_item(
    state: tauri::State<'_, AppState>,
    request: coverage::CreateCoverageItemRequest,
) -> Result<CommandResult<coverage::CoverageItem>, ()> {
    logged(
        "create_coverage_item",
        coverage::create_item(&state.pool, state.clock.as_ref(), request)
            .await
            .into(),
    )
}
#[tauri::command]
async fn get_coverage_item(
    state: tauri::State<'_, AppState>,
    request: IdRequest,
) -> Result<CommandResult<coverage::CoverageItem>, ()> {
    logged(
        "get_coverage_item",
        coverage::get_item(&state.pool, request).await.into(),
    )
}
#[tauri::command]
async fn list_coverage_items(
    state: tauri::State<'_, AppState>,
    request: coverage::ListCoverageItemsRequest,
) -> Result<CommandResult<Vec<coverage::CoverageItem>>, ()> {
    logged(
        "list_coverage_items",
        coverage::list_items(&state.pool, request).await.into(),
    )
}
#[tauri::command]
async fn update_coverage_item(
    state: tauri::State<'_, AppState>,
    request: coverage::UpdateCoverageItemRequest,
) -> Result<CommandResult<coverage::CoverageItem>, ()> {
    logged(
        "update_coverage_item",
        coverage::update_item(&state.pool, state.clock.as_ref(), request).await,
    )
}
#[tauri::command]
async fn archive_coverage_item(
    state: tauri::State<'_, AppState>,
    request: RevisionRequest,
) -> Result<CommandResult<coverage::CoverageItem>, ()> {
    logged(
        "archive_coverage_item",
        coverage::archive_item(&state.pool, state.clock.as_ref(), request).await,
    )
}
#[tauri::command]
async fn create_employee_position_restriction(
    state: tauri::State<'_, AppState>,
    request: position_restrictions::CreatePositionRestrictionRequest,
) -> Result<CommandResult<position_restrictions::PositionRestriction>, ()> {
    logged(
        "create_employee_position_restriction",
        position_restrictions::create_restriction(&state.pool, state.clock.as_ref(), request)
            .await
            .into(),
    )
}
#[tauri::command]
async fn get_employee_position_restriction(
    state: tauri::State<'_, AppState>,
    request: IdRequest,
) -> Result<CommandResult<position_restrictions::PositionRestriction>, ()> {
    logged(
        "get_employee_position_restriction",
        position_restrictions::get_restriction(&state.pool, request)
            .await
            .into(),
    )
}
#[tauri::command]
async fn list_employee_position_restrictions(
    state: tauri::State<'_, AppState>,
    request: position_restrictions::ListPositionRestrictionsRequest,
) -> Result<CommandResult<Vec<position_restrictions::PositionRestriction>>, ()> {
    logged(
        "list_employee_position_restrictions",
        position_restrictions::list_restrictions(&state.pool, request)
            .await
            .into(),
    )
}
#[tauri::command]
async fn update_employee_position_restriction(
    state: tauri::State<'_, AppState>,
    request: position_restrictions::UpdatePositionRestrictionRequest,
) -> Result<CommandResult<position_restrictions::PositionRestriction>, ()> {
    logged(
        "update_employee_position_restriction",
        position_restrictions::update_restriction(&state.pool, state.clock.as_ref(), request).await,
    )
}
#[tauri::command]
async fn archive_employee_position_restriction(
    state: tauri::State<'_, AppState>,
    request: RevisionRequest,
) -> Result<CommandResult<position_restrictions::PositionRestriction>, ()> {
    logged(
        "archive_employee_position_restriction",
        position_restrictions::archive_restriction(&state.pool, state.clock.as_ref(), request)
            .await,
    )
}
#[tauri::command]
async fn set_default_coverage_template(
    state: tauri::State<'_, AppState>,
    request: coverage::SetDefaultCoverageTemplateRequest,
) -> Result<CommandResult<coverage::CoverageTemplate>, ()> {
    logged(
        "set_default_coverage_template",
        coverage::set_default_template(&state.pool, state.clock.as_ref(), request).await,
    )
}
#[tauri::command]
async fn preview_coverage_interval(
    state: tauri::State<'_, AppState>,
    request: coverage::PreviewCoverageIntervalRequest,
) -> Result<CommandResult<coverage::CoverageIntervalPreview>, ()> {
    logged(
        "preview_coverage_interval",
        coverage::preview_interval(&state.pool, request)
            .await
            .into(),
    )
}

#[tauri::command]
async fn create_availability_entry(
    state: tauri::State<'_, AppState>,
    request: availability_pto::CreateAvailabilityRequest,
) -> Result<CommandResult<availability_pto::AvailabilityEntry>, ()> {
    logged(
        "create_availability_entry",
        availability_pto::create_availability(&state.pool, state.clock.as_ref(), request)
            .await
            .into(),
    )
}

#[tauri::command]
async fn get_availability_entry(
    state: tauri::State<'_, AppState>,
    request: IdRequest,
) -> Result<CommandResult<availability_pto::AvailabilityEntry>, ()> {
    logged(
        "get_availability_entry",
        availability_pto::get_availability(&state.pool, request)
            .await
            .into(),
    )
}

#[tauri::command]
async fn list_availability_entries(
    state: tauri::State<'_, AppState>,
    request: availability_pto::ListAvailabilityRequest,
) -> Result<CommandResult<Vec<availability_pto::AvailabilityEntry>>, ()> {
    logged(
        "list_availability_entries",
        availability_pto::list_availability(&state.pool, request)
            .await
            .into(),
    )
}

#[tauri::command]
async fn update_availability_entry(
    state: tauri::State<'_, AppState>,
    request: availability_pto::UpdateAvailabilityRequest,
) -> Result<CommandResult<availability_pto::AvailabilityEntry>, ()> {
    logged(
        "update_availability_entry",
        availability_pto::update_availability(&state.pool, state.clock.as_ref(), request).await,
    )
}

#[tauri::command]
async fn archive_availability_entry(
    state: tauri::State<'_, AppState>,
    request: RevisionRequest,
) -> Result<CommandResult<availability_pto::AvailabilityEntry>, ()> {
    logged(
        "archive_availability_entry",
        availability_pto::archive_availability(&state.pool, state.clock.as_ref(), request).await,
    )
}

#[tauri::command]
async fn create_pto_entry(
    state: tauri::State<'_, AppState>,
    request: availability_pto::CreatePtoRequest,
) -> Result<CommandResult<availability_pto::PtoEntry>, ()> {
    logged(
        "create_pto_entry",
        availability_pto::create_pto(&state.pool, state.clock.as_ref(), request)
            .await
            .into(),
    )
}

#[tauri::command]
async fn get_pto_entry(
    state: tauri::State<'_, AppState>,
    request: IdRequest,
) -> Result<CommandResult<availability_pto::PtoEntry>, ()> {
    logged(
        "get_pto_entry",
        availability_pto::get_pto(&state.pool, request).await.into(),
    )
}

#[tauri::command]
async fn list_pto_entries(
    state: tauri::State<'_, AppState>,
    request: availability_pto::ListPtoRequest,
) -> Result<CommandResult<Vec<availability_pto::PtoEntry>>, ()> {
    logged(
        "list_pto_entries",
        availability_pto::list_pto(&state.pool, request)
            .await
            .into(),
    )
}

#[tauri::command]
async fn update_pto_entry(
    state: tauri::State<'_, AppState>,
    request: availability_pto::UpdatePtoRequest,
) -> Result<CommandResult<availability_pto::PtoEntry>, ()> {
    logged(
        "update_pto_entry",
        availability_pto::update_pto(&state.pool, state.clock.as_ref(), request).await,
    )
}

#[tauri::command]
async fn archive_pto_entry(
    state: tauri::State<'_, AppState>,
    request: RevisionRequest,
) -> Result<CommandResult<availability_pto::PtoEntry>, ()> {
    logged(
        "archive_pto_entry",
        availability_pto::archive_pto(&state.pool, state.clock.as_ref(), request).await,
    )
}

#[tauri::command]
async fn evaluate_availability(
    state: tauri::State<'_, AppState>,
    request: availability_rules::EvaluateAvailabilityRequest,
) -> Result<CommandResult<availability_rules::EligibilityEvaluation>, ()> {
    logged(
        "evaluate_availability",
        availability_rules::evaluate_availability(&state.pool, request)
            .await
            .into(),
    )
}

#[tauri::command]
async fn create_holiday(
    state: tauri::State<'_, AppState>,
    request: holidays::CreateHolidayRequest,
) -> Result<CommandResult<holidays::Holiday>, ()> {
    logged(
        "create_holiday",
        holidays::create_holiday(&state.pool, state.clock.as_ref(), request)
            .await
            .into(),
    )
}

#[tauri::command]
async fn get_holiday(
    state: tauri::State<'_, AppState>,
    request: IdRequest,
) -> Result<CommandResult<holidays::Holiday>, ()> {
    logged(
        "get_holiday",
        holidays::get_holiday(&state.pool, request).await.into(),
    )
}

#[tauri::command]
async fn list_holidays(
    state: tauri::State<'_, AppState>,
    request: holidays::ListHolidaysRequest,
) -> Result<CommandResult<Vec<holidays::HolidayListEntry>>, ()> {
    logged(
        "list_holidays",
        holidays::list_holidays(&state.pool, request).await.into(),
    )
}

#[tauri::command]
async fn update_holiday(
    state: tauri::State<'_, AppState>,
    request: holidays::UpdateHolidayRequest,
) -> Result<CommandResult<holidays::Holiday>, ()> {
    logged(
        "update_holiday",
        holidays::update_holiday(&state.pool, state.clock.as_ref(), request).await,
    )
}

#[tauri::command]
async fn archive_holiday(
    state: tauri::State<'_, AppState>,
    request: RevisionRequest,
) -> Result<CommandResult<holidays::Holiday>, ()> {
    logged(
        "archive_holiday",
        holidays::archive_holiday(&state.pool, state.clock.as_ref(), request).await,
    )
}

#[tauri::command]
async fn preview_holiday_date(
    _state: tauri::State<'_, AppState>,
    request: holidays::DatePreviewRequest,
) -> Result<CommandResult<holidays::DatePreview>, ()> {
    logged(
        "preview_holiday_date",
        holidays::preview_date(request).into(),
    )
}

#[tauri::command]
async fn preview_holiday_rotation(
    state: tauri::State<'_, AppState>,
    request: holidays::RotationPreviewRequest,
) -> Result<CommandResult<holidays::RotationPreview>, ()> {
    logged(
        "preview_holiday_rotation",
        holidays::preview_rotation(&state.pool, request)
            .await
            .into(),
    )
}

#[tauri::command]
async fn preview_holiday_group_match(
    state: tauri::State<'_, AppState>,
    request: holidays::GroupMatchRequest,
) -> Result<CommandResult<holidays::HolidayGroupMatch>, ()> {
    logged(
        "preview_holiday_group_match",
        holidays::preview_group_match(&state.pool, request)
            .await
            .into(),
    )
}

#[tauri::command]
async fn resolve_holidays(
    state: tauri::State<'_, AppState>,
    request: holidays::HolidayWindow,
) -> Result<CommandResult<Vec<holidays::ResolvedHoliday>>, ()> {
    logged(
        "resolve_holidays",
        holidays::read_resolved_holidays(&state.pool, request)
            .await
            .into(),
    )
}

#[tauri::command]
async fn read_holiday_collisions(
    state: tauri::State<'_, AppState>,
    request: holidays::HolidayWindow,
) -> Result<CommandResult<holidays::CollisionPreview>, ()> {
    logged(
        "read_holiday_collisions",
        holidays::read_collisions(&state.pool, request).await.into(),
    )
}

#[tauri::command]
async fn list_schedules(
    state: tauri::State<'_, AppState>,
    request: schedules::ListSchedulesRequest,
) -> Result<CommandResult<Vec<schedules::Schedule>>, ()> {
    logged(
        "list_schedules",
        schedules::list_schedules(&state.pool, request).await.into(),
    )
}
#[tauri::command]
async fn get_schedule(
    state: tauri::State<'_, AppState>,
    request: IdRequest,
) -> Result<CommandResult<schedules::Schedule>, ()> {
    logged(
        "get_schedule",
        schedules::get_schedule(&state.pool, request).await.into(),
    )
}
#[tauri::command]
async fn list_schedule_assignments(
    state: tauri::State<'_, AppState>,
    request: IdRequest,
) -> Result<CommandResult<Vec<schedule_assignments::AssignmentView>>, ()> {
    logged(
        "list_schedule_assignments",
        schedule_assignments::list_assignments(&state.pool, request)
            .await
            .into(),
    )
}
#[tauri::command]
async fn update_schedule(
    state: tauri::State<'_, AppState>,
    request: schedules::UpdateScheduleRequest,
) -> Result<CommandResult<schedules::Schedule>, ()> {
    logged(
        "update_schedule",
        schedules::update_schedule(&state.pool, state.clock.as_ref(), request).await,
    )
}
#[tauri::command]
async fn reopen_schedule(
    state: tauri::State<'_, AppState>,
    request: schedules::ReopenScheduleRequest,
) -> Result<CommandResult<schedules::Schedule>, ()> {
    logged(
        "reopen_schedule",
        schedules::reopen_schedule(&state.pool, state.clock.as_ref(), request).await,
    )
}
#[tauri::command]
async fn archive_schedule(
    state: tauri::State<'_, AppState>,
    request: RevisionRequest,
) -> Result<CommandResult<schedules::Schedule>, ()> {
    logged(
        "archive_schedule",
        schedules::archive_schedule(&state.pool, state.clock.as_ref(), request).await,
    )
}
#[tauri::command]
async fn delete_schedule(
    state: tauri::State<'_, AppState>,
    request: schedules::DeleteScheduleRequest,
) -> Result<CommandResult<schedules::ScheduleDeletion>, ()> {
    logged(
        "delete_schedule",
        schedules::delete_schedule(&state.pool, state.clock.as_ref(), request).await,
    )
}
#[tauri::command]
async fn get_current_schedule(
    state: tauri::State<'_, AppState>,
) -> Result<CommandResult<schedules::CurrentSchedule>, ()> {
    logged(
        "get_current_schedule",
        schedules::get_current(&state.pool).await.into(),
    )
}
#[tauri::command]
async fn set_current_schedule(
    state: tauri::State<'_, AppState>,
    request: schedules::SetCurrentScheduleRequest,
) -> Result<CommandResult<schedules::CurrentSchedule>, ()> {
    logged(
        "set_current_schedule",
        schedules::set_current(&state.pool, state.clock.as_ref(), request).await,
    )
}
#[tauri::command]
async fn clear_current_schedule(
    state: tauri::State<'_, AppState>,
    request: schedules::ClearCurrentScheduleRequest,
) -> Result<CommandResult<schedules::CurrentSchedule>, ()> {
    logged(
        "clear_current_schedule",
        schedules::clear_current(&state.pool, state.clock.as_ref(), request).await,
    )
}

#[tauri::command]
async fn generate_schedule(
    state: tauri::State<'_, AppState>,
    request: generation::GenerateScheduleRequest,
) -> Result<CommandResult<generation::GenerationResult>, ()> {
    logged(
        "generate_schedule",
        generation::generate_schedule(&state.pool, state.clock.as_ref(), request)
            .await
            .into(),
    )
}
#[tauri::command]
async fn get_generation_readiness(
    state: tauri::State<'_, AppState>,
    request: generation::GenerationReadinessRequest,
) -> Result<CommandResult<crate::generation_inputs::GenerationReadiness>, ()> {
    logged(
        "get_generation_readiness",
        generation::get_generation_readiness(&state.pool, request)
            .await
            .into(),
    )
}

#[tauri::command]
async fn get_assignment_candidates(
    state: tauri::State<'_, AppState>,
    request: crate::schedule_editor::CandidatesRequest,
) -> Result<CommandResult<crate::schedule_editor::Candidates>, ()> {
    logged(
        "get_assignment_candidates",
        crate::schedule_editor::candidates(&state.pool, request)
            .await
            .into(),
    )
}
#[tauri::command]
async fn get_editor_history(
    state: tauri::State<'_, AppState>,
    request: crate::schedule_editor::HistoryRequest,
) -> Result<CommandResult<crate::editor_history::HistoryState>, ()> {
    logged(
        "get_editor_history",
        crate::schedule_editor::history(&state.pool, request)
            .await
            .into(),
    )
}
#[tauri::command]
async fn edit_schedule_assignment(
    state: tauri::State<'_, AppState>,
    request: crate::schedule_editor::ActionRequest,
) -> Result<CommandResult<crate::schedule_editor::EditorOutcome>, ()> {
    logged(
        "edit_schedule_assignment",
        crate::schedule_editor::edit(&state.pool, state.clock.as_ref(), request).await,
    )
}
#[tauri::command]
async fn replay_schedule_assignment(
    state: tauri::State<'_, AppState>,
    request: crate::schedule_editor::ReplayRequest,
) -> Result<CommandResult<crate::schedule_editor::EditorOutcome>, ()> {
    logged(
        "replay_schedule_assignment",
        crate::schedule_editor::replay(&state.pool, state.clock.as_ref(), request).await,
    )
}

#[tauri::command]
async fn validate_schedule(
    state: tauri::State<'_, AppState>,
    request: IdRequest,
) -> Result<CommandResult<crate::validation::ValidationResult>, ()> {
    logged(
        "validate_schedule",
        crate::validation::validate_schedule(&state.paths, &request.id)
            .await
            .into(),
    )
}

#[tauri::command]
async fn finalize_schedule(
    state: tauri::State<'_, AppState>,
    request: schedules::FinalizeScheduleRequest,
) -> Result<CommandResult<schedules::FinalizationOutcome>, ()> {
    logged(
        "finalize_schedule",
        schedules::finalize_schedule(&state.pool, state.clock.as_ref(), request).await,
    )
}
#[tauri::command]
async fn get_schedule_dashboard(
    state: tauri::State<'_, AppState>,
) -> Result<CommandResult<schedules::ScheduleDashboard>, ()> {
    logged(
        "get_schedule_dashboard",
        schedules::dashboard(&state.pool).await.into(),
    )
}

pub fn register(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder.invoke_handler(tauri::generate_handler![
        get_startup_status,
        save_preservation_backup,
        build_print_dto,
        export_schedule_xlsx,
        export_schedule_pdf,
        get_backup_screen,
        create_manual_backup,
        stage_database_restore,
        apply_database_restore,
        validate_schedule,
        finalize_schedule,
        get_schedule_dashboard,
        get_assignment_candidates,
        get_editor_history,
        edit_schedule_assignment,
        replay_schedule_assignment,
        generate_schedule,
        get_generation_readiness,
        list_schedules,
        get_schedule,
        list_schedule_assignments,
        update_schedule,
        reopen_schedule,
        archive_schedule,
        delete_schedule,
        get_current_schedule,
        set_current_schedule,
        clear_current_schedule,
        create_holiday,
        get_holiday,
        list_holidays,
        update_holiday,
        archive_holiday,
        preview_holiday_date,
        preview_holiday_rotation,
        preview_holiday_group_match,
        resolve_holidays,
        read_holiday_collisions,
        create_availability_entry,
        get_availability_entry,
        list_availability_entries,
        update_availability_entry,
        archive_availability_entry,
        create_pto_entry,
        get_pto_entry,
        list_pto_entries,
        update_pto_entry,
        archive_pto_entry,
        evaluate_availability,
        get_settings,
        update_manager_setting,
        get_database_health,
        get_employment_defaults,
        create_employee,
        get_employee,
        list_employees,
        update_employee,
        archive_employee,
        preview_employment_defaults,
        reset_employment_defaults,
        create_employee_restriction,
        list_employee_restrictions,
        update_employee_restriction,
        archive_employee_restriction,
        preview_shift_time,
        create_shift_template,
        get_shift_template,
        list_shift_templates,
        update_shift_template,
        archive_shift_template,
        create_employee_shift_template_restriction,
        get_employee_shift_template_restriction,
        list_employee_shift_template_restrictions,
        update_employee_shift_template_restriction,
        archive_employee_shift_template_restriction,
        create_shift_position,
        get_shift_position,
        list_shift_positions,
        update_shift_position,
        archive_shift_position,
        create_coverage_template,
        get_coverage_template,
        list_coverage_templates,
        update_coverage_template,
        archive_coverage_template,
        create_coverage_item,
        get_coverage_item,
        list_coverage_items,
        update_coverage_item,
        archive_coverage_item,
        create_employee_position_restriction,
        get_employee_position_restriction,
        list_employee_position_restrictions,
        update_employee_position_restriction,
        archive_employee_position_restriction,
        set_default_coverage_template,
        preview_coverage_interval,
    ])
}
