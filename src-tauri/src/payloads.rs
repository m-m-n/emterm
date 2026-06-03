use serde::{Deserialize, Serialize};

use crate::image;

/// Result returned from pty_spawn command.
#[derive(Serialize, Deserialize)]
pub struct SpawnResult {
    pub session_id: String,
}

/// Payload for pty_exit event.
#[derive(Serialize, Clone)]
pub struct PtyExitPayload {
    pub session_id: String,
    pub code: i32,
    /// Number of remaining sessions after this session is removed.
    /// Used by frontend to determine if window should close.
    pub remaining_sessions: usize,
}

/// Payload for pty_error event.
#[derive(Serialize, Clone)]
pub struct PtyErrorPayload {
    pub session_id: String,
    pub message: String,
}

/// Payload for tab_created event.
#[derive(Serialize, Clone)]
pub struct TabCreatedPayload {
    pub session_id: String,
}

/// Payload for tab_closed event.
#[derive(Serialize, Clone)]
pub struct TabClosedPayload {
    pub session_id: String,
    pub exit_code: i32,
}

/// Payload for tab_count_changed event.
#[derive(Serialize, Clone)]
pub struct TabCountChangedPayload {
    pub count: usize,
}

/// Payload for the `osc_notification` event.
///
/// Emitted from the reader thread when an `OSC 9 ; <message>` desktop
/// notification is recognized on the background (hidden) processing path.
/// The frontend listener fires the OS desktop notification via the existing
/// `sendNotification` sink (permission-gated). Notifications are
/// fire-and-forget side effects and are NOT part of the resume/replay stream.
#[derive(Serialize, Clone)]
pub struct OscNotificationPayload {
    pub session_id: String,
    pub message: String,
}

/// Payload for image_event IPC channel.
///
/// Wraps an `ImageEvent` with the associated session ID for routing
/// events to the correct terminal session in the frontend.
#[derive(Serialize, Clone)]
pub struct ImageEventPayload {
    /// Session ID for event routing.
    pub session_id: String,

    /// The image event.
    #[serde(flatten)]
    pub event: image::ImageEvent,
}
