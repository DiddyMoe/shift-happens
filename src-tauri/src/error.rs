pub const DIAGNOSTIC_SOURCE_ID_LIMIT: usize = 32;
pub const DIAGNOSTIC_SOURCE_ID_BYTES: usize = 128;
pub const DIAGNOSTIC_LIMIT: usize = 1_000;

use serde::{Serialize, Serializer};

/// Shared availability findings keep the same wire identity across availability,
/// generation, manual editing, and validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AvailabilityRuleCode {
    ApprovedPtoConflict,
    AvailabilityHardConflict,
    ExplicitAvailabilityMissing,
    RequestedPtoOverlap,
    PreferredAvailabilityNotMet,
}

impl AvailabilityRuleCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ApprovedPtoConflict => "approved_pto_conflict",
            Self::AvailabilityHardConflict => "availability_hard_conflict",
            Self::ExplicitAvailabilityMissing => "explicit_availability_missing",
            Self::RequestedPtoOverlap => "requested_pto_overlap",
            Self::PreferredAvailabilityNotMet => "preferred_availability_not_met",
        }
    }
}

impl Serialize for AvailabilityRuleCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidDate,
    InvalidTime,
    InvalidDatetime,
    InvalidInterval,
    InvalidText,
    OutOfRange,
    InvalidMetadata,
    InvalidSetting,
    ConfigurationInvalid,
    SettingNotEditable,
    StaleRevision,
    DatabaseUnavailable,
    DatabaseIntegrityError,
    DatabaseBusy,
    DiskFull,
    DataFolderUnwritable,
    ChangeNotAllowed,
    StartupClassificationChanged,
    UnsafeSqliteEngine,
    MigrationInvalid,
    UpgradeRequiresQualifiedStartup,
    WalRequired,
    PostSchemaInitializationFailed,
    ReleaseStartupNotQualified,
    InvalidEmployee,
    InvalidSchedule,
    InvalidAssignment,
    InvalidGenerationSnapshot,
    ScheduleReadOnly,
    ReopenConfirmationRequired,
    InvalidShiftTemplate,
    InvalidColor,
    InvalidCoverageItem,
    InvalidCoverageOffset,
    InvalidRequiredCount,
    InvalidCoverageDefault,
    InvalidHolidayRule,
    InvalidHolidayCoverage,
    HolidayCollision,
    DuplicateCoverageItem,
    WeekdayDefaultRequired,
    InvalidRestriction,
    InvalidRevision,
    NotFound,
    RecordArchived,
    DuplicateRestriction,
    ActiveDependencies,
    InputLimitExceeded,
    GenerationInputLimitExceeded,
    PlannedSlotLimitExceeded,
    TwoWeekAnchorRequired,
    CoverageMissing,
    CoverageAmbiguous,
    CoverageEmpty,
    CoverageSourceInvalid,
    DuplicatePlannedSlot,
    CandidateInputLimitExceeded,
    EditorOutputLimitExceeded,
    AssignmentLocked,
    EmployeeIneligible,
    InvalidAcknowledgement,
    FinalizationBlocked,
    HistoryStale,
    HistoryUnavailable,
    InvalidDestination,
    InvalidDestinationExtension,
    ProtectedDatabaseDestination,
    DestinationStateChanged,
    OverwriteAuthorizationInvalid,
    OverwriteUnavailable,
    TemporaryNameExhausted,
    FileWriteFailed,
    FileSynchronizationFailed,
    FilePublicationFailed,
    PublicationRecoveryRequired,
    FileCleanupFailed,
    BackupSourceInvalid,
    BackupDestinationInUse,
    BackupCopyFailed,
    BackupCopyLimitExceeded,
    BackupValidationFailed,
    BackupForeignKeyViolation,
    ForeignKeyViolationsPreserved,
    BackupMetadataUnavailable,
    BackupInProgress,
    ExportTooLargeForFormat,
    ExportTextBudgetExceeded,
    XlsxTextLimitExceeded,
    PdfTextNotRenderable,
    ExportStructureInvalid,
    ExportValidationFailed,
    ExportGenerationFailed,
    FinalizedScheduleRetained,
    RestoreSourceInvalid,
    RestoreConfirmationExpired,
    RestoreFailed,
}

impl ErrorCode {
    /// A failed SQLite operation. Each of these was reported as
    /// `DatabaseUnavailable` before SQLite causes were distinguished, so startup
    /// still classifies all of them as a database access error.
    pub fn is_database_operation_failure(self) -> bool {
        matches!(
            self,
            Self::DatabaseUnavailable
                | Self::DatabaseBusy
                | Self::DiskFull
                | Self::DataFolderUnwritable
                | Self::ChangeNotAllowed
        )
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ErrorContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_revision: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<ErrorCode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependency_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependency_names: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependency_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omitted_source_id_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publication_state: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_path: Option<String>,
    /// SQLite's extended result code, when SQLite reported the failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sqlite_code: Option<i32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cleanup_warnings: Vec<AppError>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    pub code: ErrorCode,
    pub message: &'static str,
    pub context: Box<ErrorContext>,
}

impl AppError {
    pub fn new(code: ErrorCode, message: &'static str) -> Self {
        Self {
            code,
            message,
            context: Box::default(),
        }
    }

