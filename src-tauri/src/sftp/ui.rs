//! SFTP egui UI state and drop-path handling.
//!
//! winit's `WindowEvent::DragDropped` delivers the full set of dropped paths
//! in a single event (one drag session = one event), so [`drop_batch_from_paths`]
//! maps that list directly onto the existing upload entry point — no
//! cross-event accumulation is needed. The rest of this module holds the
//! overlay / dialog / toast state machine that the egui render path draws
//! and the progress pump updates.

use std::path::PathBuf;

use crate::sftp::service::SftpConnection;
use crate::sftp::{SftpUploadProgress, SftpUploadStatus};

/// How long (in egui frame-time seconds) a terminal-state toast lingers before
/// it is auto-dismissed. Frame time is monotonic and wall-clock-free.
pub const TOAST_LINGER_SECS: f64 = 4.0;

/// A single upload's progress toast.
#[derive(Debug, Clone)]
pub struct Toast {
    pub session_id: String,
    pub file_name: String,
    pub status: SftpUploadStatus,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub error_message: Option<String>,
    /// Frame-time at which a terminal-state toast should be removed. `None`
    /// while the upload is still in a non-terminal state.
    pub dismiss_at: Option<f64>,
}

impl Toast {
    fn is_terminal(status: &SftpUploadStatus) -> bool {
        matches!(
            status,
            SftpUploadStatus::Completed | SftpUploadStatus::Failed | SftpUploadStatus::Cancelled
        )
    }
}

/// Holds every visible progress toast and applies progress events to them.
#[derive(Debug, Default)]
pub struct ToastList {
    pub toasts: Vec<Toast>,
}

impl ToastList {
    /// Apply one progress event: update an existing toast for the session, or
    /// insert a new one. Terminal states schedule auto-dismissal at
    /// `now + TOAST_LINGER_SECS`.
    pub fn apply(&mut self, p: SftpUploadProgress, now: f64) {
        let dismiss_at = if Toast::is_terminal(&p.status) {
            Some(now + TOAST_LINGER_SECS)
        } else {
            None
        };
        if let Some(t) = self
            .toasts
            .iter_mut()
            .find(|t| t.session_id == p.session_id)
        {
            t.file_name = p.file_name;
            t.status = p.status;
            t.bytes_transferred = p.bytes_transferred;
            t.total_bytes = p.total_bytes;
            t.error_message = p.error_message;
            t.dismiss_at = dismiss_at;
        } else {
            self.toasts.push(Toast {
                session_id: p.session_id,
                file_name: p.file_name,
                status: p.status,
                bytes_transferred: p.bytes_transferred,
                total_bytes: p.total_bytes,
                error_message: p.error_message,
                dismiss_at,
            });
        }
    }

    /// Remove toasts whose auto-dismiss frame-time has elapsed.
    pub fn prune_expired(&mut self, now: f64) {
        self.toasts
            .retain(|t| t.dismiss_at.map(|at| now < at).unwrap_or(true));
    }

    /// Drop the toast for a session (e.g. after the user hits cancel).
    pub fn remove(&mut self, session_id: &str) {
        self.toasts.retain(|t| t.session_id != session_id);
    }
}

/// The destination kind for an aggregated drop, decided by the active tab.
#[derive(Debug, Clone, PartialEq)]
pub enum DropTarget {
    /// SSH tab → open the upload dialog with a pre-filled remote directory.
    SshUpload { remote_dir: String },
    /// Non-SSH tab → paste the formatted local paths into the terminal.
    Paste,
}

/// The upload-confirmation dialog state.
///
/// The originating tab's `stable_id` and its **resolved** `SftpConnection` are
/// captured at drop time so the confirm / overwrite / duplicate-check paths do
/// not re-read the (possibly changed) active tab when the dialog resolves.
#[derive(Debug, Clone)]
pub struct UploadDialog {
    pub paths: Vec<PathBuf>,
    pub remote_dir: String,
    /// stable_id of the tab the drop originated on.
    pub tab_id: u64,
    /// Connection inputs resolved at drop time (identity capture).
    pub connection: SftpConnection,
}

/// The overwrite-confirmation dialog state.
#[derive(Debug, Clone)]
pub struct OverwriteDialog {
    pub paths: Vec<PathBuf>,
    pub remote_dir: String,
    pub duplicates: Vec<String>,
    /// stable_id of the tab the drop originated on.
    pub tab_id: u64,
    /// Connection inputs resolved at drop time (identity capture).
    pub connection: SftpConnection,
}

