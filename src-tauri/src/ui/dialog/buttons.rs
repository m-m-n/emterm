//! Role-colored button helpers for modal dialogs.
//!
//! The `Dialog` helper draws each action button via [`draw_role`], which
//! applies the MD3 color pair from FR6 (primary, cancel, destructive)
//! through the active [`crate::ui::md3`] preset accessors. Caller code
//! never picks colors by hand.

use egui::{Color32, RichText};

use crate::ui::md3;

/// Which dialog action a button represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonRole {
    /// Primary (verb) button — the action the dialog exists to confirm.
    Primary,
    /// Cancel (transparent) button — the universal escape hatch.
    Cancel,
    /// Destructive primary — used only inside
    /// [`crate::ui::dialog::DialogKind::DestructiveConfirm`] for the
    /// data-destroying action. Filled with `error-container`.
    Destructive,
}

impl ButtonRole {
    fn background(self) -> Color32 {
        match self {
            ButtonRole::Primary => md3::primary(),
            ButtonRole::Cancel => Color32::TRANSPARENT,
            ButtonRole::Destructive => md3::error_container(),
        }
    }

    fn foreground(self) -> Color32 {
        match self {
            ButtonRole::Primary => md3::on_primary(),
            ButtonRole::Cancel => md3::primary(),
            ButtonRole::Destructive => md3::on_error_container(),
        }
    }
}

/// Draw a labeled button with role-specific MD3 colors. Returns the
/// `egui::Response` so the caller can detect clicks and apply
/// first-frame focus when needed.
pub fn draw_role(ui: &mut egui::Ui, role: ButtonRole, label: &str) -> egui::Response {
    let fg = role.foreground();
    let bg = role.background();
    let rich = RichText::new(label).color(fg);
    let button = egui::Button::new(rich).fill(bg);
    ui.add(button)
}
