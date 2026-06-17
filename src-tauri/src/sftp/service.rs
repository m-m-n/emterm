//! In-process SFTP upload orchestration.
//!
//! Replaces the Tauri command layer (`src-tauri/src/commands/sftp.rs`). The
//! service owns the process manager and the concurrency pool, validates inputs,
//! generates wall-clock-free session ids, runs uploads on worker threads, and
//! reports progress / duplicate-check outcomes over crossbeam channels that the
//! egui loop drains each frame.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use app_settings::SshConnection;
use crossbeam_channel::{Receiver, Sender};

use crate::sftp::check::find_duplicates;
use crate::sftp::pool::ConcurrentUploadPool;
use crate::sftp::process::SftpProcessManager;
use crate::sftp::{SftpUploadProgress, SftpUploadStatus};

/// Connection inputs for an sftp invocation, decoupled from the settings type.
#[derive(Debug, Clone, PartialEq)]
pub struct SftpConnection {
    pub hostname: String,
    pub port: u16,
    pub username: String,
    pub identity_file: String,
    pub ssh_options: Vec<(String, String)>,
}

impl SftpConnection {
    /// Map a settings [`SshConnection`] record to argv inputs.
    pub fn from_ssh_connection(conn: &SshConnection) -> Self {
        Self {
            hostname: conn.hostname.clone(),
            port: conn.port,
            username: conn.username.clone(),
            identity_file: conn.identity_file.clone(),
            ssh_options: conn
                .ssh_options
                .iter()
                .map(|o| (o.key.clone(), o.value.clone()))
                .collect(),
        }
    }
}

/// Outcome of an off-thread duplicate check, delivered over the result channel.
#[derive(Debug, Clone)]
pub struct DuplicateCheckResult {
    /// The dialog/request this result belongs to (a UI-supplied token).
    pub request_id: u64,
    /// Remote names that already exist, or `Err` with a descriptive message.
    pub outcome: Result<Vec<String>, String>,
}

/// The receiver half handed to the UI for progress events.
pub type ProgressReceiver = Receiver<SftpUploadProgress>;
/// The receiver half handed to the UI for duplicate-check results.
pub type ResultReceiver = Receiver<DuplicateCheckResult>;

/// One queued upload, handed from `start_upload` to the resident dispatcher.
///
/// Created on the UI thread but carries no UI references; everything the worker
/// needs (size computation, the `put` invocation, terminal-status emission) runs
/// off-thread so neither the render thread nor a per-file blocked thread is held.
struct UploadJob {
    session_id: String,
    connection: SftpConnection,
    local_path: String,
    remote_path: String,
    is_directory: bool,
    file_name: String,
}

/// In-process SFTP orchestration service.
pub struct SftpService {
    manager: Arc<SftpProcessManager>,
    pool: Arc<ConcurrentUploadPool>,
    progress_tx: Sender<SftpUploadProgress>,
    result_tx: Sender<DuplicateCheckResult>,
    /// Detected once at construction; empty when no sftp binary is available.
    sftp_binary: String,
    /// Monotonic session-id source (no wall-clock).
    next_session: AtomicU64,
    /// session_id → originating tab stable_id, for the tab-close guard.
    /// Behind an `Arc` so worker threads can clear their own entry on finish.
    session_tab: Arc<Mutex<HashMap<String, u64>>>,
    /// Session ids that have been cancelled before/while their slot was held.
    /// `cancel` records here so a worker that has not yet acquired its pool
    /// slot (a queued upload) can abort instead of transferring. Behind an
    /// `Arc` so the dispatcher and transfer threads can consult/clear it.
    cancelled: Arc<Mutex<HashSet<String>>>,
    /// Queue feeding the single resident dispatcher thread. Each `start_upload`
    /// enqueues one job; the dispatcher blocks on `acquire_slot` so at most one
    /// extra thread waits on a slot regardless of how many files were dropped.
    job_tx: Sender<UploadJob>,
}