/// What a duplicate-check result should drive next.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmOutcome {
    /// No duplicates: start the uploads directly.
    StartUploads,
    /// Duplicates found: open the overwrite dialog with these names.
    OpenOverwrite(Vec<String>),
}

/// Decide what a duplicate-check result drives: a non-empty duplicate set opens
/// the overwrite dialog; an empty set proceeds straight to upload.
pub fn confirm_branch(duplicates: Vec<String>) -> ConfirmOutcome {
    if duplicates.is_empty() {
        ConfirmOutcome::StartUploads
    } else {
        ConfirmOutcome::OpenOverwrite(duplicates)
    }
}

/// The hover overlay shown while files are dragged over the window.
#[derive(Debug, Clone, PartialEq)]
pub enum HoverOverlay {
    /// Active tab is an SSH tab → the drop will start an upload.
    SshUpload,
    /// Active tab is not an SSH tab → the drop will paste local paths.
    Paste,
}

/// Aggregate SFTP UI state held by `App` and drawn by the render path.
#[derive(Default)]
pub struct SftpUiState {
    /// The hover overlay, when a drag is in progress.
    pub hover: Option<HoverOverlay>,
    /// The active upload-confirmation dialog, if any.
    pub upload_dialog: Option<UploadDialog>,
    /// The active overwrite-confirmation dialog, if any.
    pub overwrite_dialog: Option<OverwriteDialog>,
    /// Visible progress toasts.
    pub toasts: ToastList,
    /// Monotonic request-id source for off-thread duplicate checks. Pairs a
    /// `DuplicateCheckResult` back with the dialog that requested it.
    next_request: u64,
    /// Dialogs awaiting a duplicate-check result, keyed by request id. A map
    /// (not a single slot) so concurrent duplicate checks are not dropped.
    pub pending_check: std::collections::HashMap<u64, UploadDialog>,
    /// A pending tab-close blocked on active uploads. Holds the **stable_id**
    /// of the tab the user asked to close (not its index, which can shift when
    /// other tabs are added/removed/reordered while the dialog is open). The
    /// confirmation dialog resolves the id back to an index and cancels that
    /// tab's uploads before closing it.
    pub close_guard: Option<u64>,
}

impl SftpUiState {
    /// Allocate the next duplicate-check request id (no wall-clock).
    pub fn next_request_id(&mut self) -> u64 {
        self.next_request += 1;
        self.next_request
    }
}

