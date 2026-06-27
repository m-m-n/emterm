//! Shared modal dialog helper.
//!
//! This module implements the `Dialog` builder that every native egui
//! dialog routes through. The helper enforces the contract documented
//! in `doc/UI-DESIGN-GUIDELINES.yaml :: dialogs:`:
//!
//! - `egui::Window` chrome (`collapsible=false`, `resizable=false`,
//!   `anchor=CENTER_CENTER`) is applied internally; callers cannot opt
//!   out.
//! - `egui::Frame` chrome (surface-container-high fill,
//!   corner-extra-large rounding, elevation-3 shadow) is applied
//!   internally.
//! - Title typescale is title-large with the MD3 `on_surface` color.
//! - The actions row is right-aligned with `actions-gap` between
//!   buttons; button colors come from [`buttons::ButtonRole`] which
//!   reads the active MD3 palette.
//! - Per-kind keyboard rules (Enter / Esc) and initial focus come from
//!   [`kinds::enter_target`] / [`kinds::escape_target`] /
//!   [`kinds::initial_focus`].
//! - The primary button's label MUST NOT be "OK" / "Ok" / "ok" — the
//!   helper rejects this via `debug_assert!` so a regressing caller
//!   panics in tests.
//!
//! Callers are responsible for: a body closure that draws the content,
//! the `(ja, en)` label pairs, the on-confirm callback that returns the
//! domain value, and translating [`DialogOutcome`] into their own enum.

pub mod buttons;
pub mod focus;
pub mod kinds;
pub mod tokens;

#[cfg(test)]
mod tests;

use egui::{Align2, FontId, Frame, Margin, Rounding};

use crate::i18n::Locale;
use crate::ui::md3;

pub use kinds::DialogKind;

/// Result of a single `Dialog::show()` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogOutcome<T> {
    /// No user interaction this frame — keep the dialog open.
    Pending,
    /// User confirmed; payload is whatever the `on_confirm` closure
    /// returned.
    Confirmed(T),
    /// User cancelled (Esc, scrim, cancel button, or Enter on a
    /// destructive-confirm dialog).
    Cancelled,
}

type LabelPair<'a> = (&'a str, &'a str);
type BodyClosure<'a> = Box<dyn FnMut(&mut egui::Ui) + 'a>;
type ConfirmClosure<'a, T> = Box<dyn FnOnce() -> T + 'a>;

/// Builder for a modal dialog. Construct with [`Dialog::input`],
/// [`Dialog::confirm`], or [`Dialog::destructive_confirm`]; chain the
/// `body` / `primary_button` / `cancel_button` / `initial_focus`
/// setters; finish with [`Dialog::show`].
pub struct Dialog<'a, T> {
    title: LabelPair<'a>,
    locale: Locale,
    kind: DialogKind,
    primary: Option<LabelPair<'a>>,
    cancel: LabelPair<'a>,
    body: Option<BodyClosure<'a>>,
    on_confirm: Option<ConfirmClosure<'a, T>>,
    initial_focus_id: Option<egui::Id>,
    window_id: Option<egui::Id>,
}

impl<'a, T> Dialog<'a, T> {
    /// Default Cancel label pair. Per `dialogs.labels.rules` the Cancel
    /// label is "キャンセル" / "Cancel" verbatim across every dialog.
    pub const DEFAULT_CANCEL: LabelPair<'static> = ("キャンセル", "Cancel");

    fn new(kind: DialogKind, title_ja: &'a str, title_en: &'a str, locale: Locale) -> Self {
        Self {
            title: (title_ja, title_en),
            locale,
            kind,
            primary: None,
            cancel: Self::DEFAULT_CANCEL,
            body: None,
            on_confirm: None,
            initial_focus_id: None,
            window_id: None,
        }
    }

    /// Build an `input` dialog (text / number / select editor). Initial
    /// focus lands on the first focusable body widget — register that
    /// widget's id via [`Self::initial_focus`].
    pub fn input(title_ja: &'a str, title_en: &'a str, locale: Locale) -> Self {
        Self::new(DialogKind::Input, title_ja, title_en, locale)
    }

    /// Build a `confirm` dialog (non-destructive confirmation). Initial
    /// focus lands on the primary button.
    pub fn confirm(title_ja: &'a str, title_en: &'a str, locale: Locale) -> Self {
        Self::new(DialogKind::Confirm, title_ja, title_en, locale)
    }

    /// Build a `destructive-confirm` dialog. Initial focus lands on
    /// Cancel; Enter maps to Cancel; the primary button uses the
    /// destructive color pair.
    pub fn destructive_confirm(title_ja: &'a str, title_en: &'a str, locale: Locale) -> Self {
        Self::new(DialogKind::DestructiveConfirm, title_ja, title_en, locale)
    }

    /// Register the body content. The closure is invoked exactly once
    /// per frame during [`Self::show`], inside the dialog's surface.
    pub fn body(mut self, body: impl FnMut(&mut egui::Ui) + 'a) -> Self {
        self.body = Some(Box::new(body));
        self
    }