impl SftpService {
    /// Construct the service, detecting the sftp binary once. Returns the
    /// service plus the receiver halves for the egui loop to drain.
    pub fn new(max_concurrent: u16) -> (Self, ProgressReceiver, ResultReceiver) {
        let (progress_tx, progress_rx) = crossbeam_channel::unbounded();
        let (result_tx, result_rx) = crossbeam_channel::unbounded();
        let (job_tx, job_rx) = crossbeam_channel::unbounded::<UploadJob>();

        let manager = Arc::new(SftpProcessManager::new());
        let pool = Arc::new(ConcurrentUploadPool::new(max_concurrent));
        let session_tab = Arc::new(Mutex::new(HashMap::new()));
        let cancelled = Arc::new(Mutex::new(HashSet::new()));
        let sftp_binary = detect_sftp_binary();

        spawn_dispatcher(DispatcherCtx {
            job_rx,
            manager: Arc::clone(&manager),
            pool: Arc::clone(&pool),
            progress_tx: progress_tx.clone(),
            session_tab: Arc::clone(&session_tab),
            cancelled: Arc::clone(&cancelled),
            sftp_binary: sftp_binary.clone(),
        });

        let service = Self {
            manager,
            pool,
            progress_tx,
            result_tx,
            sftp_binary,
            next_session: AtomicU64::new(1),
            session_tab,
            cancelled,
            job_tx,
        };
        (service, progress_rx, result_rx)
    }

    /// Whether an sftp binary was found at construction.
    pub fn has_sftp_binary(&self) -> bool {
        !self.sftp_binary.is_empty()
    }

    /// Produce a unique, monotonically increasing session id (no wall-clock).
    pub fn next_session_id(&self) -> String {
        let n = self.next_session.fetch_add(1, Ordering::Relaxed);
        format!("sftp-{}", n)
    }

    /// Update the concurrency cap (settings reload).
    pub fn set_max_concurrent(&self, max: u16) {
        self.pool.set_max_concurrent(max);
    }

    /// Current concurrency cap.
    pub fn max_concurrent(&self) -> u16 {
        self.pool.max_concurrent()
    }

    /// List a remote directory off the UI thread and detect duplicates,
    /// delivering the outcome over the result channel keyed by `request_id`.
    pub fn check_duplicates(
        &self,
        request_id: u64,
        connection: SftpConnection,
        remote_dir: String,
        file_names: Vec<String>,
    ) {
        let result_tx = self.result_tx.clone();
        let manager = Arc::clone(&self.manager);
        let sftp_binary = self.sftp_binary.clone();

        // Validate up-front so an immediate error still flows over the channel.
        if let Err(e) = validate_connection(&connection)
            .and_then(|_| validate_remote_path(&remote_dir))
            .and_then(|_| get_sftp_binary(&sftp_binary))
        {
            let _ = result_tx.send(DuplicateCheckResult {
                request_id,
                outcome: Err(e),
            });
            return;
        }

        std::thread::spawn(move || {
            let outcome = manager
                .spawn_ls(
                    &sftp_binary,
                    &connection.hostname,
                    connection.port,
                    &connection.username,
                    &connection.identity_file,
                    &connection.ssh_options,
                    &remote_dir,
                )
                .map(|ls_output| find_duplicates(&ls_output, &file_names));
            let _ = result_tx.send(DuplicateCheckResult {
                request_id,
                outcome,
            });
        });
    }

