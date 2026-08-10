//! Blink / visual-bell / restart-toast timing for [`App`].

use std::time::Instant;

use super::App;

/// Cursor blink half-period in milliseconds. 530 ms matches xterm's
/// `cursorBlinkXOR` interval; one full on/off cycle is `2 * BLINK_HALF_MS`.
pub const BLINK_HALF_MS: u128 = 530;

/// Visual-bell flash duration. Mirrors the WebView build's
/// `.terminal-bell-flash` animation (150 ms ease-out, `src/styles.css`).
pub const BELL_FLASH_MS: u64 = 150;

/// How long (in egui frame-time seconds) the binary-mismatch restart toast
/// lingers before it auto-dismisses. Owned by this feature (deliberately NOT
/// the SFTP `TOAST_LINGER_SECS`) so the two toasts can diverge.
pub(super) const RESTART_TOAST_LINGER_SECS: f64 = 4.0;

/// Single auto-dismissing toast prompting a restart after the running binary
/// no longer matches the on-disk binary. Mirrors the SFTP toast's monotonic
/// frame-time dismiss model (no wall-clock).
#[derive(Debug, Default)]
pub struct RestartToast {
    /// Frame-time at which the toast auto-dismisses. `None` while inactive.
    pub(super) dismiss_at: Option<f64>,
}

impl RestartToast {
    /// (Re)arm the single toast: schedule dismissal at `now + linger`. A
    /// subsequent arm overwrites the prior instant (one toast, refreshed).
    pub(super) fn arm(&mut self, now: f64) {
        self.dismiss_at = Some(now + RESTART_TOAST_LINGER_SECS);
    }

    /// Clear the toast once the frame time reaches its dismissal instant.
    pub(super) fn prune(&mut self, now: f64) {
        if matches!(self.dismiss_at, Some(at) if now >= at) {
            self.dismiss_at = None;
        }
    }

    /// Whether the toast should currently be drawn.
    pub fn active(&self) -> bool {
        self.dismiss_at.is_some()
    }
}

impl App {
    /// True when the cursor should currently render its glyph. When the
    /// terminal disables blink the cursor is always considered visible
    /// here (terminal-visibility is gated separately in `draw_cursor`).
    pub fn blink_visible_now(&self, blink_enabled: bool) -> bool {
        if !blink_enabled {
            return true;
        }
        let phase = self.blink_started.elapsed().as_millis() / BLINK_HALF_MS;
        phase.is_multiple_of(2)
    }

    /// Reset the blink reference to "now" so the cursor enters its visible
    /// half-cycle. Use this when the user does something that should
    /// re-pin attention to the cursor (typing, paste, tab switch, focus
    /// regain).
    pub fn reset_blink_phase(&mut self) {
        self.blink_started = Instant::now();
        self.previous_blink_visible = true;
    }

    /// Progress (0.0–1.0) of the in-flight visual-bell flash, `None`
    /// when idle. `render::draw_terminal` maps this to the overlay's
    /// decaying alpha.
    pub fn visual_bell_progress(&self) -> Option<f32> {
        let started = self.visual_bell_started?;
        let t = started.elapsed().as_secs_f32() / (BELL_FLASH_MS as f32 / 1000.0);
        (t < 1.0).then_some(t)
    }

    /// True while a visual-bell flash needs frames. Polled in
    /// `about_to_wait` alongside [`App::needs_blink_repaint`] so the
    /// 150 ms decay animates even when no PTY / input event would
    /// otherwise request a redraw. Clears the latch once the flash
    /// expired — returning true one last time so the final frame erases
    /// the overlay.
    ///
    /// task0005 AC-4: also sets [`Self::bell_erase_pending`] on that same
    /// expiry turn — `visual_bell_started` is already cleared by the time
    /// `window_host::render` runs, so the skip decision needs this
    /// separate one-shot signal to know the erase frame must not be
    /// skipped. Consumed via [`App::take_bell_erase_pending`].
    pub fn needs_bell_repaint(&mut self) -> bool {
        match self.visual_bell_started {
            None => false,
            Some(started) if started.elapsed().as_millis() as u64 >= BELL_FLASH_MS => {
                self.visual_bell_started = None;
                self.bell_erase_pending = true;
                true
            }
            Some(_) => true,
        }
    }

