use crate::{
    database::DatabasePaths,
    error::{AppError, CommandResult, ErrorCode},
    export_pdf,
    export_snapshot::{self, ExportDiagnostic, ExportFormat, ExportSnapshot},
    export_xlsx,
    metadata::MetadataClock,
    safe_destination::{self, DestinationPolicy, PreparedResult, SafeDestinationService},
};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrintRequest {
    pub schedule_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportRequest {
    pub schedule_id: String,
    pub destination_path: Option<String>,
    pub overwrite_token: Option<String>,
}
#[derive(Serialize)]
pub struct ExportFile {
    pub format: ExportFormat,
    pub destination_path: String,
    pub file_size_bytes: u64,
    pub schedule_id: String,
    pub captured_revision: i64,
    pub exported_at_utc: String,
    pub assignment_count: usize,
    pub diagnostics: Vec<ExportDiagnostic>,
    pub omitted_diagnostic_count: usize,
}
pub async fn print_snapshot(
    paths: &DatabasePaths,
    clock: &dyn MetadataClock,
    request: PrintRequest,
) -> CommandResult<ExportSnapshot> {
    match export_snapshot::read_snapshot(paths, &request.schedule_id, ExportFormat::Print, clock)
        .await
    {
        Ok(data) => CommandResult::Success {
            data,
            warnings: Vec::new(),
        },
        Err(error) => CommandResult::Failure { error },
    }
}
pub async fn export(
    paths: &DatabasePaths,
    clock: Arc<dyn MetadataClock>,
    destinations: Arc<SafeDestinationService>,
    request: ExportRequest,
    format: ExportFormat,
) -> CommandResult<ExportFile> {
    let Some(destination) = request.destination_path else {
        return CommandResult::Canceled;
    };
    let snapshot =
        match export_snapshot::read_snapshot(paths, &request.schedule_id, format, clock.as_ref())
            .await
        {
            Ok(snapshot) => snapshot,
            Err(error) => return CommandResult::Failure { error },
        };
    // read_snapshot has already rolled back AND closed the dedicated connection.
    // This closure owns no pool, connection, transaction, SQL row or database borrow.
    let source = paths.main.clone();
    tauri::async_runtime::spawn_blocking(move || {
        generate(
            snapshot,
            source,
            destination,
            request.overwrite_token,
            destinations,
            format,
        )
    })
    .await
    .unwrap_or_else(|_| CommandResult::Failure {
        error: export_xlsx::generation_error(),
    })
}
fn generate(
    snapshot: ExportSnapshot,
    source: PathBuf,
    destination: String,
    token: Option<String>,
    destinations: Arc<SafeDestinationService>,
    format: ExportFormat,
) -> CommandResult<ExportFile> {
    let prepared = match destinations.prepare(
        &source,
        &PathBuf::from(destination),
        format.name(),
        token.as_deref(),
        DestinationPolicy::Manual,
    ) {
        Ok(PreparedResult::Ready(value)) => value,
        Ok(PreparedResult::RequiresOverwrite {
            destination_path,
            overwrite_token,
        }) => {
            return CommandResult::RequiresOverwrite {
                destination_path,
                overwrite_token,
            };
        }
        Err(error) => return CommandResult::Failure { error },
    };
    let font = match format {
        ExportFormat::Pdf => match export_pdf::preflight(&snapshot) {
            Ok(font) => Some(font),
            Err(error) => return CommandResult::Failure { error },
        },
        ExportFormat::Xlsx => {
            if let Err(error) = export_xlsx::preflight(&snapshot) {
                return CommandResult::Failure { error };
            }
            None
        }
        ExportFormat::Print => {
            return CommandResult::Failure {
                error: AppError::new(
                    ErrorCode::InvalidDestination,
                    "Select XLSX or PDF for file export.",
                ),
            };
        }
    };
    let layout = font
        .as_ref()
        .map(|font| export_pdf::layout(&snapshot, font));
    let mut temporary = match destinations.claim_temporary(&prepared, false) {
        Ok(temporary) => temporary,
        Err(error) => return CommandResult::Failure { error },
    };
    let result: crate::error::AppResult<_> = (|| {
        if let (Some(font), Some(layout)) = (&font, &layout) {
            export_pdf::write(font, layout, &temporary)?;
        } else {
            export_xlsx::write(&snapshot, &temporary)?;
        }
        let size = temporary.synchronize()?;
        if let Some(layout) = &layout {
            export_pdf::validate(layout, &temporary)?;
        } else {
            export_xlsx::validate(&snapshot, &temporary)?;
        }
        // Every writer, synchronizer and independent parser has been released.
        // Shared service rechecks exact observed state and performs publication.
        let warnings = destinations.publish(&prepared, &mut temporary, false)?;
        Ok((size, warnings))
    })();
    match result {
        Ok((file_size_bytes, warnings)) => CommandResult::Success {
            data: ExportFile {
                format,
                destination_path: safe_destination::display_path(prepared.final_path()),
                file_size_bytes,
                schedule_id: snapshot.schedule_id,
                captured_revision: snapshot.revision,
                exported_at_utc: snapshot.exported_at_utc,
                assignment_count: snapshot.rows.len(),
                diagnostics: snapshot.diagnostics,
                omitted_diagnostic_count: snapshot.omitted_diagnostic_count,
            },
            warnings,
        },
        Err(mut error) => {
            error.context.cleanup_warnings.extend(temporary.cleanup());
            CommandResult::Failure { error }
        }
    }
}