    /// Validate then spawn an upload worker, recording the session→tab link.
    ///
    /// Returns the new session id on success, or a validation error string.
    /// `preparing` is emitted immediately; `uploading` and the terminal status
    /// are emitted from the worker thread.
    pub fn start_upload(
        &self,
        tab_id: u64,
        connection: SftpConnection,
        local_path: String,
        remote_path: String,
        is_directory: bool,
    ) -> Result<String, String> {
        validate_connection(&connection)?;
        validate_remote_path(&remote_path)?;
        validate_local_path(&local_path)?;
        get_sftp_binary(&self.sftp_binary)?;

        let session_id = self.next_session_id();
        self.session_tab
            .lock()
            .unwrap()
            .insert(session_id.clone(), tab_id);

        let file_name = std::path::Path::new(&local_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| local_path.clone());

        // preparing — emitted immediately with total_bytes=0; the size is
        // computed off the UI thread (it recurses into directories) and the
        // real total is reported with the `Uploading` event from the worker.
        self.emit(SftpUploadProgress {
            session_id: session_id.clone(),
            file_name: file_name.clone(),
            bytes_transferred: 0,
            total_bytes: 0,
            status: SftpUploadStatus::Preparing,
            error_message: None,
        });

        // Enqueue onto the resident dispatcher rather than spawning a per-file
        // thread that blocks on `acquire_slot`; the dispatcher serializes slot
        // acquisition so the number of blocked-waiting threads stays bounded.
        let _ = self.job_tx.send(UploadJob {
            session_id: session_id.clone(),
            connection,
            local_path,
            remote_path,
            is_directory,
            file_name,
        });

        Ok(session_id)
    }

    /// Request abort of an upload (whether queued or in-flight).
    ///
    /// Records the session in the cancellation set and kills any running
    /// subprocess. Ownership of slot release and the `session_tab` entry stays
    /// with the worker (it removes them when it reaches its terminal state),
    /// so a queued cancel does not double-release a slot and `has_active_for_tab`
    /// stays true until the worker actually finishes. A queued worker (no slot
    /// yet) observes the cancellation set right after `acquire_slot` and aborts
    /// without transferring.
    pub fn cancel(&self, session_id: &str) {
        self.cancelled
            .lock()
            .unwrap()
            .insert(session_id.to_string());
        // Kill an already-running subprocess if present; a no-op for a queued
        // upload whose child has not spawned yet (the dispatcher will catch it).
        let _ = self.manager.cancel(session_id);
    }

    /// Whether the given tab has any active uploads.
    pub fn has_active_for_tab(&self, tab_id: u64) -> bool {
        self.session_tab
            .lock()
            .unwrap()
            .values()
            .any(|&t| t == tab_id)
    }

    /// Cancel every active upload that originated from the given tab.
    pub fn cancel_for_tab(&self, tab_id: u64) {
        let sessions: Vec<String> = {
            let map = self.session_tab.lock().unwrap();
            map.iter()
                .filter(|&(_, &t)| t == tab_id)
                .map(|(s, _)| s.clone())
                .collect()
        };
        for s in sessions {
            self.cancel(&s);
        }
    }

    fn emit(&self, progress: SftpUploadProgress) {
        let _ = self.progress_tx.send(progress);
    }
}

// ============================================================
// Resident upload dispatcher
// ============================================================

/// Shared handles the dispatcher thread needs to drive uploads.
struct DispatcherCtx {
    job_rx: Receiver<UploadJob>,
    manager: Arc<SftpProcessManager>,
    pool: Arc<ConcurrentUploadPool>,
    progress_tx: Sender<SftpUploadProgress>,
    session_tab: Arc<Mutex<HashMap<String, u64>>>,
    cancelled: Arc<Mutex<HashSet<String>>>,
    sftp_binary: String,
}

/// Emit a terminal `Cancelled` event and release the worker-owned bookkeeping
/// for `session_id` (slot is released only when `slot_held`).
fn finish_cancelled(
    ctx: &DispatcherCtx,
    session_id: &str,
    file_name: &str,
    total_bytes: u64,
    slot_held: bool,
) {
    if slot_held {
        ctx.pool.release_slot(session_id);
    }
    ctx.session_tab.lock().unwrap().remove(session_id);
    ctx.cancelled.lock().unwrap().remove(session_id);
    let _ = ctx.progress_tx.send(SftpUploadProgress {
        session_id: session_id.to_string(),
        file_name: file_name.to_string(),
        bytes_transferred: 0,
        total_bytes,
        status: SftpUploadStatus::Cancelled,
        error_message: None,
    });
}