    /// Consume the one-shot bell-erase-frame signal (task0005 AC-4).
    /// Returns `true` exactly once per bell expiry — the render skip
    /// decision ORs this into its `overlay_work` input — then reads
    /// `false` again until the next flash expires.
    pub fn take_bell_erase_pending(&mut self) -> bool {
        std::mem::take(&mut self.bell_erase_pending)
    }

    /// True when the cursor's blink half-cycle has crossed a boundary
    /// since the last paint and the cell needs to repaint to flip the
    /// on/off state. The event loop polls this in `about_to_wait` so a
    /// blinking cursor advances even when no PTY / IME / input event
    /// would otherwise dirty a row. Without this, `egui_ctx`'s
    /// `request_repaint_after` is silent (no callback bridges it back
    /// to `window.request_redraw()`), so the cursor would freeze at
    /// whatever phase the last paint landed on.
    pub fn needs_blink_repaint(&self) -> bool {
        // Blink is suppressed while the window is unfocused (the
        // outline cursor stays steady). Skip waking up for blink
        // transitions in that case — saves a redraw every 530 ms when
        // the user is working in another window.
        if !self.window_focused {
            return false;
        }
        let Some(tab) = self.tabs.get(self.active) else {
            return false;
        };
        let core = tab.core.lock();
        if !core.get_cursor_visible() {
            return false;
        }
        let blink_enabled = core.get_cursor_blink();
        if !blink_enabled {
            return false;
        }
        self.blink_visible_now(blink_enabled) != self.previous_blink_visible
    }

    /// Next `Instant` at which the active tab's cursor blink phase will
    /// flip, if blink is currently eligible to animate (task0004 D4). Shares
    /// `blink_started` / [`BLINK_HALF_MS`] with [`App::blink_visible_now`] /
    /// [`App::needs_blink_repaint`], which detect *whether* the phase
    /// flipped since it was last observed; this instead computes *when* the
    /// next flip will occur, so the event loop can schedule a
    /// `ControlFlow::WaitUntil` for it instead of polling every turn.
    ///
    /// `None` when there is nothing to schedule: no active tab, the window
    /// is unfocused, the cursor is hidden, or blink is disabled — AC-2:
    /// blink disabled means no periodic wakeup at all, even with the window
    /// focused.
    pub fn next_blink_deadline(&self) -> Option<Instant> {
        if !self.window_focused {
            return None;
        }
        let tab = self.tabs.get(self.active)?;
        let core = tab.core.lock();
        if !core.get_cursor_visible() || !core.get_cursor_blink() {
            return None;
        }
        let elapsed_ms = self.blink_started.elapsed().as_millis();
        let next_phase = elapsed_ms / BLINK_HALF_MS + 1;
        let next_ms = (next_phase * BLINK_HALF_MS) as u64;
        Some(self.blink_started + std::time::Duration::from_millis(next_ms))
    }

    /// Next `Instant` at which the in-flight visual-bell flash finishes
    /// decaying, `None` while idle (task0004 D4). Companion to
    /// [`App::needs_bell_repaint`] (which edge-triggers a redraw once the
    /// flash has crossed [`BELL_FLASH_MS`]); this exposes the deadline
    /// itself so the event loop can schedule a `ControlFlow::WaitUntil` for
    /// it instead of polling every turn.
    pub fn next_bell_deadline(&self) -> Option<Instant> {
        let started = self.visual_bell_started?;
        Some(started + std::time::Duration::from_millis(BELL_FLASH_MS))
    }

    /// Binary-mismatch restart toast: a failed self-spawn (possibly off the
    /// App thread) sets a process-global flag. Consume it once per frame to
    /// arm/refresh the single toast, then auto-dismiss via frame time. Returns
    /// true when the toast state changed (so the caller can request a redraw).
    /// `now` is the egui frame time (monotonic, wall-clock-free).
    pub fn pump_restart_toast(&mut self, now: f64) -> bool {
        let mut changed = false;
        if crate::self_exec::restart_required() {
            self.restart_toast.arm(now);
            changed = true;
        }
        let was_active = self.restart_toast.active();
        self.restart_toast.prune(now);
        if was_active != self.restart_toast.active() {
            changed = true;
        }
        changed
    }
}
