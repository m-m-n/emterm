//! Per-kind keyboard and focus rules.
//!
//! This module is the single source of truth for the FR5 table in
//! `doc/UI-DESIGN-GUIDELINES.yaml :: dialogs.keyboard` and
//! `dialogs.focus`. Both the rendering path (`Dialog::show`) and the
//! drift / kind-rule introspection tests read these rules through the
//! same functions.

/// Which role a key event targets, after the helper resolves it for the
/// dialog kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Primary,
    Cancel,
}

/// Dialog kind. Used by the builder to pick keyboard / focus rules and
/// to gate the destructive button color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    /// Text / number / select editing. Focus lands on the first body
    /// widget; Enter confirms (text inputs go through `lost_focus +
    /// key_pressed(Enter)` for IME safety; the helper exposes
    /// non-input Enter through `ui.input`).
    Input,
    /// Non-destructive confirmation. Focus lands on the primary
    /// button; Enter confirms.
    Confirm,
    /// Destructive confirmation. Focus lands on cancel; Enter targets
    /// cancel so a stray Enter never destroys data. Tab still reaches
    /// the primary button (Q4 default).
    DestructiveConfirm,
}

/// Initial focus rule for the given dialog kind.
pub fn initial_focus(kind: DialogKind) -> Target {
    match kind {
        DialogKind::Input => Target::Primary, // placeholder: caller provides explicit body-widget id
        DialogKind::Confirm => Target::Primary,
        DialogKind::DestructiveConfirm => Target::Cancel,
    }
}

/// Which role bare Enter (without IME composition) maps to for the
/// given kind. Note: for `Input`, the helper additionally honors the
/// `lost_focus + key_pressed(Enter)` pattern on text widgets so IME
/// commits do not steal the primary action; the result here is the
/// fallback when no text widget is focused.
pub fn enter_target(kind: DialogKind) -> Target {
    match kind {
        DialogKind::Input => Target::Primary,
        DialogKind::Confirm => Target::Primary,
        DialogKind::DestructiveConfirm => Target::Cancel,
    }
}

/// Esc always maps to cancel across kinds (`dialogs.keyboard.*.escape:
/// cancel`).
pub fn escape_target(_kind: DialogKind) -> Target {
    Target::Cancel
}
