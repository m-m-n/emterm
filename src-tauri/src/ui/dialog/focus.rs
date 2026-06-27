//! First-frame focus helper.
//!
//! egui has no built-in "focus this widget exactly once on the frame it
//! appears" primitive; the long-standing pattern is to track a
//! `focused_once: bool` on the dialog's state. This helper wraps that
//! pattern so caller code doesn't repeat the same two-line check.

/// State for "request focus once" semantics. Construct one per dialog
/// instance and call [`FirstFrameFocus::request_once`] each frame; it
/// fires `request_focus()` on the response on the first frame only.
#[derive(Debug, Default, Clone, Copy)]
pub struct FirstFrameFocus {
    requested: bool,
}

impl FirstFrameFocus {
    pub fn new() -> Self {
        Self { requested: false }
    }

    /// Reset to the unfocused state. Use this when the dialog is closed
    /// and re-opened (e.g. switching between rename / move dialogs).
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.requested = false;
    }

    /// Returns `true` exactly once across all calls on this instance,
    /// signaling the caller to call `request_focus()` on its response.
    pub fn take(&mut self) -> bool {
        if self.requested {
            false
        } else {
            self.requested = true;
            true
        }
    }

    /// Convenience helper: if [`Self::take`] returns true, call
    /// `request_focus()` on the supplied response.
    #[allow(dead_code)]
    pub fn request_once(&mut self, response: &egui::Response) {
        if self.take() {
            response.request_focus();
        }
    }
}
