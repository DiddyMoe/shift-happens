//! Explicit local diagnostic: one real generation call, no fixtures or assertions.
use crate::{
    database::{DatabasePaths, inspect_migration_state, open_normal_pool},
    error::{AppError, AppResult, CommandResult, ErrorCode},
    generation::{GenerateScheduleRequest, generate_schedule},
    metadata::SystemMetadataClock,
};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Serialize)]
struct Summary {
    schedule_id: String,
    assignment_count: usize,
    assigned_count: i64,
    open_count: i64,
    total_warning_count: i64,
    warning_counts: std::collections::BTreeMap<crate::generation::GenerationCode, i64>,
    details_truncated: bool,
    retained_warning_ids: Vec<String>,
    diagnostics: crate::generation::GenerationDiagnostics,
}
async fn run(
    database: String,
    request: String,
    initialize_development: bool,
) -> AppResult<Summary> {
    let request: GenerateScheduleRequest = serde_json::from_str(&request).map_err(|_| {
        AppError::new(
            ErrorCode::InvalidSchedule,
            "Supply one complete GenerateScheduleRequest JSON argument.",
        )
    })?;
    let main = PathBuf::from(database);
    if !main.is_absolute() {
        return Err(AppError::new(
            ErrorCode::InvalidText,
            "Specify an absolute path to an existing development database.",
        ));
    }
    let directory = main
        .parent()
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::InvalidText,
                "The database needs a parent directory.",
            )
        })?
        .to_path_buf();
    let paths = DatabasePaths { directory, main };
    let pool = if initialize_development {
        if paths.directory.file_name().and_then(|n| n.to_str()) != Some("development") {
            return Err(AppError::new(
                ErrorCode::InvalidText,
                "Diagnostic initialization requires an explicit disposable development directory.",
            ));
        }
        // Existing startup owns create-new claiming, migrations, WAL and normal
        // application seeds, and rejects existing databases needing an upgrade.
        crate::database::start_development_database(paths, std::sync::Arc::new(SystemMetadataClock))
            .await?
            .pool
    } else {
        if !inspect_migration_state(&paths).await?.current {
            return Err(AppError::new(
                ErrorCode::MigrationInvalid,
                "The diagnostic does not upgrade existing databases.",
            ));
        }
        open_normal_pool(&paths).await?
    };
    let generated = generate_schedule(&pool, &SystemMetadataClock, request).await;
    // Closing a pool cannot convert a committed generation into a retryable error.
    pool.close().await;
    let result = generated?;
    Ok(Summary {
        schedule_id: result.schedule.id,
        assignment_count: result.assignment_count,
        assigned_count: result.assigned_count,
        open_count: result.open_count,
        total_warning_count: result.total_warning_count,
        warning_counts: result.warning_counts,
        details_truncated: result.details_truncated,
        retained_warning_ids: result.warnings.into_iter().map(|w| w.warning_id).collect(),
        diagnostics: result.diagnostics,
    })
}
pub fn execute(mut args: impl Iterator<Item = String>) {
    let first = args.next();
    let initialize_development = first.as_deref() == Some("--initialize-development");
    let database = if initialize_development {
        args.next()
    } else {
        first
    };
    let arguments = database.zip(args.next());
    let result = match arguments {
        Some((database, request)) if args.next().is_none() => {
            tauri::async_runtime::block_on(run(database, request, initialize_development))
        }
        _ => Err(AppError::new(
            ErrorCode::InvalidSchedule,
            "Usage: shift-happens --generation-diagnostic [--initialize-development] ABSOLUTE_DATABASE_PATH REQUEST_JSON. This creates a complete draft in that database.",
        )),
    };
    let failed = result.is_err();
    // Summary only: never print names, notes, assignment contents, or PTO details.
    match serde_json::to_string(&CommandResult::from(result)) {
        Ok(output) => println!("{output}"),
        Err(_) => {
            eprintln!("The diagnostic response could not be formatted.");
            std::process::exit(1);
        }
    }
    if failed {
        std::process::exit(1);
    }
}
