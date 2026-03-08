//! SFTP file upload module.
//!
//! Provides SFTP subprocess management for file uploads via drag & drop.
//! Uses the external `sftp` command, reusing SSH connection settings.

pub mod args;
pub mod check;
pub mod pool;
pub mod progress;
pub mod upload;

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
