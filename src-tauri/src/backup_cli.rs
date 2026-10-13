//! Local backup CLI, using the same trusted-source orchestration as the UI.
use crate::{
    backup::{BackupRuntime, ClassifiedStartupBackupSource, ManualBackupRequest, manual_backup},
    database::{
        DatabasePaths, inspect_database_file_set, inspect_migration_state, open_main,
        open_normal_pool,
    },
    error::{AppError, AppResult, CommandResult, ErrorCode},
    metadata::SystemMetadataClock,
};
use sqlx::Connection;
use std::{
    io::{self, Write},
    path::PathBuf,
    sync::Arc,
};

async fn run(source: String, destination: String) -> AppResult<()> {
    let main = PathBuf::from(source);
    if !main.is_absolute() {
        return Err(AppError::new(
            ErrorCode::BackupSourceInvalid,
            "Supply the absolute path to an existing application database.",
        ));
    }
    let paths = DatabasePaths {
        directory: main
            .parent()
            .ok_or_else(|| {
                AppError::new(
                    ErrorCode::BackupSourceInvalid,
                    "The source needs an existing parent folder.",
                )
            })?
            .to_owned(),
        main,
    };
    let file_set = inspect_database_file_set(&paths)?;
    let clock = Arc::new(SystemMetadataClock);
    let classified =
        ClassifiedStartupBackupSource::from_classified(&paths, &file_set, clock.clone())?;
    // History is optional. A current-schema rollback-journal database is also
    // a valid preservation source; backup must not require a WAL transition.
    let pool = if inspect_migration_state(&paths).await?.current {
        let mut reader = open_main(&paths, true).await?;
        let journal_mode = sqlx::query_scalar::<_, String>("PRAGMA journal_mode")
            .fetch_one(&mut reader)
            .await;
        reader.close().await?;
        if journal_mode? == "wal" {
            Some(open_normal_pool(&paths).await?)
        } else {
            None
        }
    } else {
        None
    };
    let runtime = BackupRuntime::default();
    let first = manual_backup(
        classified.path.clone(),
        pool.as_ref(),
        clock.clone(),
        &runtime,
        ManualBackupRequest {
            destination_path: Some(destination),
            overwrite_token: None,
        },
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
                .map_err(|_| AppError::new(
                    ErrorCode::InvalidDestination,
                    "The confirmation could not be formatted."
                ))?
            );
            eprintln!(
                "Type REPLACE to replace exactly this observed file; any other response cancels."
            );
            io::stdout().flush().map_err(|_| {
                AppError::new(
                    ErrorCode::FileWriteFailed,
                    "The confirmation could not be displayed.",
                )
            })?;
            let answer = tauri::async_runtime::spawn_blocking(|| {
                let mut answer = String::new();
                io::stdin()
                    .read_line(&mut answer)
                    .map(|_| answer.trim() == "REPLACE")
            })
            .await;
            if matches!(answer, Ok(Ok(true))) {
                manual_backup(
                    classified.path,
                    pool.as_ref(),
                    clock,
                    &runtime,
                    ManualBackupRequest {
                        destination_path: Some(destination_path),
                        overwrite_token: Some(overwrite_token),
                    },
                )
                .await
            } else {
                CommandResult::Canceled
            }
        }
        result => result,
    };
    if let Some(pool) = pool {
        pool.close().await;
    }
    let failed = matches!(
        result,
        CommandResult::Failure { .. }
            | CommandResult::Blocked { .. }
            | CommandResult::ValidationError { .. }
    );
    println!(
        "{}",
        serde_json::to_string(&result).map_err(|_| AppError::new(
            ErrorCode::FileWriteFailed,
            "The backup result could not be formatted. Inspect the destination before retrying."
        ))?
    );
    if failed {
        std::process::exit(1);
    }
    Ok(())
}
pub fn execute(mut args: impl Iterator<Item = String>) {
    let result = match args.next().zip(args.next()) {
        Some((source, destination)) if args.next().is_none() => {
            tauri::async_runtime::block_on(run(source, destination))
        }
        _ => Err(AppError::new(
            ErrorCode::InvalidDestination,
            "Usage: shift-happens --backup-database ABSOLUTE_DATABASE_PATH ABSOLUTE_DESTINATION_PATH. Existing destinations require interactive confirmation.",
        )),
    };
    if let Err(error) = result {
        if let Ok(output) = serde_json::to_string(&CommandResult::<()>::Failure { error }) {
            eprintln!("{output}");
        }
        std::process::exit(1);
    }
}
