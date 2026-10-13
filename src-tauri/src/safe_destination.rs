//! Shared, invocation-owned file output for SQLite, XLSX/PDF and pre-migration backup.
//! Windows replacement uses a same-volume MoveFileExW and an invocation-owned hard
//! link to the original. Failure is classified by observed identities; an ambiguous
//! result retains the original link and blocks retry. No crash-atomicity claim.
use crate::error::{AppError, AppResult, ErrorCode};
use std::{
    collections::{HashMap, VecDeque},
    fs::{self, File, Metadata, OpenOptions},
    io,
    path::{Component, Path, PathBuf},
    sync::Mutex,
    time::SystemTime,
};
use uuid::Uuid;

pub const TEMPORARY_NAME_ATTEMPTS: usize = 32;
const MAX_OVERWRITE_TOKENS: usize = 32;

#[derive(Clone, Debug)]
pub enum DestinationPolicy {
    Manual,
    AppOwnedNoOverwrite { directory: PathBuf },
}
impl DestinationPolicy {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::AppOwnedNoOverwrite { .. } => "app_owned_no_overwrite",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileIdentity(u64, u64);
#[derive(Clone, Debug, PartialEq, Eq)]
struct FileState {
    identity: FileIdentity,
    length: u64,
    modified: SystemTime,
    created: Option<SystemTime>,
    readonly: bool,
    change: (i64, i64),
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Observation {
    parent: PathBuf,
    parent_identity: FileIdentity,
    final_path: PathBuf,
    state: Option<FileState>,
}
struct Confirmation {
    token: String,
    observation: Observation,
}
#[derive(Default)]
pub struct SafeDestinationService {
    publication: Mutex<()>,
    tokens: Mutex<VecDeque<Confirmation>>,
    recovery: Mutex<HashMap<PathBuf, PathBuf>>,
}

pub enum PreparedResult {
    Ready(PreparedDestination),
    RequiresOverwrite {
        destination_path: String,
        overwrite_token: String,
    },
}
pub struct PreparedDestination {
    requested: PathBuf,
    observed: Observation,
    source: PathBuf,
    policy: DestinationPolicy,
}
impl PreparedDestination {
    pub fn final_path(&self) -> &Path {
        &self.observed.final_path
    }
}

/// Show ordinary selected paths; canonical OS prefixes remain internal.
pub fn display_path(path: &Path) -> String {
    let value = path.to_string_lossy().into_owned();
    #[cfg(windows)]
    {
        if let Some(unc) = value.strip_prefix(r"\\?\UNC\") {
            return format!(r"\\{unc}");
        }
        if let Some(local) = value.strip_prefix(r"\\?\") {
            return local.to_owned();
        }
    }
    value
}

pub fn file_error(code: ErrorCode, message: &'static str, path: &Path) -> AppError {
    let mut error = AppError::new(code, message);
    error.context.destination_path = Some(display_path(path));
    error
}
fn invalid_path() -> AppError {
    AppError::new(
        ErrorCode::InvalidDestination,
        "Choose a regular file in an existing, accessible folder.",
    )
}
fn changed(path: &Path) -> AppError {
    file_error(
        ErrorCode::DestinationStateChanged,
        "The destination or its folder changed. Choose the destination again.",
        path,
    )
}
fn io_failure(path: &Path) -> AppError {
    file_error(
        ErrorCode::FileWriteFailed,
        "The selected folder could not be written or inspected.",
        path,
    )
}

pub fn is_reparse(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

/// Narrow native handle, with no-follow flags. Writers and validation readers
/// close before publication; retained directory handles only locate owned
/// artifacts for cleanup. No frontend file permissions are involved.
pub fn open_narrow(path: &Path, write: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(write);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        };
        options.custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path)
}

pub fn handle_identity(file: &File) -> io::Result<FileIdentity> {
    #[cfg(windows)]
    {
        use std::{mem::MaybeUninit, os::windows::io::AsRawHandle};
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };
        let mut information = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
        // SAFETY: valid owned handle and writable correctly-sized output buffer.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        let information = unsafe { information.assume_init() };
        Ok(FileIdentity(
            u64::from(information.dwVolumeSerialNumber),
            (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
        ))
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = file.metadata()?;
        Ok(FileIdentity(metadata.dev(), metadata.ino()))
    }
}
fn state(path: &Path) -> AppResult<Option<FileState>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(io_failure(path)),
    };
    if !metadata.is_file() || is_reparse(&metadata) {
        return Err(invalid_path());
    }
    let file = open_narrow(path, false).map_err(|_| io_failure(path))?;
    let current = file.metadata().map_err(|_| io_failure(path))?;
    if !current.is_file() || is_reparse(&current) {
        return Err(invalid_path());
    }
    #[cfg(unix)]
    let change = {
        use std::os::unix::fs::MetadataExt;
        (current.ctime(), current.ctime_nsec())
    };
    #[cfg(windows)]
    let change = {
        use std::os::windows::fs::MetadataExt;
        (i64::from(current.file_attributes()), 0)
    };
    Ok(Some(FileState {
        identity: handle_identity(&file).map_err(|_| io_failure(path))?,
        length: current.len(),
        modified: current.modified().map_err(|_| io_failure(path))?,
        created: current.created().ok(),
        readonly: current.permissions().readonly(),
        change,
    }))
}
pub fn require_regular_file(path: &Path) -> AppResult<FileIdentity> {
    state(path)?.map(|s| s.identity).ok_or_else(invalid_path)
}
pub fn sidecar_paths(path: &Path) -> [PathBuf; 3] {
    ["-wal", "-shm", "-journal"].map(|suffix| {
        let mut value = path.as_os_str().to_os_string();
        value.push(suffix);
        PathBuf::from(value)
    })
}
pub fn require_no_sidecars(path: &Path) -> AppResult<()> {
    for sidecar in sidecar_paths(path) {
        match fs::symlink_metadata(&sidecar) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Ok(_) => {
                return Err(file_error(
                    ErrorCode::BackupDestinationInUse,
                    "A SQLite sidecar exists at this backup name. Choose another destination or close its other user.",
                    &sidecar,
                ));
            }
            Err(_) => return Err(io_failure(&sidecar)),
        }
    }
    Ok(())
}