/// Map a winit `WindowEvent::DragDropped` path list onto the SFTP drop
/// entry point (FR3 / IMPLEMENTATION.md D3).
///
/// `paths` is fed to [`crate::app::App::dispatch_drop`] in list order,
/// preserved verbatim (`DragDropped` already delivers the whole drag
/// session's paths in one event, in drop order); an empty list is a
/// no-op (`None`).
pub fn drop_batch_from_paths(paths: Vec<PathBuf>) -> Option<Vec<PathBuf>> {
    if paths.is_empty() { None } else { Some(paths) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress(session: &str, status: SftpUploadStatus) -> SftpUploadProgress {
        SftpUploadProgress {
            session_id: session.to_string(),
            file_name: "f.txt".to_string(),
            bytes_transferred: 0,
            total_bytes: 100,
            status,
            error_message: None,
        }
    }

    #[test]
    fn toast_insert_then_update_same_session() {
        let mut list = ToastList::default();
        list.apply(progress("s1", SftpUploadStatus::Preparing), 0.0);
        assert_eq!(list.toasts.len(), 1);
        assert_eq!(list.toasts[0].status, SftpUploadStatus::Preparing);
        assert_eq!(list.toasts[0].dismiss_at, None);

        // Same session updates in place (no second toast).
        list.apply(progress("s1", SftpUploadStatus::Uploading), 1.0);
        assert_eq!(list.toasts.len(), 1);
        assert_eq!(list.toasts[0].status, SftpUploadStatus::Uploading);
        assert_eq!(list.toasts[0].dismiss_at, None);
    }

    #[test]
    fn toast_terminal_state_schedules_dismiss() {
        let mut list = ToastList::default();
        list.apply(progress("s1", SftpUploadStatus::Uploading), 0.0);
        list.apply(progress("s1", SftpUploadStatus::Completed), 10.0);
        assert_eq!(list.toasts[0].dismiss_at, Some(10.0 + TOAST_LINGER_SECS));
    }

    #[test]
    fn toast_prune_removes_only_expired() {
        let mut list = ToastList::default();
        list.apply(progress("done", SftpUploadStatus::Completed), 0.0);
        list.apply(progress("live", SftpUploadStatus::Uploading), 0.0);

        // Before the linger window elapses, nothing is pruned.
        list.prune_expired(TOAST_LINGER_SECS - 0.1);
        assert_eq!(list.toasts.len(), 2);

        // After it elapses, the terminal toast goes; the live one stays.
        list.prune_expired(TOAST_LINGER_SECS + 0.1);
        assert_eq!(list.toasts.len(), 1);
        assert_eq!(list.toasts[0].session_id, "live");
    }

    #[test]
    fn toast_remove_drops_session() {
        let mut list = ToastList::default();
        list.apply(progress("s1", SftpUploadStatus::Uploading), 0.0);
        list.remove("s1");
        assert!(list.toasts.is_empty());
    }

    #[test]
    fn confirm_branch_no_duplicates_starts_upload() {
        assert_eq!(confirm_branch(vec![]), ConfirmOutcome::StartUploads);
    }

    #[test]
    fn confirm_branch_with_duplicates_opens_overwrite() {
        let dups = vec!["a.txt".to_string(), "b.txt".to_string()];
        assert_eq!(
            confirm_branch(dups.clone()),
            ConfirmOutcome::OpenOverwrite(dups)
        );
    }

    // ── drop_batch_from_paths (AC-4) ─────────────────────────────────────

    #[test]
    fn drop_batch_empty_list_is_noop() {
        assert_eq!(drop_batch_from_paths(Vec::new()), None);
    }

    #[test]
    fn drop_batch_non_empty_list_preserves_order() {
        let paths = vec![
            PathBuf::from("/a/one.txt"),
            PathBuf::from("/a/two.txt"),
            PathBuf::from("/a/three.txt"),
        ];
        let batch = drop_batch_from_paths(paths.clone()).expect("a batch");
        assert_eq!(batch, paths);
    }

    #[test]
    fn drop_batch_single_path() {
        let paths = vec![PathBuf::from("/a/one.txt")];
        assert_eq!(drop_batch_from_paths(paths.clone()), Some(paths));
    }

    fn upload_dialog(remote: &str) -> UploadDialog {
        UploadDialog {
            paths: vec![PathBuf::from("/a/f.txt")],
            remote_dir: remote.to_string(),
            tab_id: 1,
            connection: SftpConnection {
                hostname: "h".to_string(),
                port: 22,
                username: String::new(),
                identity_file: String::new(),
                ssh_options: Vec::new(),
            },
        }
    }

    #[test]
    fn pending_check_keeps_concurrent_requests_distinct() {
        // #13: two duplicate checks in flight must each be retrievable by their
        // own request id; resolving one must not clobber the other.
        let mut ui = SftpUiState::default();
        let r1 = ui.next_request_id();
        let r2 = ui.next_request_id();
        assert_ne!(r1, r2);

        ui.pending_check.insert(r1, upload_dialog("/one"));
        ui.pending_check.insert(r2, upload_dialog("/two"));
        assert_eq!(ui.pending_check.len(), 2);

        // Resolving r1 removes only r1; r2 still pending.
        let d1 = ui.pending_check.remove(&r1).expect("r1 present");
        assert_eq!(d1.remote_dir, "/one");
        assert!(ui.pending_check.contains_key(&r2));
        assert_eq!(ui.pending_check.len(), 1);

        let d2 = ui.pending_check.remove(&r2).expect("r2 present");
        assert_eq!(d2.remote_dir, "/two");
        assert!(ui.pending_check.is_empty());
    }

    #[test]
    fn pending_check_unknown_request_is_ignored() {
        // A stale/superseded result whose id is not in the map must be a clean
        // no-op (the pump just `continue`s).
        let mut ui = SftpUiState::default();
        let r1 = ui.next_request_id();
        ui.pending_check.insert(r1, upload_dialog("/one"));
        assert!(ui.pending_check.remove(&999).is_none());
        // The real pending check is untouched.
        assert!(ui.pending_check.contains_key(&r1));
    }

    #[test]
    fn next_request_id_is_strictly_increasing() {
        let mut ui = SftpUiState::default();
        let a = ui.next_request_id();
        let b = ui.next_request_id();
        let c = ui.next_request_id();
        assert!(a < b && b < c);
    }
}
