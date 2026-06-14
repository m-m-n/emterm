//! UI-layer rendering for the mux rename / move dialogs.
//!
//! The domain layer (`App`) owns the data via
//! [`crate::mux::dialog::MuxDialogState`]. This module is the *only* place
//! that knows about `egui::Context` for these dialogs — keeps the App
//! free of egui imports for the dialog path (gpt-architecture #3).
//!
//! Per-frame contract: `draw(state, ctx)` reads / mutates the plain state
//! enum, draws the modal via egui, and returns a [`MuxDialogOutcome`]. The
//! render pipeline interprets the outcome and dispatches it back to the
//! domain layer (`App::confirm_mux_*`).

use crate::mux::dialog::{MuxDialogOutcome, MuxDialogState};

/// Render the currently-open mux dialog (if any) for this frame.
///
/// Returns:
/// - [`MuxDialogOutcome::Pending`] when no dialog is open OR the dialog is
///   still awaiting user input. The caller takes no action.
/// - [`MuxDialogOutcome::ConfirmRename`] / [`MuxDialogOutcome::ConfirmMove`]
///   on user confirm. The caller must clear `state` to [`MuxDialogState::Closed`]
///   and dispatch the confirm to the domain layer (`App::confirm_mux_*`).
/// - [`MuxDialogOutcome::Cancelled`] on Esc / Cancel / empty-confirm. The
///   caller must clear `state` to [`MuxDialogState::Closed`].
pub fn draw(state: &mut MuxDialogState, ctx: &egui::Context) -> MuxDialogOutcome {
    match state {
        MuxDialogState::Closed => MuxDialogOutcome::Pending,
        MuxDialogState::Rename { .. } => draw_rename(state, ctx),
        MuxDialogState::Move { .. } => draw_move(state, ctx),
    }
}

fn draw_rename(state: &mut MuxDialogState, ctx: &egui::Context) -> MuxDialogOutcome {
    let MuxDialogState::Rename {
        window_id,
        name,
        focused_once,
    } = state
    else {
        return MuxDialogOutcome::Pending;
    };
    let captured_id = *window_id;
    let mut outcome = MuxDialogOutcome::Pending;
    egui::Window::new("ウィンドウ名を変更")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            let resp = ui.text_edit_singleline(name);
            if !*focused_once {
                resp.request_focus();
                *focused_once = true;
            }
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                outcome = resolve_rename_confirm(captured_id, name);
            }
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    outcome = resolve_rename_confirm(captured_id, name);
                }
                if ui.button("キャンセル").clicked() {
                    outcome = MuxDialogOutcome::Cancelled;
                }
            });
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                outcome = MuxDialogOutcome::Cancelled;
            }
        });
    outcome
}

fn resolve_rename_confirm(window_id: u32, name: &str) -> MuxDialogOutcome {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        MuxDialogOutcome::Cancelled
    } else {
        MuxDialogOutcome::ConfirmRename {
            window_id,
            name: trimmed.to_string(),
        }
    }
}

fn draw_move(state: &mut MuxDialogState, ctx: &egui::Context) -> MuxDialogOutcome {
    let MuxDialogState::Move {
        window_id,
        current_position,
        window_count,
        target,
    } = state
    else {
        return MuxDialogOutcome::Pending;
    };
    let captured_id = *window_id;
    let cur = *current_position;
    let count = *window_count;
    let mut outcome = MuxDialogOutcome::Pending;
    egui::Window::new("ウィンドウを移動")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.label(format!("現在: {cur} / {count} 個中"));
            ui.horizontal(|ui| {
                ui.label("移動先:");
                let mut t = *target as i64;
                ui.add(egui::DragValue::new(&mut t).range(1..=(count as i64)));
                *target = t.clamp(1, count as i64) as usize;
            });
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    outcome = resolve_move_confirm(captured_id, *target, cur, count);
                }
                if ui.button("キャンセル").clicked() {
                    outcome = MuxDialogOutcome::Cancelled;
                }
            });
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                outcome = resolve_move_confirm(captured_id, *target, cur, count);
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                outcome = MuxDialogOutcome::Cancelled;
            }
        });
    outcome
}

fn resolve_move_confirm(
    window_id: u32,
    target: usize,
    current_position: usize,
    window_count: usize,
) -> MuxDialogOutcome {
    if target < 1 || target > window_count || target == current_position {
        MuxDialogOutcome::Cancelled
    } else {
        MuxDialogOutcome::ConfirmMove { window_id, target }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_rename_trims_and_rejects_empty() {
        assert_eq!(
            resolve_rename_confirm(1, "  vim  "),
            MuxDialogOutcome::ConfirmRename {
                window_id: 1,
                name: "vim".to_string()
            }
        );
        assert_eq!(
            resolve_rename_confirm(1, "   "),
            MuxDialogOutcome::Cancelled
        );
        assert_eq!(resolve_rename_confirm(1, ""), MuxDialogOutcome::Cancelled);
    }

    #[test]
    fn resolve_move_rejects_out_of_range_and_same_position() {
        assert_eq!(
            resolve_move_confirm(3, 4, 2, 5),
            MuxDialogOutcome::ConfirmMove {
                window_id: 3,
                target: 4
            }
        );
        // Same position
        assert_eq!(
            resolve_move_confirm(3, 2, 2, 5),
            MuxDialogOutcome::Cancelled
        );
        // Out of range
        assert_eq!(
            resolve_move_confirm(3, 0, 2, 5),
            MuxDialogOutcome::Cancelled
        );
        assert_eq!(
            resolve_move_confirm(3, 6, 2, 5),
            MuxDialogOutcome::Cancelled
        );
    }
}