fn validate_path(path: &Path) -> AppResult<()> {
    if !path.is_absolute() || path.file_name().is_none() {
        return Err(invalid_path());
    }
    let text = path.to_str().ok_or_else(invalid_path)?;
    if text
        .chars()
        .any(|c| c.is_control() || matches!(c, '<' | '>' | '"' | '|' | '?' | '*'))
    {
        return Err(invalid_path());
    }
    // Reject namespaces supplied by the caller. Canonical Windows paths may later
    // contain the OS-generated extended-length prefix, which SQLite accepts.
    if text.starts_with("\\\\?\\") || text.starts_with("\\\\.\\") {
        return Err(invalid_path());
    }
    for component in path.components() {
        match component {
            Component::ParentDir | Component::CurDir => return Err(invalid_path()),
            Component::Normal(value) => {
                let name = value.to_str().ok_or_else(invalid_path)?;
                if name.contains(':') || name.ends_with(['.', ' ']) {
                    return Err(invalid_path());
                }
                let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
                if matches!(
                    stem.as_str(),
                    "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
                ) || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                    && stem[3..].chars().count() == 1
                    && stem[3..]
                        .chars()
                        .any(|c| matches!(c, '1'..='9' | '¹' | '²' | '³')))
                {
                    return Err(invalid_path());
                }
            }
            _ => {}
        }
    }
    Ok(())
}
fn normalize_extension(path: &Path, extension: &str) -> AppResult<PathBuf> {
    if !matches!(extension, "sqlite" | "xlsx" | "pdf") {
        return Err(invalid_path());
    }
    match path.extension().and_then(|s| s.to_str()) {
        None | Some("") => Ok(path.with_extension(extension)),
        Some(actual) if actual.eq_ignore_ascii_case(extension) => {
            Ok(path.with_extension(extension))
        }
        Some(_) => Err(file_error(
            ErrorCode::InvalidDestinationExtension,
            "The filename has a different extension. Choose the requested file type.",
            path,
        )),
    }
}
fn observe(path: &Path) -> AppResult<Observation> {
    let parent =
        fs::canonicalize(path.parent().ok_or_else(invalid_path)?).map_err(|_| invalid_path())?;
    let parent_handle = open_narrow(&parent, false).map_err(|_| invalid_path())?;
    let metadata = parent_handle.metadata().map_err(|_| invalid_path())?;
    if !metadata.is_dir() || is_reparse(&metadata) {
        return Err(invalid_path());
    }
    let parent_identity = handle_identity(&parent_handle).map_err(|_| invalid_path())?;
    drop(parent_handle);
    let final_path = parent.join(path.file_name().ok_or_else(invalid_path)?);
    let state = state(&final_path)?;
    let final_path = if state.is_some() {
        let canonical = fs::canonicalize(final_path).map_err(|_| invalid_path())?;
        // Windows canonicalization may return the existing entry's uppercase
        // extension. Keep the requested normalized extension for publication.
        match path.extension() {
            Some(extension) => canonical.with_extension(extension),
            None => canonical,
        }
    } else {
        final_path
    };
    Ok(Observation {
        parent,
        parent_identity,
        final_path,
        state,
    })
}
fn same_path(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        left.to_string_lossy().to_lowercase() == right.to_string_lossy().to_lowercase()
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}
fn protect(source: &Path, observation: &Observation) -> AppResult<()> {
    let source = fs::canonicalize(source).map_err(|_| invalid_path())?;
    for protected in std::iter::once(source.clone()).chain(sidecar_paths(&source)) {
        if same_path(&protected, &observation.final_path)
            || observation.state.as_ref().is_some_and(|destination| {
                // Compare identities of existing protected entries, including hard links.
                fs::metadata(&protected).is_ok()
                    && open_narrow(&protected, false)
                        .and_then(|f| handle_identity(&f))
                        .is_ok_and(|id| id == destination.identity)
            })
        {
            return Err(file_error(
                ErrorCode::ProtectedDatabaseDestination,
                "The live database and its sidecars cannot be output destinations. Choose another file.",
                &observation.final_path,
            ));
        }
        // Fail closed if an existing protected entry cannot be inspected.
        match fs::symlink_metadata(&protected) {
            Ok(_) => {
                let file = open_narrow(&protected, false)
                    .map_err(|_| io_failure(&observation.final_path))?;
                handle_identity(&file).map_err(|_| io_failure(&observation.final_path))?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return Err(io_failure(&observation.final_path)),
        }
    }
    Ok(())
}

impl SafeDestinationService {
    pub fn prepare(
        &self,
        source: &Path,
        requested: &Path,
        extension: &str,
        token: Option<&str>,
        policy: DestinationPolicy,
    ) -> AppResult<PreparedResult> {
        // Keep observations and recovery decisions consistent with another
        // invocation's native publication. Format generation stays parallel.
        let _publication = self.publication.lock().map_err(|_| io_failure(requested))?;
        // Consume even a malformed or wrong-path confirmation exactly once.
        let authorization = if let Some(token) = token {
            let mut tokens = self.tokens.lock().map_err(|_| invalid_path())?;
            let index = tokens.iter().position(|entry| entry.token == token)
                .ok_or_else(|| AppError::new(ErrorCode::OverwriteAuthorizationInvalid, "This overwrite confirmation expired or was already used. Choose the file again."))?;
            tokens.remove(index)
        } else {
            None
        };
        validate_path(requested)?;
        if let DestinationPolicy::AppOwnedNoOverwrite { directory } = &policy {
            if requested.parent() != Some(directory.as_path()) {
                return Err(invalid_path());
            }
            fs::create_dir_all(directory).map_err(|_| io_failure(directory))?;
        }
        // Protect direct sidecar names before extension processing too.
        protect(source, &observe(requested)?)?;
        let requested = normalize_extension(requested, extension)?;
        let observed = observe(&requested)?;
        protect(source, &observed)?;
        if let Some(recovery) = self
            .recovery
            .lock()
            .map_err(|_| invalid_path())?
            .iter()
            .find(|(path, _)| same_path(path, &observed.final_path))
            .map(|(_, recovery)| recovery)
        {
            return Err(recovery_error(&observed.final_path, recovery));
        }
        if extension == "sqlite" {
            require_no_sidecars(&observed.final_path)?;
        }
        if let Some(authorization) = authorization {
            if !same_path(&authorization.observation.final_path, &observed.final_path) {
                return Err(AppError::new(
                    ErrorCode::OverwriteAuthorizationInvalid,
                    "The confirmation belongs to another destination. Choose the file again.",
                ));
            }
            if authorization.observation != observed {
                return Err(changed(&observed.final_path));
            }
        } else if observed.state.is_some() {
            if !matches!(policy, DestinationPolicy::Manual) {
                return Err(changed(&observed.final_path));
            }
            let token = Uuid::new_v4().to_string();
            let mut tokens = self.tokens.lock().map_err(|_| invalid_path())?;
            // A fresh observation supersedes unused confirmations for this path.
            tokens.retain(|entry| !same_path(&entry.observation.final_path, &observed.final_path));
            while tokens.len() >= MAX_OVERWRITE_TOKENS {
                tokens.pop_front();
            }
            tokens.push_back(Confirmation {
                token: token.clone(),
                observation: observed.clone(),
            });
            return Ok(PreparedResult::RequiresOverwrite {
                destination_path: display_path(&observed.final_path),
                overwrite_token: token,
            });
        }
        Ok(PreparedResult::Ready(PreparedDestination {
            requested,
            observed,
            source: source.to_owned(),
            policy,
        }))
    }

    pub fn claim_temporary(
        &self,
        destination: &PreparedDestination,
        sqlite: bool,
    ) -> AppResult<OwnedTemporary> {
        for _ in 0..TEMPORARY_NAME_ATTEMPTS {
            #[cfg(windows)]
            let parent = retain_temporary_parent(destination)?;
            let path = destination
                .observed
                .parent
                .join(format!(".shift-happens-{}.tmp", Uuid::new_v4()));
            if sqlite && require_no_sidecars(&path).is_err() {
                continue;
            }
            let file = match OpenOptions::new()
                .write(true)
                .read(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(io_failure(destination.final_path())),
            };
            let identity = match handle_identity(&file) {
                Ok(identity) => identity,
                Err(_) => {
                    drop(file);
                    // Without a verified identity, deleting by pathname could
                    // remove another actor's replacement. Retain and report it.
                    let mut error = io_failure(&path);
                    error.context.cleanup_warnings.push(file_error(
                        ErrorCode::FileCleanupFailed,
                        "A claimed temporary file could not be verified and was retained.",
                        &path,
                    ));
                    return Err(error);
                }
            };
            drop(file);
            let mut temporary = OwnedTemporary {
                path,
                identity,
                cleanup_attempted: false,
                #[cfg(windows)]
                parent,
            };
            if sqlite && require_no_sidecars(&temporary.path).is_err() {
                // Unknown sidecars are never opened, adopted or deleted.
                let warnings = temporary.cleanup();
                if let Some(error) = warnings.into_iter().next() {
                    return Err(error);
                }
                continue;
            }
            if let Err(mut error) = temporary.verify() {
                error.context.cleanup_warnings.extend(temporary.cleanup());
                return Err(error);
            }
            return Ok(temporary);
        }
        Err(file_error(
            ErrorCode::TemporaryNameExhausted,
            "No unused temporary filename could be claimed. Choose another folder.",
            destination.final_path(),
        ))
    }

    fn revalidate(&self, destination: &PreparedDestination, sqlite: bool) -> AppResult<()> {
        let current =
            observe(&destination.requested).map_err(|_| changed(destination.final_path()))?;
        if current != destination.observed || protect(&destination.source, &current).is_err() {
            return Err(changed(destination.final_path()));
        }
        if sqlite {
            require_no_sidecars(destination.final_path())?;
        }
        Ok(())
    }

    /// Caller has closed its writer, synchronizer and independent validator.
    /// Returns committed warnings; an OS error after an observed commit is success.
    pub fn publish(
        &self,
        destination: &PreparedDestination,
        temporary: &mut OwnedTemporary,
        sqlite: bool,
    ) -> AppResult<Vec<AppError>> {
        // Rechecking and committing form one critical section across every
        // caller of this service. A later invocation must observe our commit.
        let _publication = self
            .publication
            .lock()
            .map_err(|_| io_failure(destination.final_path()))?;
        temporary.verify()?;
        let mut recovery = None;
        if destination.observed.state.is_some() {
            if !matches!(destination.policy, DestinationPolicy::Manual) {
                return Err(changed(destination.final_path()));
            }
            self.revalidate(destination, sqlite)?;
            // The hard link protects prior bytes even if a native rename reports
            // failure with changed pathnames. No delete-then-rename fallback.
            for _ in 0..TEMPORARY_NAME_ATTEMPTS {
                #[cfg(windows)]
                let parent = retain_temporary_parent(destination)?;
                let path = destination.observed.parent.join(format!(
                    ".shift-happens-original-{}.recovery",
                    Uuid::new_v4()
                ));
                match fs::hard_link(destination.final_path(), &path) {
                    Ok(()) => {
                        let identity = match require_regular_file(&path) {
                            Ok(identity) => identity,
                            Err(_) => {
                                self.recovery
                                    .lock()
                                    .map_err(|_| recovery_error(destination.final_path(), &path))?
                                    .insert(destination.observed.final_path.clone(), path.clone());
                                return Err(recovery_error(destination.final_path(), &path));
                            }
                        };
                        if Some(&identity)
                            != destination.observed.state.as_ref().map(|s| &s.identity)
                        {
                            // Another actor's entry is not ours to delete.
                            self.recovery
                                .lock()
                                .map_err(|_| recovery_error(destination.final_path(), &path))?
                                .insert(destination.observed.final_path.clone(), path.clone());
                            return Err(recovery_error(destination.final_path(), &path));
                        }
                        recovery = Some(OwnedTemporary {
                            path,
                            identity,
                            cleanup_attempted: false,
                            #[cfg(windows)]
                            parent,
                        });
                        break;
                    }
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(_) => {
                        return Err(file_error(
                            ErrorCode::OverwriteUnavailable,
                            "The folder cannot protect the existing file for replacement. Choose a new filename.",
                            destination.final_path(),
                        ));
                    }
                }
            }
            if recovery.is_none() {
                return Err(file_error(
                    ErrorCode::TemporaryNameExhausted,
                    "No recovery filename could be claimed. Choose another folder.",
                    destination.final_path(),
                ));
            }
        }
        // On Unix, creating a hard link changes ctime without changing bytes or
        // authorized modification metadata. Observe that specific owned change.
        #[cfg(unix)]
        let original_change = recovery.as_ref().and_then(|_| {
            state(destination.final_path())
                .ok()
                .flatten()
                .map(|s| s.change)
        });
        #[cfg(unix)]
        let rechecked = {
            let current =
                observe(&destination.requested).map_err(|_| changed(destination.final_path()));
            current.and_then(|mut current| {
                if let (Some(change), Some(current_state), Some(original)) = (
                    original_change,
                    current.state.as_mut(),
                    destination.observed.state.as_ref(),
                ) && current_state.change == change
                {
                    current_state.change = original.change;
                }
                if current != destination.observed
                    || protect(&destination.source, &current).is_err()
                {
                    return Err(changed(destination.final_path()));
                }
                if sqlite {
                    require_no_sidecars(destination.final_path())?;
                }
                Ok(())
            })
        };
        #[cfg(windows)]
        let rechecked = self.revalidate(destination, sqlite);
        if let Err(mut error) = rechecked {
            if let Some(mut original) = recovery {
                error.context.cleanup_warnings.extend(original.cleanup());
            }
            return Err(error);
        }
        let result = native_publish(
            &temporary.path,
            destination.final_path(),
            destination.observed.state.is_some(),
        );
        match result {
            Ok(()) => {
                let mut warnings = temporary.cleanup();
                if let Some(mut original) = recovery {
                    warnings.extend(original.cleanup());
                }
                Ok(warnings)
            }
            Err(_) => {
                let final_state = state(destination.final_path());
                if final_state
                    .as_ref()
                    .ok()
                    .and_then(|s| s.as_ref())
                    .is_some_and(|s| s.identity == temporary.identity)
                {
                    let mut warnings = vec![file_error(
                        ErrorCode::FilePublicationFailed,
                        "The operating system reported a move error, but the new file was published. Do not retry this file operation.",
                        destination.final_path(),
                    )];
                    warnings.extend(temporary.cleanup());
                    if let Some(mut original) = recovery {
                        warnings.extend(original.cleanup());
                    }
                    return Ok(warnings);
                }
                if let Some(mut original) = recovery {
                    let unchanged = final_state
                        .as_ref()
                        .ok()
                        .and_then(|s| s.as_ref())
                        .is_some_and(|s| {
                            destination.observed.state.as_ref().is_some_and(|old| {
                                s.identity == old.identity
                                    && s.length == old.length
                                    && s.modified == old.modified
                            })
                        });
                    if !unchanged {
                        let location_warning = original.refresh_cleanup_path().err();
                        let mut error = recovery_error(destination.final_path(), &original.path);
                        error.context.cleanup_warnings.extend(location_warning);
                        self.recovery
                            .lock()
                            .map_err(|_| recovery_error(destination.final_path(), &original.path))?
                            .insert(
                                destination.observed.final_path.clone(),
                                original.path.clone(),
                            );
                        error.context.publication_state = Some("unconfirmed_recovery_retained");
                        // Do not clean original: it contains the prior destination.
                        return Err(error);
                    }
                    let mut error = file_error(
                        ErrorCode::FilePublicationFailed,
                        "The file could not be replaced. The prior destination is preserved.",
                        destination.final_path(),
                    );
                    error.context.publication_state = Some("unpublished_original_preserved");
                    error.context.cleanup_warnings.extend(original.cleanup());
                    return Err(error);
                }
                match final_state {
                    Ok(None) => {
                        let mut error = file_error(
                            ErrorCode::FilePublicationFailed,
                            "The completed file could not be published. The destination remains absent.",
                            destination.final_path(),
                        );
                        error.context.publication_state = Some("unpublished_destination_absent");
                        Err(error)
                    }
                    Ok(Some(_)) => Err(changed(destination.final_path())),
                    Err(_) => {
                        let mut error = file_error(
                            ErrorCode::FilePublicationFailed,
                            "The operating system reported a publication error and the destination cannot be inspected. Inspect it before retrying.",
                            destination.final_path(),
                        );
                        error.context.publication_state =
                            Some("unconfirmed_destination_unavailable");
                        Err(error)
                    }
                }
            }
        }
    }
}
#[cfg(windows)]
fn retain_temporary_parent(destination: &PreparedDestination) -> AppResult<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
    };
    let parent = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        // Rust's default read/write/delete sharing lets the manager rename this
        // directory; the retained handle never opens a writer or child file.
        .open(&destination.observed.parent)
        .map_err(|_| changed(destination.final_path()))?;
    let metadata = parent
        .metadata()
        .map_err(|_| changed(destination.final_path()))?;
    if !metadata.is_dir()
        || is_reparse(&metadata)
        || handle_identity(&parent).map_err(|_| changed(destination.final_path()))?
            != destination.observed.parent_identity
    {
        return Err(changed(destination.final_path()));
    }
    Ok(parent)
}
fn recovery_error(path: &Path, recovery: &Path) -> AppError {
    let mut error = file_error(
        ErrorCode::PublicationRecoveryRequired,
        "Publication could not be confirmed. The prior file is retained at the recovery path. Resolve recovery before retrying this destination.",
        path,
    );
    error.context.recovery_path = Some(display_path(recovery));
    error.context.publication_state = Some("unconfirmed_recovery_retained");
    error
}

