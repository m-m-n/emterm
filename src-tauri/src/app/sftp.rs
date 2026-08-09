//! SFTP drag & drop upload flow for [`App`].

use super::App;

impl App {
    // ── SFTP upload (drag & drop) ────────────────────────────────

    /// Drain the SFTP progress + duplicate-check channels and update the UI.
    /// `now` is the current egui frame time (monotonic, wall-clock-free).
    /// Returns true when any toast/dialog state changed (so the caller can
    /// request a redraw).
    pub fn pump_sftp(&mut self, now: f64) -> bool {
        // The binary-mismatch restart toast shares this per-frame pump but is
        // an independent concern (see `pump_restart_toast`).
        let mut changed = self.pump_restart_toast(now);

        // Progress events → toasts.
        while let Ok(progress) = self.sftp_progress_rx.try_recv() {
            self.sftp_ui.toasts.apply(progress, now);
            changed = true;
        }
        // Auto-dismiss elapsed terminal toasts.
        let before = self.sftp_ui.toasts.toasts.len();
        self.sftp_ui.toasts.prune_expired(now);
        if self.sftp_ui.toasts.toasts.len() != before {
            changed = true;
        }

        // Duplicate-check results → overwrite dialog or direct upload.
        // pending_check is a map keyed by request_id so concurrent checks are
        // not clobbered (#13); a result with no matching entry (already
        // superseded/consumed) is simply ignored.
        while let Ok(result) = self.sftp_result_rx.try_recv() {
            changed = true;
            let Some(dialog) = self.sftp_ui.pending_check.remove(&result.request_id) else {
                continue;
            };
            match result.outcome {
                Ok(duplicates) => match crate::sftp::ui::confirm_branch(duplicates) {
                    crate::sftp::ui::ConfirmOutcome::StartUploads => {
                        self.start_uploads_with(
                            now,
                            dialog.tab_id,
                            dialog.connection,
                            dialog.paths,
                            dialog.remote_dir,
                        );
                    }
                    crate::sftp::ui::ConfirmOutcome::OpenOverwrite(dups) => {
                        self.sftp_ui.overwrite_dialog = Some(crate::sftp::ui::OverwriteDialog {
                            paths: dialog.paths,
                            remote_dir: dialog.remote_dir,
                            duplicates: dups,
                            tab_id: dialog.tab_id,
                            connection: dialog.connection,
                        });
                    }
                },
                Err(_e) => {
                    // The remote listing failed. Do NOT silently fall through to
                    // an upload (#12): surface the failure as a toast and abort,
                    // so an unverified destination never receives an implicit
                    // overwrite.
                    self.push_sftp_error_toast(
                        now,
                        "重複チェックに失敗したためアップロードを中止しました",
                        "Upload aborted: duplicate check failed",
                    );
                }
            }
        }

        changed
    }

    /// Route an aggregated drop batch by the active tab kind. Returns the
    /// drop target chosen (so the caller / tests can assert the branch).
    pub fn dispatch_drop(&mut self, paths: Vec<std::path::PathBuf>) -> crate::sftp::ui::DropTarget {
        // Capture the originating tab's identity (stable_id + resolved
        // connection) at drop time so the later confirm/overwrite paths do not
        // re-read the active tab, which may have changed.
        let identity = self.active_tab().filter(|t| t.is_ssh_tab()).and_then(|t| {
            let id = t.stable_id;
            t.ssh_connection(&self.settings)
                .map(crate::sftp::service::SftpConnection::from_ssh_connection)
                .map(|conn| (id, conn))
        });
        if let Some((tab_id, connection)) = identity {
            let remote_dir = self.active_tab_remote_dir();
            self.sftp_ui.upload_dialog = Some(crate::sftp::ui::UploadDialog {
                paths,
                remote_dir: remote_dir.clone(),
                tab_id,
                connection,
            });
            crate::sftp::ui::DropTarget::SshUpload { remote_dir }
        } else {
            let strs: Vec<String> = paths
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect();
            let line = crate::sftp::remote_path::format_paths_for_paste(&strs);
            if let Some(tab) = self.active_tab() {
                tab.write(line.into_bytes());
            }
            crate::sftp::ui::DropTarget::Paste
        }
    }

