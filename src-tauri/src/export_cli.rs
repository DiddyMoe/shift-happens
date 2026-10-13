//! Narrow local export CLI calling the same services as the manager screen.
//! Reports only file metadata/diagnostics; never dumps manager-entered text.
use crate::{
    database::{DatabasePaths, inspect_migration_state},
    error::{AppError, AppResult, CommandResult, ErrorCode},
    export_snapshot::ExportFormat,
    exports::{self, ExportRequest},
    metadata::SystemMetadataClock,
    safe_destination::SafeDestinationService,
};
use std::{io, path::PathBuf, sync::Arc};

async fn run(
    database: String,
    schedule_id: String,
    format: String,
    destination: String,
) -> AppResult<()> {
    let main = PathBuf::from(database);
    if !main.is_absolute() {
        return Err(AppError::new(
            ErrorCode::InvalidDestination,
            "Supply an absolute existing application database path.",
        ));
    }
    let paths = DatabasePaths {
        directory: main
            .parent()
            .ok_or_else(crate::export_snapshot::structural)?
            .to_owned(),
        main,
    };
    if !inspect_migration_state(&paths).await?.current {
        return Err(AppError::new(
            ErrorCode::MigrationInvalid,
            "Exports require a current application database. The export CLI never initializes or migrates it.",
        ));
    }
    let format = match format.as_str() {
        "xlsx" => ExportFormat::Xlsx,
        "pdf" => ExportFormat::Pdf,
        _ => {
            return Err(AppError::new(
                ErrorCode::InvalidDestination,
                "Choose xlsx or pdf.",
            ));
        }
    };
    let destinations = Arc::new(SafeDestinationService::default());
    let clock = Arc::new(SystemMetadataClock);
    let first = exports::export(
        &paths,
        clock.clone(),
        destinations.clone(),
        ExportRequest {
            schedule_id: schedule_id.clone(),
            destination_path: Some(destination),
            overwrite_token: None,
        },
        format,
    )
    .await;
    let result = match first {
        CommandResult::RequiresOverwrite {
            destination_path,
            overwrite_token,
        } => {
            println!(
                "{}",
                serde_json::to_string(&CommandResult::<()>::RequiresOverwrite {
                    destination_path: destination_path.clone(),
                    overwrite_token: overwrite_token.clone()
                })
                .map_err(|_| crate::export_xlsx::generation_error())?
            );
            eprintln!(
                "Type REPLACE to replace exactly this observed file; any other response cancels."
            );
            let answer = tauri::async_runtime::spawn_blocking(|| {
                let mut answer = String::new();
                io::stdin()
                    .read_line(&mut answer)
                    .map(|_| answer.trim() == "REPLACE")
            })
            .await;
            if matches!(answer, Ok(Ok(true))) {
                exports::export(
                    &paths,
                    clock,
                    destinations,
                    ExportRequest {
                        schedule_id,
                        destination_path: Some(destination_path),
                        overwrite_token: Some(overwrite_token),
                    },
                    format,
                )
                .await
            } else {
                CommandResult::Canceled
            }
        }
        result => result,
    };
    let failed = matches!(result, CommandResult::Failure { .. });
    println!(
        "{}",
        serde_json::to_string(&result).map_err(|_| crate::export_xlsx::generation_error())?
    );
    if failed {
        std::process::exit(1);
    }
    Ok(())
}
pub fn execute(mut args: impl Iterator<Item = String>) {
    let arguments = args
        .next()
        .zip(args.next())
        .zip(args.next())
        .zip(args.next());
    let result = match arguments {
        Some((((database, schedule), format), destination)) if args.next().is_none() => {
            tauri::async_runtime::block_on(run(database, schedule, format, destination))
        }
        _ => Err(AppError::new(
            ErrorCode::InvalidDestination,
            "Usage: shift-happens --export-schedule ABSOLUTE_DATABASE_PATH SCHEDULE_ID xlsx|pdf ABSOLUTE_DESTINATION_PATH. Existing files require interactive confirmation.",
        )),
    };
    if let Err(error) = result {
        if let Ok(output) = serde_json::to_string(&CommandResult::<()>::Failure { error }) {
            eprintln!("{output}");
        }
        std::process::exit(1);
    }
}