/// Spawn the single resident dispatcher thread.
///
/// The dispatcher pulls one job at a time, blocks on a pool slot (so at most one
/// extra thread waits regardless of batch size), re-checks the cancellation set
/// both before and after acquiring the slot, then spawns a short-lived transfer
/// thread that owns slot release and the `session_tab` entry through its
/// terminal state.
fn spawn_dispatcher(ctx: DispatcherCtx) {
    std::thread::spawn(move || {
        while let Ok(job) = ctx.job_rx.recv() {
            let UploadJob {
                session_id,
                connection,
                local_path,
                remote_path,
                is_directory,
                file_name,
            } = job;

            // Queued cancel: aborted before a slot was ever taken.
            if ctx.cancelled.lock().unwrap().contains(&session_id) {
                finish_cancelled(&ctx, &session_id, &file_name, 0, false);
                continue;
            }

            ctx.pool.acquire_slot(&session_id);

            // Cancelled while waiting for the slot — release it and abort.
            if ctx.cancelled.lock().unwrap().contains(&session_id) {
                finish_cancelled(&ctx, &session_id, &file_name, 0, true);
                continue;
            }

            // Size is computed here (off the UI thread); directories recurse.
            let total_bytes = get_local_size(&local_path, is_directory);

            let _ = ctx.progress_tx.send(SftpUploadProgress {
                session_id: session_id.clone(),
                file_name: file_name.clone(),
                bytes_transferred: 0,
                total_bytes,
                status: SftpUploadStatus::Uploading,
                error_message: None,
            });

            // Hand the actual transfer to a short-lived thread so the dispatcher
            // returns to pulling jobs; that thread owns slot release and the
            // session_tab/cancelled cleanup at its terminal state.
            let manager = Arc::clone(&ctx.manager);
            let pool = Arc::clone(&ctx.pool);
            let progress_tx = ctx.progress_tx.clone();
            let session_tab = Arc::clone(&ctx.session_tab);
            let cancelled = Arc::clone(&ctx.cancelled);
            let sftp_binary = ctx.sftp_binary.clone();

            std::thread::spawn(move || {
                let result = manager.spawn_upload(
                    &session_id,
                    &sftp_binary,
                    &connection.hostname,
                    connection.port,
                    &connection.username,
                    &connection.identity_file,
                    &connection.ssh_options,
                    &local_path,
                    &remote_path,
                    is_directory,
                );

                pool.release_slot(&session_id);
                session_tab.lock().unwrap().remove(&session_id);
                let was_cancelled = cancelled.lock().unwrap().remove(&session_id);

                let progress = match result {
                    Ok(_) => SftpUploadProgress {
                        session_id,
                        file_name,
                        bytes_transferred: total_bytes,
                        total_bytes,
                        status: SftpUploadStatus::Completed,
                        error_message: None,
                    },
                    Err(e) => {
                        let status = if was_cancelled || e.contains("cancelled") {
                            SftpUploadStatus::Cancelled
                        } else {
                            SftpUploadStatus::Failed
                        };
                        SftpUploadProgress {
                            session_id,
                            file_name,
                            bytes_transferred: 0,
                            total_bytes,
                            status,
                            error_message: Some(e),
                        }
                    }
                };
                let _ = progress_tx.send(progress);
            });
        }
    });
}

// ============================================================
// Helper functions (ported from src-tauri/src/commands/sftp.rs)
// ============================================================

/// Get the total size of a local file or directory in bytes.
fn get_local_size(path: &str, is_directory: bool) -> u64 {
    if is_directory {
        dir_size(std::path::Path::new(path))
    } else {
        std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
    }
}

/// Recursively compute the total size of a directory.
fn dir_size(path: &std::path::Path) -> u64 {
    let mut total: u64 = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.is_dir() {
                total += dir_size(&entry.path());
            } else {
                total += meta.len();
            }
        }
    }
    total
}

/// Validate SSH connection inputs.
fn validate_connection(connection: &SftpConnection) -> Result<(), String> {
    if connection.hostname.is_empty() {
        return Err("Missing hostname".to_string());
    }
    if connection
        .hostname
        .chars()
        .any(|c| matches!(c, ';' | '|' | '&' | '`' | '$' | '(' | ')' | '{' | '}'))
    {
        return Err("Invalid hostname: contains shell metacharacters".to_string());
    }
    // Argv flag smuggling: a `[user@]host` element beginning with `-` would be
    // parsed by sftp as an option (e.g. `-oProxyCommand=...`). `build_sftp_args`
    // also inserts a `--` end-of-options marker as defense in depth, but reject
    // the value up front so such a connection never spawns a subprocess.
    if connection.hostname.starts_with('-') {
        return Err("Invalid hostname: must not start with '-'".to_string());
    }
    if connection.username.starts_with('-') {
        return Err("Invalid username: must not start with '-'".to_string());
    }
    Ok(())
}