    /// Register the primary action label and on-confirm callback.
    ///
    /// The label MUST NOT be "OK" / "Ok" / "ok" in either locale —
    /// `dialogs.labels.rules` forbids the generic "OK" label. This
    /// constructor `debug_assert!`s the rule so a regressing caller
    /// panics during `cargo test`.
    pub fn primary_button(
        mut self,
        ja: &'a str,
        en: &'a str,
        on_confirm: impl FnOnce() -> T + 'a,
    ) -> Self {
        debug_assert!(
            !label_is_generic_ok(ja) && !label_is_generic_ok(en),
            "Dialog primary button label must not be a generic OK — \
             see dialogs.labels.rules in UI-DESIGN-GUIDELINES.yaml (got: ja={ja:?}, en={en:?})"
        );
        self.primary = Some((ja, en));
        self.on_confirm = Some(Box::new(on_confirm));
        self
    }

    /// Override the cancel label. Defaults to `("キャンセル",
    /// "Cancel")`; overriding is rarely needed but supported for
    /// dialogs that want a more specific dismiss verb.
    #[allow(dead_code)]
    pub fn cancel_button(mut self, ja: &'a str, en: &'a str) -> Self {
        self.cancel = (ja, en);
        self
    }

    /// For `Input` dialogs: the egui [`egui::Id`] of the widget that
    /// receives focus on the first frame. Callers obtain this by
    /// calling `.id()` on their text-field response.
    pub fn initial_focus(mut self, id: egui::Id) -> Self {
        self.initial_focus_id = Some(id);
        self
    }

    /// Override the `egui::Id` used for the dialog's `Window` and
    /// associated egui state (focus tracker, etc.). Defaults to a stable
    /// id derived from the English title. Useful when two dialogs share
    /// the same English title but live on different state slots.
    #[allow(dead_code)]
    pub fn window_id(mut self, id: egui::Id) -> Self {
        self.window_id = Some(id);
        self
    }

    /// Resolve a `(ja, en)` pair under the active locale.
    fn resolve(&self, pair: LabelPair<'a>) -> &'a str {
        match self.locale {
            Locale::Ja => pair.0,
            Locale::En => pair.1,
        }
    }

    /// Render the dialog and return its outcome for this frame.
    pub fn show(mut self, ctx: &egui::Context) -> DialogOutcome<T> {
        let title = self.resolve(self.title);
        let primary_label = self.primary.map(|p| self.resolve(p));
        let cancel_label = self.resolve(self.cancel);
        let kind = self.kind;
        let initial_focus_id = self.initial_focus_id;
        let window_id = self
            .window_id
            .unwrap_or_else(|| egui::Id::new(("emterm-dialog", self.title.1)));

        let mut outcome = DialogOutcome::Pending;

        // Scrim: full-screen dim layer below the dialog. Mirrors the
        // WebView shell's `.dialog-overlay` and `profile_selector.rs`
        // — both paint `dialogs.scrim` and treat outside-click as
        // cancel. Without this, the dialog reads as a floating panel
        // (not a modal), the underlying terminal stays interactive,
        // and the SSOT `dialogs.anatomy.overlay` claim drifts.
        let screen = ctx.screen_rect();
        let scrim_resp = egui::Area::new(window_id.with("scrim"))
            .order(egui::Order::Middle)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let painter = ui.painter();
                painter.rect_filled(screen, 0.0, tokens::SCRIM_COLOR);
                ui.allocate_rect(screen, egui::Sense::click())
            })
            .inner;
        if scrim_resp.clicked() {
            outcome = DialogOutcome::Cancelled;
        }