    pub fn setting(mut self, key: &str) -> Self {
        self.context.key = Some(key.to_owned());
        self
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for AppError {}

/// SQLite failures map to a cause the manager can act on. The message stays
/// static; the extended result code travels in `context.sqlite_code` and the
/// diagnostic log.
impl From<sqlx::Error> for AppError {
    fn from(error: sqlx::Error) -> Self {
        use libsqlite3_sys as ffi;
        let (sqlite_code, sqlite_message) = match &error {
            sqlx::Error::Database(database) => (
                database.code().and_then(|code| code.parse::<i32>().ok()),
                Some(database.message()),
            ),
            _ => (None, None),
        };
        // Extended result codes keep their primary code in the low byte.
        let (code, message) = match (&error, sqlite_code.map(|code| code & 0xff)) {
            (sqlx::Error::PoolTimedOut, _) | (_, Some(ffi::SQLITE_BUSY | ffi::SQLITE_LOCKED)) => (
                ErrorCode::DatabaseBusy,
                "Shift Happens is busy finishing another change. Try again in a moment.",
            ),
            (_, Some(ffi::SQLITE_FULL)) => (
                ErrorCode::DiskFull,
                "The disk is full. Free some space, then try again.",
            ),
            (
                _,
                Some(
                    ffi::SQLITE_READONLY
                    | ffi::SQLITE_CANTOPEN
                    | ffi::SQLITE_PERM
                    | ffi::SQLITE_IOERR,
                ),
            ) => (
                ErrorCode::DataFolderUnwritable,
                "Shift Happens can't write to its data folder. Check the disk and folder permissions, then restart the app.",
            ),
            // Triggers (RAISE) and table constraints guard records the services
            // already validated; reaching one means this record cannot change.
            (_, Some(ffi::SQLITE_CONSTRAINT)) => (
                ErrorCode::ChangeNotAllowed,
                "That change isn't allowed for this record.",
            ),
            // Same code as before, so startup classification is unchanged.
            (_, Some(ffi::SQLITE_CORRUPT | ffi::SQLITE_NOTADB)) => (
                ErrorCode::DatabaseUnavailable,
                "SQLite could not read a valid database. Close the app and preserve its files for developer inspection.",
            ),
            _ => (
                ErrorCode::DatabaseUnavailable,
                "The local database operation could not complete.",
            ),
        };
        let mut mapped = Self::new(code, message);
        mapped.context.sqlite_code = sqlite_code;
        crate::diagnostics::sqlite_error(&error, sqlite_code, sqlite_message, code);
        mapped
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CommandResult<T> {
    Success {
        data: T,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        warnings: Vec<AppError>,
    },
    NoChange {
        data: T,
    },
    RequiresConfirmation {
        data: T,
    },
    RequiresOverwrite {
        destination_path: String,
        overwrite_token: String,
    },
    Canceled,
    ValidationError {
        error: AppError,
    },
    StaleRevision {
        error: AppError,
    },
    Blocked {
        error: AppError,
    },
    Failure {
        error: AppError,
    },
}

impl<T> From<AppResult<T>> for CommandResult<T> {
    fn from(result: AppResult<T>) -> Self {
        match result {
            Ok(data) => Self::Success {
                data,
                warnings: Vec::new(),
            },
            Err(error) => match error.code {
                ErrorCode::InvalidSetting
                | ErrorCode::SettingNotEditable
                | ErrorCode::InvalidDate
                | ErrorCode::InvalidTime
                | ErrorCode::InvalidDatetime
                | ErrorCode::InvalidInterval
                | ErrorCode::InvalidText
                | ErrorCode::OutOfRange
                | ErrorCode::InvalidSchedule
                | ErrorCode::InvalidAssignment
                | ErrorCode::InvalidGenerationSnapshot
                | ErrorCode::ReopenConfirmationRequired
                | ErrorCode::InvalidEmployee
                | ErrorCode::InvalidShiftTemplate
                | ErrorCode::InvalidCoverageItem
                | ErrorCode::InvalidCoverageOffset
                | ErrorCode::InvalidRequiredCount
                | ErrorCode::InvalidCoverageDefault
                | ErrorCode::InvalidHolidayRule
                | ErrorCode::InvalidHolidayCoverage
                | ErrorCode::DuplicateCoverageItem
                | ErrorCode::InvalidColor
                | ErrorCode::InvalidRestriction
                | ErrorCode::InvalidRevision
                | ErrorCode::InvalidAcknowledgement
                | ErrorCode::DuplicateRestriction
                | ErrorCode::NotFound => Self::ValidationError { error },
                ErrorCode::StaleRevision => Self::StaleRevision { error },
                ErrorCode::ConfigurationInvalid
                | ErrorCode::UnsafeSqliteEngine
                | ErrorCode::WalRequired
                | ErrorCode::ScheduleReadOnly
                | ErrorCode::RecordArchived
                | ErrorCode::WeekdayDefaultRequired
                | ErrorCode::ActiveDependencies
                | ErrorCode::HolidayCollision
                | ErrorCode::GenerationInputLimitExceeded
                | ErrorCode::PlannedSlotLimitExceeded
                | ErrorCode::TwoWeekAnchorRequired
                | ErrorCode::CoverageMissing
                | ErrorCode::CoverageAmbiguous
                | ErrorCode::CoverageEmpty
                | ErrorCode::CoverageSourceInvalid
                | ErrorCode::DuplicatePlannedSlot
                | ErrorCode::InputLimitExceeded => Self::Blocked { error },
                ErrorCode::CandidateInputLimitExceeded
                | ErrorCode::EditorOutputLimitExceeded
                | ErrorCode::AssignmentLocked
                | ErrorCode::EmployeeIneligible
                | ErrorCode::FinalizationBlocked
                | ErrorCode::HistoryStale
                | ErrorCode::HistoryUnavailable
                | ErrorCode::FinalizedScheduleRetained => Self::Blocked { error },
                _ => Self::Failure { error },
            },
        }
    }
}
