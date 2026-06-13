//! SFTP file upload module (native-poc port).
//!
//! Provides SFTP subprocess management for file uploads via drag & drop.
//! Uses the external `sftp` command, reusing SSH connection settings.
//!
//! Ported from `src-tauri/src/sftp/*`. The Tauri-specific IPC/command layer
//! is replaced by an in-process [`service::SftpService`] that reports progress
//! over a crossbeam channel; the pure logic below is behavior-preserving.
//!
//! A few ported items are intentionally retained but unused in native-poc:
//! the interactive-mode progress parser (`progress::parse_progress_line` and
//! friends — batch mode `-b -` suppresses progress bars, see the source note)
//! and the pool/service introspection helpers kept for parity and tests. They
//! are covered by their ported unit tests, so `dead_code` is allowed
//! module-wide rather than scattering per-item attributes.
#![allow(dead_code)]

pub mod args;
pub mod check;
pub mod pool;
pub mod process;
pub mod progress;
pub mod remote_path;
pub mod service;
pub mod ui;

use serde::{Deserialize, Serialize};

/// Status of an SFTP upload session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SftpUploadStatus {
    Preparing,
    Uploading,
    Completed,
    Failed,
    Cancelled,
}

/// Progress payload emitted during file transfer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SftpUploadProgress {
    pub session_id: String,
    pub file_name: String,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub status: SftpUploadStatus,
    pub error_message: Option<String>,
}
