//! UI-layer rendering for the mux rename / move dialogs.
//!
//! The domain layer (`App`) owns the data via
//! [`crate::mux::dialog::MuxDialogState`]. This module is the *only* place
//! that knows about `egui::Context` for these dialogs — keeps the App
//! free of egui imports for the dialog path (gpt-architecture #3).
//!
//! Both dialogs route through the shared [`crate::ui::dialog::Dialog`]
//! builder so Window chrome, MD3 styling, role-colored buttons, and
//! keyboard rules stay consistent with the rest of the dialog system.

use std::cell::Cell;
use std::rc::Rc;

use crate::mux::dialog::{MuxDialogOutcome, MuxDialogState};
use crate::ui::dialog::{Dialog, DialogOutcome};

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
pub fn draw(
    state: &mut MuxDialogState,
    ctx: &egui::Context,
    locale: crate::i18n::Locale,
) -> MuxDialogOutcome {
    match state {
        MuxDialogState::Closed => MuxDialogOutcome::Pending,
        MuxDialogState::Rename { .. } => draw_rename(state, ctx, locale),
        MuxDialogState::Move { .. } => draw_move(state, ctx, locale),
    }
}

fn draw_rename(
    state: &mut MuxDialogState,
    ctx: &egui::Context,
    locale: crate::i18n::Locale,
) -> MuxDialogOutcome {
    let MuxDialogState::Rename { window_id, name } = state else {
        return MuxDialogOutcome::Pending;
    };
    let captured_id = *window_id;

    // Share the latest text-field snapshot between the body closure (which
    // writes it) and the on-confirm closure (which reads it). egui draws
    // single-threaded so there is no `Send + Sync` requirement; `Rc<Cell>`
    // is the minimal shape that satisfies the borrow checker.
    let snapshot: Rc<std::cell::RefCell<String>> = Rc::new(std::cell::RefCell::new(name.clone()));
    let snapshot_body = Rc::clone(&snapshot);
    let snapshot_confirm = Rc::clone(&snapshot);

    // Captured-once text-field id for the helper's first-frame focus.
    let text_field_id: Rc<Cell<Option<egui::Id>>> = Rc::new(Cell::new(None));
    let text_field_id_body = Rc::clone(&text_field_id);

    let outcome = {
        let name_ref: &mut String = name;
        Dialog::<MuxDialogOutcome>::input("ウィンドウ名を変更", "Rename Window", locale)
            .body(move |ui: &mut egui::Ui| {
                let resp = ui.text_edit_singleline(name_ref);
                if text_field_id_body.get().is_none() {
                    text_field_id_body.set(Some(resp.id));
                }
                *snapshot_body.borrow_mut() = name_ref.clone();
            })
            .primary_button("変更", "Rename", move || {
                resolve_rename_confirm(captured_id, &snapshot_confirm.borrow())
            })
            .show(ctx)
    };

    // First-frame focus on the text field. The helper's generic
    // initial-focus path takes a builder-time `egui::Id`; since the
    // text field's id is only known after the body runs, we request
    // focus directly from egui memory once it lands. Subsequent frames
    // are no-ops because `focused()` reports the field as focused.
    if let Some(id) = text_field_id.get() {
        ctx.memory_mut(|mem| {
            if mem.focused().is_none() {
                mem.request_focus(id);
            }
        });
    }

    translate_outcome(outcome)
}

fn translate_outcome(outcome: DialogOutcome<MuxDialogOutcome>) -> MuxDialogOutcome {
    match outcome {
        DialogOutcome::Pending => MuxDialogOutcome::Pending,
        DialogOutcome::Confirmed(value) => value,
        DialogOutcome::Cancelled => MuxDialogOutcome::Cancelled,
    }
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

fn draw_move(
    state: &mut MuxDialogState,
    ctx: &egui::Context,
    locale: crate::i18n::Locale,
) -> MuxDialogOutcome {
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

    let target_snapshot: Rc<Cell<usize>> = Rc::new(Cell::new(*target));
    let target_snapshot_body = Rc::clone(&target_snapshot);
    let target_snapshot_confirm = Rc::clone(&target_snapshot);

    let outcome = {
        let target_ref: &mut usize = target;
        Dialog::<MuxDialogOutcome>::input("ウィンドウを移動", "Move Window", locale)
            .body(move |ui: &mut egui::Ui| {
                let current_label = match locale {
                    crate::i18n::Locale::Ja => format!("現在: {cur} / {count} 個中"),
                    crate::i18n::Locale::En => format!("Current: {cur} / {count}"),
                };
                ui.label(current_label);
                // Arrow keys drive the target counter. `consume_key`
                // removes the event from the queue so nothing else
                // re-interprets it.
                let (up, down) = ui.input_mut(|i| {
                    (
                        i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                        i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                    )
                });
                if up && count > 0 {
                    *target_ref = (*target_ref).saturating_add(1).min(count);
                }
                if down {
                    *target_ref = (*target_ref).saturating_sub(1).max(1);
                }
                ui.horizontal(|ui| {
                    let t_label = match locale {
                        crate::i18n::Locale::Ja => "移動先:",
                        crate::i18n::Locale::En => "Move to:",
                    };
                    ui.label(t_label);
                    // Pointer-friendly −/+ buttons flanking a plain Label.
                    // Keyboard users still get ↑↓, but mouse/touch users
                    // cannot type into a Label (DragValue was rejected
                    // because its focused state holds an internal
                    // text-edit buffer that shadows the external
                    // `&mut value`, defeating arrow-key updates).
                    if ui.button("−").clicked() {
                        *target_ref = (*target_ref).saturating_sub(1).max(1);
                    }
                    ui.label(format!("{}", *target_ref));
                    if ui.button("+").clicked() && count > 0 {
                        *target_ref = (*target_ref).saturating_add(1).min(count);
                    }
                });
                let hint = match locale {
                    crate::i18n::Locale::Ja => {
                        "↑↓キーまたは −/+ ボタンで変更、Enterで確定、Escでキャンセル"
                    }
                    crate::i18n::Locale::En => {
                        "Use ↑↓ or −/+ buttons to change, Enter to confirm, Esc to cancel"
                    }
                };
                ui.label(hint);
                target_snapshot_body.set(*target_ref);
            })
            .primary_button("移動", "Move", move || {
                resolve_move_confirm(captured_id, target_snapshot_confirm.get(), cur, count)
            })
            .show(ctx)
    };

    translate_outcome(outcome)
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