/// Validate a remote path against sftp-batch command injection only.
///
/// This rejects null bytes and characters that could break out of (or smuggle
/// extra tokens into) the `ls`/`put` batch line. It deliberately does **not**
/// guard against directory traversal (`..`): the remote destination is derived
/// from the active SSH tab's OSC 7 CWD, which is already inside the trusted
/// remote session, so a traversal restriction would only break legitimate
/// uploads. Emission-time escaping in `process.rs` is the defense in depth for
/// the quoted argument itself.
fn validate_remote_path(path: &str) -> Result<(), String> {
    if path.contains('\0') {
        return Err("Invalid remote path: contains null bytes".to_string());
    }
    if path.chars().any(|c| {
        matches!(
            c,
            ';' | '|' | '&' | '`' | '$' | '(' | ')' | '"' | '\\' | '\n' | '\r'
        )
    }) {
        return Err("Invalid remote path: contains unsafe characters".to_string());
    }
    Ok(())
}

/// Validate that a local path is safe and exists.
fn validate_local_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("Local path is empty".to_string());
    }
    if path.contains('\0') {
        return Err("Invalid local path: contains null bytes".to_string());
    }
    #[cfg(windows)]
    let has_unsafe = path.chars().any(|c| matches!(c, '"' | '\n' | '\r'));
    #[cfg(not(windows))]
    let has_unsafe = path.chars().any(|c| matches!(c, '"' | '\\' | '\n' | '\r'));
    if has_unsafe {
        return Err("Invalid local path: contains unsafe characters".to_string());
    }
    if !std::path::Path::new(path).exists() {
        return Err(format!("Local path does not exist: {}", path));
    }
    Ok(())
}

/// Confirm an sftp binary was detected.
fn get_sftp_binary(detected: &str) -> Result<(), String> {
    if detected.is_empty() {
        Err("sftp command not found. Ensure openssh is installed.".to_string())
    } else {
        Ok(())
    }
}

/// Detect the sftp binary path on the current platform.
fn detect_sftp_binary() -> String {
    #[cfg(unix)]
    {
        detect_sftp_unix()
    }
    #[cfg(windows)]
    {
        detect_sftp_windows()
    }
}

#[cfg(unix)]
fn detect_sftp_unix() -> String {
    let path_var = match std::env::var("PATH") {
        Ok(p) => p,
        Err(_) => return String::new(),
    };
    for dir in path_var.split(':') {
        let candidate = std::path::PathBuf::from(dir).join("sftp");
        if candidate.is_file() {
            return candidate.to_string_lossy().to_string();
        }
    }
    String::new()
}