pub struct OwnedTemporary {
    pub path: PathBuf,
    identity: FileIdentity,
    cleanup_attempted: bool,
    #[cfg(windows)]
    parent: File,
}
impl OwnedTemporary {
    /// Keep cleanup attached to the originally verified directory when its name
    /// changes. This only locates owned artifacts; publication still requires
    /// the manager's original parent observation to remain unchanged.
    fn refresh_cleanup_path(&mut self) -> AppResult<()> {
        #[cfg(windows)]
        {
            use std::os::windows::{ffi::OsStringExt, io::AsRawHandle};
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_NAME_NORMALIZED, GetFinalPathNameByHandleW, VOLUME_NAME_DOS,
            };
            let failure = || {
                file_error(
                    ErrorCode::FileCleanupFailed,
                    "The temporary folder could not be located through its verified handle; inspect that folder for the retained owned file.",
                    &self.path,
                )
            };
            let metadata = self.parent.metadata().map_err(|_| failure())?;
            if !metadata.is_dir() || is_reparse(&metadata) {
                return Err(failure());
            }
            let mut buffer = vec![0u16; 260];
            for _ in 0..3 {
                // SAFETY: the verified directory handle stays live, and the
                // UTF-16 output buffer has the capacity passed to Windows.
                let written = unsafe {
                    GetFinalPathNameByHandleW(
                        self.parent.as_raw_handle(),
                        buffer.as_mut_ptr(),
                        buffer.len() as u32,
                        FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
                    )
                } as usize;
                if written == 0 || written > 32768 {
                    return Err(failure());
                }
                if written < buffer.len() {
                    let parent = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..written]));
                    let current = open_narrow(&parent, false).map_err(|_| failure())?;
                    let current_metadata = current.metadata().map_err(|_| failure())?;
                    if !current_metadata.is_dir()
                        || is_reparse(&current_metadata)
                        || handle_identity(&current).map_err(|_| failure())?
                            != handle_identity(&self.parent).map_err(|_| failure())?
                    {
                        return Err(failure());
                    }
                    self.path = parent.join(self.path.file_name().ok_or_else(failure)?);
                    return Ok(());
                }
                // A directory rename can lengthen the name between calls.
                // Bound both allocation and retries rather than searching paths.
                buffer.resize(written + 1, 0);
            }
            Err(failure())
        }
        #[cfg(not(windows))]
        Ok(())
    }
    pub fn verify(&self) -> AppResult<()> {
        if require_regular_file(&self.path)? != self.identity {
            return Err(changed(&self.path));
        }
        Ok(())
    }
    pub fn synchronize(&self) -> AppResult<u64> {
        let file = self.open_writer()?;
        file.sync_all().map_err(|_| {
            file_error(
                ErrorCode::FileSynchronizationFailed,
                "The completed file could not be synchronized. No output was published.",
                &self.path,
            )
        })?;
        Ok(file.metadata().map_err(|_| io_failure(&self.path))?.len())
    }
    /// Format writers reopen only our claimed entry, without creation/truncation
    /// or following links. The consumer flushes and closes it before sync.
    pub fn open_writer(&self) -> AppResult<File> {
        self.verify()?;
        let file = open_narrow(&self.path, true).map_err(|_| io_failure(&self.path))?;
        if handle_identity(&file).map_err(|_| io_failure(&self.path))? != self.identity {
            return Err(changed(&self.path));
        }
        Ok(file)
    }
    /// Windows deletion stays attached to the verified handle. Deleting by
    /// pathname after closing an identity check could delete a replacement.
    fn remove_owned(&self) -> AppResult<()> {
        #[cfg(windows)]
        {
            use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
            use windows_sys::Win32::Storage::FileSystem::{
                DELETE, FILE_DISPOSITION_INFO, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ, FILE_SHARE_WRITE, FileDispositionInfo, SetFileInformationByHandle,
            };
            let file = OpenOptions::new()
                .access_mode(DELETE | FILE_READ_ATTRIBUTES)
                // Hold the name against deletion/rename while checking and
                // marking this handle. Existing incompatible users fail closed.
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .open(&self.path)
                .map_err(|_| io_failure(&self.path))?;
            let metadata = file.metadata().map_err(|_| io_failure(&self.path))?;
            if !metadata.is_file()
                || is_reparse(&metadata)
                || handle_identity(&file).map_err(|_| io_failure(&self.path))? != self.identity
            {
                return Err(changed(&self.path));
            }
            let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
            // SAFETY: live DELETE-access handle, verified owned regular file,
            // and a correctly sized initialized disposition structure.
            if unsafe {
                SetFileInformationByHandle(
                    file.as_raw_handle(),
                    FileDispositionInfo,
                    (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                    std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
                )
            } == 0
            {
                return Err(io_failure(&self.path));
            }
            // Close applies deletion to this handle's entry, never a later
            // pathname replacement. Other links to the original remain intact.
            drop(file);
            Ok(())
        }
        #[cfg(unix)]
        {
            self.verify()?;
            fs::remove_file(&self.path).map_err(|_| io_failure(&self.path))
        }
    }
    /// Attempt deletion once, only while the exact entry still belongs to us.
    /// SQLite normally removes its own sidecars on clean close. Unknown sidecars
    /// are never deleted; they are surfaced separately for manager recovery.
    pub fn cleanup(&mut self) -> Vec<AppError> {
        if self.cleanup_attempted {
            return Vec::new();
        }
        self.cleanup_attempted = true;
        if let Err(warning) = self.refresh_cleanup_path() {
            return vec![warning];
        }
        match fs::symlink_metadata(&self.path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Ok(_) => match self.remove_owned() {
                Ok(()) => Vec::new(),
                Err(_) => vec![file_error(
                    ErrorCode::FileCleanupFailed,
                    "A temporary path changed, is in use, or could not be deleted; it was retained.",
                    &self.path,
                )],
            },
            _ => vec![file_error(
                ErrorCode::FileCleanupFailed,
                "A temporary path changed or cannot be inspected; it was retained.",
                &self.path,
            )],
        }
    }
}

#[cfg(windows)]
fn native_publish(source: &Path, destination: &Path, replace: bool) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let flags = MOVEFILE_WRITE_THROUGH
        | if replace {
            MOVEFILE_REPLACE_EXISTING
        } else {
            0
        };
    // SAFETY: live null-terminated native strings; same parent, no copy/reboot flags.
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), flags) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
#[cfg(unix)]
fn native_publish(source: &Path, destination: &Path, replace: bool) -> io::Result<()> {
    if replace {
        fs::rename(source, destination)
    } else {
        // link is an atomic no-replace publication, with owned-name cleanup later.
        fs::hard_link(source, destination)
    }
}