        // Floating surface via `Area + Frame` (NOT `Window`) so contents
        // sizing is deterministic — egui::Window persists Resize state
        // in memory and fights `auto_sized` / `default_size` / `max_width`
        // in ways that surface as "dialog reopens at a stale size".
        //
        // Width is pinned to `WIDTH_COMPACT` (400px) — fits Rename /
        // Move / Upload / Overwrite / Close-guard. Wider variants
        // (profile editor etc.) can opt into `WIDTH_STANDARD` (480px)
        // via a future builder switch.
        egui::Area::new(window_id)
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .interactable(true)
            .show(ctx, |ui| {
                ui.set_min_width(tokens::WIDTH_COMPACT);
                ui.set_max_width(tokens::WIDTH_COMPACT);
                Frame::none()
                    .fill(md3::surface_container_high())
                    .rounding(Rounding::same(tokens::CORNER_RADIUS))
                    .inner_margin(Margin::same(tokens::PADDING))
                    .shadow(tokens::elevation_shadow())
                    .show(ui, |ui| {
                        // MD3 body uses ~8px vertical spacing between widgets;
                        // egui's default item_spacing.y is 6 and reads too dense.
                        ui.spacing_mut().item_spacing.y = tokens::BODY_ITEM_SPACING;

                        ui.label(
                            egui::RichText::new(title)
                                .font(FontId::proportional(tokens::TITLE_LARGE_SIZE))
                                .color(md3::on_surface()),
                        );
                        ui.add_space(tokens::TITLE_TO_BODY_MARGIN);

                        if let Some(body) = self.body.as_mut() {
                            body(ui);
                        }

                        ui.add_space(tokens::ACTIONS_TOP_MARGIN);

                        let primary_role = match kind {
                            DialogKind::DestructiveConfirm => buttons::ButtonRole::Destructive,
                            _ => buttons::ButtonRole::Primary,
                        };

                        // Right-aligned actions row. Cancel is first (left of
                        // primary), matching MD3 "leading-tonal / trailing-filled"
                        // convention.
                        let mut primary_clicked = false;
                        let mut cancel_clicked = false;
                        let mut primary_response: Option<egui::Response> = None;
                        let mut cancel_response: Option<egui::Response> = None;

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = tokens::ACTIONS_GAP;
                            if let Some(label) = primary_label {
                                let resp = buttons::draw_role(ui, primary_role, label);
                                primary_clicked = resp.clicked();
                                primary_response = Some(resp);
                            }
                            let cancel_resp =
                                buttons::draw_role(ui, buttons::ButtonRole::Cancel, cancel_label);
                            cancel_clicked = cancel_resp.clicked();
                            cancel_response = Some(cancel_resp);
                        });

                        // First-frame focus. Stored in egui memory under a
                        // window-scoped id so we only fire `request_focus` once
                        // even if the dialog re-runs across many frames.
                        let focus_memory_id = window_id.with("emterm-dialog-focus-armed");
                        let already_focused: bool = ui
                            .memory_mut(|mem| mem.data.get_temp(focus_memory_id))
                            .unwrap_or(false);
                        if !already_focused {
                            match kinds::initial_focus(kind) {
                                kinds::Target::Primary => {
                                    if let Some(id) = initial_focus_id {
                                        ui.memory_mut(|m| m.request_focus(id));
                                    } else if kind != DialogKind::Input {
                                        // Input dialogs do NOT fall back to focusing the primary
                                        // button when no explicit initial_focus_id was registered.
                                        // The caller's post-show body-widget focus request needs an
                                        // unfocused frame to succeed; stealing focus to the primary
                                        // button here would block the text field from receiving it.
                                        // Confirm / DestructiveConfirm have no text field, so they
                                        // keep the primary-button focus fallback.
                                        if let Some(resp) = primary_response.as_ref() {
                                            resp.request_focus();
                                        }
                                    }
                                }
                                kinds::Target::Cancel => {
                                    if let Some(resp) = cancel_response.as_ref() {
                                        resp.request_focus();
                                    }
                                }
                            }
                            ui.memory_mut(|mem| mem.data.insert_temp(focus_memory_id, true));
                        }

                        let (enter, esc) = ui.input(|i| {
                            (
                                i.key_pressed(egui::Key::Enter),
                                i.key_pressed(egui::Key::Escape),
                            )
                        });

                        if esc {
                            outcome = DialogOutcome::Cancelled;
                        }
                        if enter {
                            match kinds::enter_target(kind) {
                                kinds::Target::Primary => {
                                    // For Input kind, guard against an IME-composition Enter
                                    // being intercepted as a dialog confirm. Only map Enter →
                                    // primary when no widget owns focus (bare Enter in an empty
                                    // dialog), or when the primary button itself owns focus
                                    // (the Tab+Enter confirm path). When a text-edit widget
                                    // (e.g. Rename's TextEdit) owns focus, the Enter belongs
                                    // to that widget's commit path, not the dialog.
                                    //
                                    // Confirm / DestructiveConfirm always focus a button on
                                    // open, so no text widget can own focus — skip the guard.
                                    let fire = if kind == DialogKind::Input {
                                        let focused = ui.memory(|m| m.focused());
                                        let primary_id = primary_response.as_ref().map(|r| r.id);
                                        focused.is_none() || focused == primary_id
                                    } else {
                                        true
                                    };
                                    if fire {
                                        primary_clicked = true;
                                    }
                                }
                                kinds::Target::Cancel => {
                                    cancel_clicked = true;
                                    // Defang any concurrent button activation on the
                                    // (destructive) primary so Enter never confirms it.
                                    primary_clicked = false;
                                }
                            }
                        }
                        if cancel_clicked {
                            outcome = DialogOutcome::Cancelled;
                        }
                        if primary_clicked {
                            if let Some(on_confirm) = self.on_confirm.take() {
                                outcome = DialogOutcome::Confirmed(on_confirm());
                            }
                        }
                    });
            });

        outcome
    }
}

/// Normalize a label and check whether it equals the generic "OK" verb.
/// Trimmed + ascii-lowercased so " Ok " / "ok " / "OK" all qualify; any
/// label with extra context (e.g. "OK, save") is allowed.
fn label_is_generic_ok(label: &str) -> bool {
    label.trim().eq_ignore_ascii_case("ok")
}