#[cfg(windows)]
fn detect_sftp_windows() -> String {
    let system32_path = std::path::PathBuf::from(r"C:\Windows\System32\OpenSSH\sftp.exe");
    if system32_path.is_file() {
        return system32_path.to_string_lossy().to_string();
    }
    let path_var = match std::env::var("PATH") {
        Ok(p) => p,
        Err(_) => return String::new(),
    };
    for dir in path_var.split(';') {
        let candidate = std::path::PathBuf::from(dir).join("sftp.exe");
        if candidate.is_file() {
            return candidate.to_string_lossy().to_string();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(host: &str) -> SftpConnection {
        SftpConnection {
            hostname: host.to_string(),
            port: 22,
            username: String::new(),
            identity_file: String::new(),
            ssh_options: Vec::new(),
        }
    }

    #[test]
    fn validate_connection_rejects_empty_hostname() {
        assert!(validate_connection(&conn("")).is_err());
    }

    #[test]
    fn validate_connection_rejects_shell_metacharacters() {
        assert!(validate_connection(&conn("host;rm -rf")).is_err());
        assert!(validate_connection(&conn("a$(b)")).is_err());
    }

    #[test]
    fn validate_connection_rejects_leading_dash() {
        // Argv flag smuggling: host/user beginning with `-` would be read as an
        // sftp option.
        assert!(validate_connection(&conn("-oProxyCommand=touch /tmp/pwned")).is_err());
        let mut c = conn("example.com");
        c.username = "-oProxyCommand=evil".to_string();
        assert!(validate_connection(&c).is_err());
    }

    #[test]
    fn validate_connection_accepts_plain_host() {
        assert!(validate_connection(&conn("example.com")).is_ok());
    }

    #[test]
    fn validate_remote_path_rejects_null_and_unsafe() {
        assert!(validate_remote_path("/tmp/\0x").is_err());
        assert!(validate_remote_path("/tmp/$(x)").is_err());
        assert!(validate_remote_path("/tmp/a\"b").is_err());
        assert!(validate_remote_path("/tmp/ok-dir").is_ok());
    }

    #[test]
    fn validate_local_path_rejects_empty_and_missing() {
        assert!(validate_local_path("").is_err());
        assert!(validate_local_path("/nonexistent/xyz/abc").is_err());
    }

    #[test]
    fn validate_local_path_accepts_existing() {
        assert!(validate_local_path("Cargo.toml").is_ok());
    }

    #[test]
    fn get_sftp_binary_reports_missing() {
        assert!(get_sftp_binary("").is_err());
        assert!(get_sftp_binary("/usr/bin/sftp").is_ok());
    }

    #[test]
    fn next_session_id_is_monotonic() {
        let (service, _p, _r) = SftpService::new(4);
        let a = service.next_session_id();
        let b = service.next_session_id();
        let c = service.next_session_id();
        assert_ne!(a, b);
        assert_ne!(b, c);
        // ids embed a strictly increasing counter
        let n = |s: &str| s.trim_start_matches("sftp-").parse::<u64>().unwrap();
        assert!(n(&a) < n(&b));
        assert!(n(&b) < n(&c));
    }

    #[test]
    fn from_ssh_connection_maps_fields() {
        use app_settings::{SshConnection, SshOption};
        let sc = SshConnection {
            name: "work".to_string(),
            hostname: "h".to_string(),
            port: 2222,
            username: "u".to_string(),
            identity_file: "~/k".to_string(),
            ssh_options: vec![SshOption {
                key: "K".to_string(),
                value: "V".to_string(),
            }],
            extra_options: String::new(),
        };
        let c = SftpConnection::from_ssh_connection(&sc);
        assert_eq!(c.hostname, "h");
        assert_eq!(c.port, 2222);
        assert_eq!(c.username, "u");
        assert_eq!(c.identity_file, "~/k");
        assert_eq!(c.ssh_options, vec![("K".to_string(), "V".to_string())]);
    }

    #[test]
    fn set_max_concurrent_updates_cap() {
        let (service, _p, _r) = SftpService::new(4);
        assert_eq!(service.max_concurrent(), 4);
        service.set_max_concurrent(2);
        assert_eq!(service.max_concurrent(), 2);
        // Minimum of 1 is enforced by the pool.
        service.set_max_concurrent(0);
        assert_eq!(service.max_concurrent(), 1);
    }

    #[test]
    fn active_for_tab_tracks_sessions() {
        let (service, _p, _r) = SftpService::new(4);
        // No uploads yet.
        assert!(!service.has_active_for_tab(7));
        // Simulate a recorded session→tab association.
        service
            .session_tab
            .lock()
            .unwrap()
            .insert("sftp-1".to_string(), 7);
        service
            .session_tab
            .lock()
            .unwrap()
            .insert("sftp-2".to_string(), 9);
        assert!(service.has_active_for_tab(7));
        assert!(service.has_active_for_tab(9));
        assert!(!service.has_active_for_tab(8));
    }

    #[test]
    fn cancel_marks_session_and_leaves_session_tab_to_worker() {
        // Ownership model (#3): cancel() only records the cancellation and
        // kills any subprocess; it must NOT remove the session_tab entry or
        // release the slot. The worker owns that cleanup at its terminal state,
        // so has_active_for_tab stays true until the worker actually finishes.
        let (service, _p, _r) = SftpService::new(4);
        service
            .session_tab
            .lock()
            .unwrap()
            .insert("sftp-1".to_string(), 7);

        service.cancel("sftp-1");

        // Recorded in the cancellation set so a queued worker can abort.
        assert!(service.cancelled.lock().unwrap().contains("sftp-1"));
        // session_tab entry is retained (worker-owned), so the tab still
        // reports an active upload until the worker tears it down.
        assert!(service.has_active_for_tab(7));
    }

    #[test]
    fn cancel_for_tab_marks_only_that_tabs_sessions() {
        let (service, _p, _r) = SftpService::new(4);
        service
            .session_tab
            .lock()
            .unwrap()
            .insert("sftp-1".to_string(), 7);
        service
            .session_tab
            .lock()
            .unwrap()
            .insert("sftp-2".to_string(), 9);

        service.cancel_for_tab(7);

        // Only tab 7's session is in the cancellation set.
        let cancelled = service.cancelled.lock().unwrap();
        assert!(cancelled.contains("sftp-1"));
        assert!(!cancelled.contains("sftp-2"));
    }

    #[test]
    fn dispatcher_queued_cancel_emits_cancelled_without_uploading() {
        // #3: a session cancelled before the dispatcher pulls its job must be
        // reported Cancelled and never reach spawn_upload. We exercise the
        // dispatcher's terminal-state path directly via finish_cancelled, which
        // is the same routine the dispatcher uses on a queued cancel.
        let (progress_tx, progress_rx) = crossbeam_channel::unbounded();
        let ctx = DispatcherCtx {
            job_rx: crossbeam_channel::unbounded().1,
            manager: Arc::new(SftpProcessManager::new()),
            pool: Arc::new(ConcurrentUploadPool::new(2)),
            progress_tx,
            session_tab: Arc::new(Mutex::new(HashMap::new())),
            cancelled: Arc::new(Mutex::new(HashSet::new())),
            sftp_binary: String::new(),
        };
        ctx.session_tab
            .lock()
            .unwrap()
            .insert("sftp-1".to_string(), 7);
        ctx.cancelled.lock().unwrap().insert("sftp-1".to_string());

        // slot_held = false: queued cancel never acquired a slot.
        finish_cancelled(&ctx, "sftp-1", "f.txt", 0, false);

        let p = progress_rx.try_recv().expect("a terminal progress event");
        assert_eq!(p.status, SftpUploadStatus::Cancelled);
        assert_eq!(p.session_id, "sftp-1");
        // Worker-owned cleanup ran: session_tab + cancelled cleared.
        assert!(ctx.session_tab.lock().unwrap().is_empty());
        assert!(ctx.cancelled.lock().unwrap().is_empty());
    }

    #[test]
    fn dispatcher_slot_held_cancel_releases_slot() {
        // A cancel detected *after* acquiring a slot must release it so the
        // pool does not leak capacity.
        let (progress_tx, progress_rx) = crossbeam_channel::unbounded();
        let pool = Arc::new(ConcurrentUploadPool::new(1));
        pool.acquire_slot("sftp-1");
        assert_eq!(pool.active_count(), 1);
        let ctx = DispatcherCtx {
            job_rx: crossbeam_channel::unbounded().1,
            manager: Arc::new(SftpProcessManager::new()),
            pool: Arc::clone(&pool),
            progress_tx,
            session_tab: Arc::new(Mutex::new(HashMap::new())),
            cancelled: Arc::new(Mutex::new(HashSet::new())),
            sftp_binary: String::new(),
        };

        finish_cancelled(&ctx, "sftp-1", "f.txt", 0, true);

        assert_eq!(pool.active_count(), 0, "slot must be released");
        let p = progress_rx.try_recv().expect("a terminal progress event");
        assert_eq!(p.status, SftpUploadStatus::Cancelled);
    }
}
