//! Domain-layer state for the mux rename / move dialogs.
//!
//! `App` owns one of these (`app.mux_dialog`) instead of importing any
//! egui-flavoured widget struct. Drawing happens in `ui::mux_dialogs::draw`,
//! which takes `&mut MuxDialogState` and an `&egui::Context` and returns a
//! plain [`MuxDialogOutcome`]. That keeps the domain layer free of egui
//! types (gpt-architecture #3); the UI layer is the only place that knows
//! about `egui::Context`.

/// Currently-open mux dialog, if any. Holds only plain data; the
/// transient "have we grabbed focus once" flag is intentionally part of
/// the state (a single boolean, NOT an egui type) so the data → view
/// projection is stateless across frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MuxDialogState {
    /// No dialog open.
    Closed,
    /// Rename-window dialog. The stable `window_id` is re-resolved on
    /// confirm so the rename targets the right window even if the active
    /// index shifted.
    Rename {
        window_id: u32,
        /// Current text-field contents (seeded with the window name).
        name: String,
        /// Whether the field has been focused once (egui needs an explicit
        /// `request_focus()` on the first frame the widget is shown).
        focused_once: bool,
    },
    /// Move-window dialog. Captures a 1-based target in `[1, window_count]`.
    Move {
        window_id: u32,
        /// 1-based current position (display only).
        current_position: usize,
        /// Total window count (validation upper bound).
        window_count: usize,
        /// 1-based target the user is editing.
        target: usize,
    },
}

impl MuxDialogState {
    /// Whether *any* dialog is currently open. Used by the input layer to
    /// route keys to the dialog instead of the PTY, and by `apply_tab_event`
    /// to suppress tab-bar mouse clicks (so the dialog cannot retarget
    /// itself against a different active tab — claude-comprehensive #1).
    pub fn is_open(&self) -> bool {
        !matches!(self, Self::Closed)
    }
}

impl Default for MuxDialogState {
    fn default() -> Self {
        Self::Closed
    }
}

/// Outcome of one draw pass from `ui::mux_dialogs::draw`. The render
/// pipeline interprets this and calls back into the `App` confirm methods;
/// the dialog code itself never touches `App` directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MuxDialogOutcome {
    /// Still open, awaiting input.
    Pending,
    /// User confirmed a rename with this (non-empty, trimmed) name.
    ConfirmRename { window_id: u32, name: String },
    /// User confirmed a move to this 1-based target.
    ConfirmMove { window_id: u32, target: usize },
    /// User cancelled (Esc / Cancel button) or entered an empty / no-op
    /// value that the dialog treats as cancel.
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_is_not_open() {
        assert!(!MuxDialogState::Closed.is_open());
    }

    #[test]
    fn rename_and_move_are_open() {
        let r = MuxDialogState::Rename {
            window_id: 1,
            name: String::new(),
            focused_once: false,
        };
        let m = MuxDialogState::Move {
            window_id: 2,
            current_position: 1,
            window_count: 3,
            target: 1,
        };
        assert!(r.is_open());
        assert!(m.is_open());
    }
}