    /// The remote upload directory derived from the active tab's OSC 7 CWD.
    fn active_tab_remote_dir(&self) -> String {
        let cwd = self
            .active_tab()
            .and_then(|tab| tab.cb_state.lock().cwd.clone())
            .unwrap_or_default();
        crate::sftp::remote_path::extract_remote_path(&cwd)
    }

    /// Confirm the upload dialog: request an off-thread duplicate check for the
    /// pending paths. The result channel pump branches to overwrite or upload.
    ///
    /// Uses the connection/tab identity captured in the dialog at drop time —
    /// it never re-reads the active tab — so switching tabs between drop and
    /// confirm cannot redirect the upload (#4).
    pub fn confirm_upload_dialog(&mut self, now: f64) {
        let Some(dialog) = self.sftp_ui.upload_dialog.take() else {
            return;
        };
        // Guard: the originating tab must still exist and still be an SSH tab.
        if !self.tab_is_still_ssh(dialog.tab_id) {
            self.push_sftp_error_toast(
                now,
                "アップロード対象のタブが見つかりません",
                "Upload target tab is no longer available",
            );
            return;
        }
        let file_names: Vec<String> = dialog
            .paths
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .collect();
        let req_id = self.sftp_ui.next_request_id();
        let connection = dialog.connection.clone();
        let remote_dir = dialog.remote_dir.clone();
        self.sftp_ui.pending_check.insert(req_id, dialog);
        self.sftp_service
            .check_duplicates(req_id, connection, remote_dir, file_names);
    }

    /// Confirm the overwrite dialog: start the uploads despite the duplicates,
    /// using the dialog's captured identity (not the active tab).
    pub fn confirm_overwrite_dialog(&mut self, now: f64) {
        let Some(dialog) = self.sftp_ui.overwrite_dialog.take() else {
            return;
        };
        self.start_uploads_with(
            now,
            dialog.tab_id,
            dialog.connection,
            dialog.paths,
            dialog.remote_dir,
        );
    }

    /// Cancel a running upload (toast cancel control).
    pub fn cancel_sftp_upload(&mut self, session_id: &str) {
        self.sftp_service.cancel(session_id);
        self.sftp_ui.toasts.remove(session_id);
    }

    /// Whether the tab with `tab_id` still exists and is still an SSH tab.
    fn tab_is_still_ssh(&self, tab_id: u64) -> bool {
        self.tabs
            .iter()
            .find(|t| t.stable_id == tab_id)
            .map(|t| t.is_ssh_tab())
            .unwrap_or(false)
    }

    /// Surface an SFTP error to the user as a transient failure toast. Uses a
    /// synthetic session id so it slots into the same toast stack and
    /// auto-dismisses like a real terminal-state toast.
    fn push_sftp_error_toast(&mut self, now: f64, ja: &'static str, en: &'static str) {
        let msg = match self.locale {
            crate::i18n::Locale::Ja => ja,
            crate::i18n::Locale::En => en,
        };
        let session_id = format!("sftp-error-{}", self.sftp_ui.next_request_id());
        self.sftp_ui.toasts.apply(
            crate::sftp::SftpUploadProgress {
                session_id,
                file_name: msg.to_string(),
                bytes_transferred: 0,
                total_bytes: 0,
                status: crate::sftp::SftpUploadStatus::Failed,
                error_message: Some(msg.to_string()),
            },
            now,
        );
    }

    /// Start one upload per path in the batch against the captured identity.
    /// The originating tab is re-validated (existence + still SSH); if it is
    /// gone the batch is dropped with an error toast instead of being
    /// redirected to whatever tab happens to be active.
    fn start_uploads_with(
        &mut self,
        now: f64,
        tab_id: u64,
        connection: crate::sftp::service::SftpConnection,
        paths: Vec<std::path::PathBuf>,
        remote_dir: String,
    ) {
        if !self.tab_is_still_ssh(tab_id) {
            self.push_sftp_error_toast(
                now,
                "アップロード対象のタブが見つかりません",
                "Upload target tab is no longer available",
            );
            return;
        }
        for path in paths {
            let is_directory = crate::sftp::remote_path::is_directory(&path);
            let local_path = path.to_string_lossy().to_string();
            let _ = self.sftp_service.start_upload(
                tab_id,
                connection.clone(),
                local_path,
                remote_dir.clone(),
                is_directory,
            );
        }
    }
}
