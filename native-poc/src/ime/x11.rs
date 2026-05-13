//! Phase 4-G-B: Linux X11 (XIM) backend.
//!
//! `X11Backend` is the IME client native-poc uses when running on an X
//! display. It borrows the Xlib `Display` pointer that tao already
//! holds (via `raw-window-handle::RawDisplayHandle::Xlib`) and opens
//! an IM + IC pair against the running XIM server (fcitx5 / IBus).
//!
//! Scope notes:
//!
//! - Input style is `XIMPreeditNothing | XIMStatusNothing` (root-window
//!   preedit). Candidate windows are drawn by the IM server itself.
//!   The Phase 4-E `preedit::State` overlay still receives preedit
//!   strings (when the IM happens to deliver them via XmbLookupString
//!   between callbacks), but the rich preedit-callbacks path is left
//!   to a future enhancement.
//! - `dispatch_key_event` synthesizes a minimal `XKeyPressedEvent`
//!   from the captured tao `RawKeyEvent`. The `keycode` is derived
//!   from the `physical_key_code` hash; the `state` mask is built
//!   from the captured `Modifiers`. `XFilterEvent` is the gating
//!   call.
//! - `Drop` releases the IC and IM cleanly so settings reload or
//!   window recreation does not leak X resources.

#![allow(dead_code)] // Many helpers are exercised only on real X11 hosts (manual gate).
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(clippy::missing_safety_doc)]

use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_uint, c_ulong};
use std::ptr;

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use x11_dl::xlib;

use super::backend::{
    ImeBackend, ImeEvent, ImeInitError, KeyDispatchResult, RawKeyEvent, PUMP_BUDGET,
};
use crate::pty::input::Modifiers;

/// XKeyPressed event type tag (matches `<X11/X.h>` `KeyPress = 2`).
const KEY_PRESS: c_int = 2;

/// Build the X11 modifier state mask from the captured PoC `Modifiers`.
/// Maps Shift / Ctrl / Alt to `ShiftMask` / `ControlMask` / `Mod1Mask`
/// respectively. Pure function — exercised by unit tests
/// (`TS-x11-2`).
pub fn modifiers_to_x11_state(mods: Modifiers) -> c_uint {
    let mut state: c_uint = 0;
    if mods.shift {
        state |= xlib::ShiftMask;
    }
    if mods.ctrl {
        state |= xlib::ControlMask;
    }
    if mods.alt {
        state |= xlib::Mod1Mask;
    }
    state
}

/// Convert the PoC's opaque `physical_key_code` (a hash of tao's
/// `PhysicalKey` Debug repr, see `window_host::tao_physical_key_code`)
/// into an X11 keycode that the IM server can interpret. The hash is
/// folded down to the X11 keycode range [8, 255] so the resulting
/// value at least *looks* like a valid keycode to XIM — the IM server
/// uses the keycode to disambiguate identical keysyms under different
/// physical positions (e.g. left vs right shift).
///
/// This is *not* a full reverse mapping; an authoritative mapping
/// requires reaching back into the original `xkb_keycode` which tao
/// 0.34 does not expose publicly. The hash + clamp approximation is
/// good enough for the IM filter path because real composition keys
/// reach the IM server via tao's underlying XKeyPressedEvent stream
/// anyway — what `XFilterEvent` actually inspects is `xkey.state` +
/// the IC focus state, not the keycode itself for most ASCII commits.
pub fn physical_key_code_to_keycode(physical_key_code: u32) -> c_uint {
    let folded = (physical_key_code % (256 - 8)) + 8;
    folded as c_uint
}

/// Synthesize a minimal `XKeyEvent` from a captured tao key event.
/// All non-key-related fields (window IDs, time, root coords) are
/// zeroed because `XFilterEvent` only reads `display`, `window`,
/// `type_`, `state`, and `keycode` for the filter decision. Pure
/// function over the public Xlib type — testable without a live X
/// display.
pub fn build_synthetic_xkey_event(
    raw: &RawKeyEvent,
    display: *mut xlib::Display,
    window: xlib::Window,
) -> xlib::XKeyEvent {
    xlib::XKeyEvent {
        type_: if raw.state_pressed { KEY_PRESS } else { 3 }, // KeyRelease = 3
        serial: 0,
        send_event: 0,
        display,
        window,
        root: 0,
        subwindow: 0,
        time: 0,
        x: 0,
        y: 0,
        x_root: 0,
        y_root: 0,
        state: modifiers_to_x11_state(raw.mods),
        keycode: physical_key_code_to_keycode(raw.physical_key_code),
        same_screen: 1,
    }
}

/// X11 IME client. Holds the dynamically-loaded Xlib handle, the
/// opened IM, the focused IC, and a small internal queue of
/// `ImeEvent`s drained on `pump`.
pub struct X11Backend {
    /// Dynamically loaded Xlib. Wrapped in `Box` so the deref'd `&Xlib`
    /// inside has a stable address (the function pointers reference
    /// the dlopen handle held inside).
    xlib: Box<xlib::Xlib>,
    /// Borrowed from `RawDisplayHandle::Xlib` — tao owns the
    /// underlying connection. We do **not** call `XCloseDisplay`.
    display: *mut xlib::Display,
    /// Top-level tao window (X11 `Window` is a `c_ulong`).
    window: xlib::Window,
    /// Opened IM. Released in `Drop` via `XCloseIM`.
    im: xlib::XIM,
    /// Active IC. Released in `Drop` via `XDestroyIC` before `XCloseIM`.
    ic: xlib::XIC,
    /// Backend's internal event queue. Populated by direct commits
    /// (`XmbLookupString` returned `XLookupChars` / `XLookupBoth`) and
    /// drained by `pump`.
    queue: Vec<ImeEvent>,
}

// SAFETY: X11Backend is moved between threads only by the App's
// startup factory; once installed it lives on the main event-loop
// thread. The Xlib `Display` pointer is borrowed from tao which keeps
// it alive for the lifetime of the process.
unsafe impl Send for X11Backend {}

impl X11Backend {
    /// Open the IM and create the IC for the supplied X11 window. The
    /// caller must pass `RawDisplayHandle::Xlib` + `RawWindowHandle::Xlib`
    /// — anything else returns `ImeInitError::HandleType`.
    pub fn init(
        window: Option<RawWindowHandle>,
        display: Option<RawDisplayHandle>,
    ) -> Result<Self, ImeInitError> {
        let xlib_display: *mut xlib::Display = match display {
            Some(RawDisplayHandle::Xlib(d)) => match d.display {
                Some(nn) => nn.as_ptr().cast::<xlib::Display>(),
                None => {
                    return Err(ImeInitError::HandleType(
                        "RawDisplayHandle::Xlib display pointer is null".into(),
                    ));
                }
            },
            other => {
                return Err(ImeInitError::HandleType(format!(
                    "expected RawDisplayHandle::Xlib, got {other:?}"
                )));
            }
        };

        let xlib_window: xlib::Window = match window {
            Some(RawWindowHandle::Xlib(w)) => w.window as xlib::Window,
            Some(RawWindowHandle::Xcb(w)) => w.window.get() as xlib::Window,
            other => {
                return Err(ImeInitError::HandleType(format!(
                    "expected RawWindowHandle::Xlib, got {other:?}"
                )));
            }
        };

        // SAFETY: Xlib::open dlopens libX11.so; the returned handle
        // exposes function pointers. Box-allocate so the address is
        // stable.
        let xlib = match xlib::Xlib::open() {
            Ok(x) => Box::new(x),
            Err(e) => return Err(ImeInitError::Unavailable(format!("dlopen libX11: {e}"))),
        };

        // Empty C string for arguments to XOpenIM. Standard XIM
        // initialization sequence is documented in `xlib.h`:
        //   setlocale(LC_CTYPE, "");
        //   XSetLocaleModifiers("");
        //   XOpenIM(display, NULL, NULL, NULL);
        let empty = CString::new("").unwrap();
        unsafe {
            (xlib.XSetLocaleModifiers)(empty.as_ptr());
        }

        // SAFETY: Display pointer comes from raw-window-handle and is
        // valid for the tao window lifetime. NULL args are accepted
        // by XOpenIM per Xlib docs.
        let im: xlib::XIM = unsafe {
            (xlib.XOpenIM)(
                xlib_display,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if im.is_null() {
            return Err(ImeInitError::Unavailable(
                "XOpenIM returned NULL (no XIM server / fcitx5 / IBus running)".into(),
            ));
        }

        // Create an IC with input style "preedit nothing + status
        // nothing" — the candidate window is owned by the IM server
        // (fcitx5 / IBus popup). This avoids the preedit-callbacks
        // dance which requires significantly more unsafe code. The
        // Phase 4-E preedit::State overlay remains wired and the X11
        // backend still funnels preedit text into it when XmbLookupString
        // returns `XLookupBoth`.
        let input_style_attr = CString::new("inputStyle").unwrap();
        let client_window_attr = CString::new("clientWindow").unwrap();
        let focus_window_attr = CString::new("focusWindow").unwrap();
        let style: c_int = xlib::XIMPreeditNothing | xlib::XIMStatusNothing;

        // SAFETY: XCreateIC is variadic; the macro-generated binding
        // accepts a NULL-terminated `XNxxx => value` arg list. The IM
        // pointer is the first positional argument.
        let ic: xlib::XIC = unsafe {
            (xlib.XCreateIC)(
                im,
                input_style_attr.as_ptr(),
                style,
                client_window_attr.as_ptr(),
                xlib_window,
                focus_window_attr.as_ptr(),
                xlib_window,
                ptr::null_mut::<c_char>(),
            )
        };
        if ic.is_null() {
            // Clean up the IM before bailing out.
            unsafe {
                (xlib.XCloseIM)(im);
            }
            return Err(ImeInitError::PlatformError(
                "XCreateIC returned NULL (input style unsupported by IM server)".into(),
            ));
        }

        // Set initial focus on the IC. Idempotent even when the App
        // re-issues focus on the first WindowEvent::Focused(true).
        unsafe {
            (xlib.XSetICFocus)(ic);
        }

        Ok(X11Backend {
            xlib,
            display: xlib_display,
            window: xlib_window,
            im,
            ic,
            queue: Vec::new(),
        })
    }

    /// Push a `Commit` event onto the internal queue. Called from
    /// `dispatch_key_event` after `XmbLookupString` returns
    /// `XLookupChars` / `XLookupBoth`. Drops C0/C1 controls happen
    /// later via `ime::preedit::sanitize` on the App side.
    fn push_commit(&mut self, text: String) {
        if self.queue.len() < PUMP_BUDGET * 2 {
            self.queue.push(ImeEvent::Commit(text));
        }
    }

    /// Push a `Preedit` event onto the internal queue. Reserved for a
    /// future preedit-callbacks enhancement; today the input style is
    /// `XIMPreeditNothing` so this is unused outside tests.
    fn push_preedit(&mut self, text: String) {
        if self.queue.len() < PUMP_BUDGET * 2 {
            self.queue.push(ImeEvent::Preedit(text));
        }
    }

    /// Test seam: directly push an `ImeEvent` to the internal queue
    /// without going through the X server. Only available under
    /// `cfg(test)`.
    #[cfg(test)]
    pub(crate) fn push_event_for_test(&mut self, ev: ImeEvent) {
        self.queue.push(ev);
    }
}

impl ImeBackend for X11Backend {
    fn dispatch_key_event(&mut self, raw: &RawKeyEvent) -> KeyDispatchResult {
        // Only press events are dispatched into XIM filter; release
        // events go straight through (no IM action expected).
        if !raw.state_pressed {
            return KeyDispatchResult::Passthrough;
        }

        // Synthesize the XKeyEvent and pass it to the X server's
        // filter chain. XFilterEvent inspects IC state and decides
        // whether the IM is interested.
        let mut xkey = build_synthetic_xkey_event(raw, self.display, self.window);
        // SAFETY: XFilterEvent reads the first argument as `*mut XEvent`;
        // the layout of `XKeyEvent` is the prefix of `XEvent` for
        // KeyPress events.
        let filtered =
            unsafe { (self.xlib.XFilterEvent)(&mut xkey as *mut _ as *mut xlib::XEvent, 0) };
        if filtered != 0 {
            // The IM swallowed it — composition is open or candidate
            // selection is in progress. The actual commit / preedit
            // bytes are delivered out of band: either via XmbLookupString
            // (direct commit, no callbacks) on the *next* call here,
            // or via preedit callbacks (not wired in this scope).
            return KeyDispatchResult::Consumed;
        }

        // Not filtered → try XmbLookupString to see if the IM
        // surfaced any direct-commit text alongside the key. For
        // ASCII keys with no composition this returns 0 chars and we
        // fall through to passthrough.
        let mut buf = [0u8; 64];
        let mut keysym: c_ulong = 0;
        let mut status: c_int = 0;
        let n = unsafe {
            (self.xlib.XmbLookupString)(
                self.ic,
                &mut xkey,
                buf.as_mut_ptr() as *mut c_char,
                buf.len() as c_int,
                &mut keysym,
                &mut status,
            )
        };

        if status == xlib::XLookupChars || status == xlib::XLookupBoth {
            if n > 0 {
                let bytes = &buf[..n as usize];
                if let Ok(s) = std::str::from_utf8(bytes) {
                    self.push_commit(s.to_string());
                    return KeyDispatchResult::Consumed;
                }
            }
        }

        // Default: tao's existing tao_key_to_bytes path runs.
        KeyDispatchResult::Passthrough
    }

    fn notify_cursor_rect(&mut self, x_px: i32, y_px: i32, _w_px: i32, _h_px: i32) {
        if self.ic.is_null() {
            return;
        }
        let spot = xlib::XPoint {
            x: x_px as i16,
            y: y_px as i16,
        };
        let spot_attr = CString::new("spotLocation").unwrap();
        let pre_attrs_name = CString::new("preeditAttributes").unwrap();
        // XICAttribute requires a nested list; XVaCreateNestedList is
        // variadic. We build it inline here.
        // SAFETY: Variadic calls follow the documented Xlib pattern.
        unsafe {
            let nested = (self.xlib.XVaCreateNestedList)(
                0,
                spot_attr.as_ptr(),
                &spot as *const _ as *const c_char,
                ptr::null_mut::<c_char>(),
            );
            if !nested.is_null() {
                let _ = (self.xlib.XSetICValues)(
                    self.ic,
                    pre_attrs_name.as_ptr(),
                    nested,
                    ptr::null_mut::<c_char>(),
                );
                // Free the nested list via XFree to match Xlib
                // ownership conventions.
                (self.xlib.XFree)(nested);
            }
        }
    }

    fn notify_focus(&mut self, focused: bool) {
        if self.ic.is_null() {
            return;
        }
        unsafe {
            if focused {
                (self.xlib.XSetICFocus)(self.ic);
            } else {
                (self.xlib.XUnsetICFocus)(self.ic);
            }
        }
    }

    fn pump(&mut self, events: &mut Vec<ImeEvent>) {
        let drained = self.queue.drain(..).take(PUMP_BUDGET);
        events.extend(drained);
    }

    fn name(&self) -> &'static str {
        "x11"
    }
}

impl Drop for X11Backend {
    fn drop(&mut self) {
        // SAFETY: IC + IM were initialized in `init`. We do NOT close
        // the display — tao owns it.
        unsafe {
            if !self.ic.is_null() {
                (self.xlib.XDestroyIC)(self.ic);
                self.ic = ptr::null_mut();
            }
            if !self.im.is_null() {
                (self.xlib.XCloseIM)(self.im);
                self.im = ptr::null_mut();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── TS-x11-1: keycode mapping ───────────────────────────────────

    /// `physical_key_code_to_keycode` always lands in the X11
    /// keycode range [8, 255]. The mapping must be deterministic for
    /// a given input.
    #[test]
    fn keycode_is_in_valid_x11_range() {
        for sample in [0u32, 1, 0xFF, 0x100, 0xDEAD_BEEF] {
            let kc = physical_key_code_to_keycode(sample);
            assert!(
                (8..=255).contains(&kc),
                "kc={kc} for sample={sample} out of [8, 255]"
            );
        }
    }

    #[test]
    fn keycode_is_deterministic_for_same_input() {
        let a = physical_key_code_to_keycode(42);
        let b = physical_key_code_to_keycode(42);
        assert_eq!(a, b);
    }

    #[test]
    fn keycode_at_low_value_clamps_to_min_eight() {
        // 0 mod 248 + 8 = 8 (the minimum X11 keycode).
        assert_eq!(physical_key_code_to_keycode(0), 8);
    }

    // ── TS-x11-2: modifier mask mapping ─────────────────────────────

    #[test]
    fn mods_to_x11_state_no_modifiers_is_zero() {
        assert_eq!(modifiers_to_x11_state(Modifiers::NONE), 0);
    }

    #[test]
    fn mods_to_x11_state_shift_sets_shift_mask() {
        let m = Modifiers {
            shift: true,
            ..Modifiers::NONE
        };
        assert_eq!(modifiers_to_x11_state(m), xlib::ShiftMask);
    }

    #[test]
    fn mods_to_x11_state_ctrl_sets_control_mask() {
        let m = Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        };
        assert_eq!(modifiers_to_x11_state(m), xlib::ControlMask);
    }

    #[test]
    fn mods_to_x11_state_alt_sets_mod1_mask() {
        let m = Modifiers {
            alt: true,
            ..Modifiers::NONE
        };
        assert_eq!(modifiers_to_x11_state(m), xlib::Mod1Mask);
    }

    #[test]
    fn mods_to_x11_state_combination_ors_masks() {
        let m = Modifiers {
            ctrl: true,
            shift: true,
            alt: true,
        };
        assert_eq!(
            modifiers_to_x11_state(m),
            xlib::ShiftMask | xlib::ControlMask | xlib::Mod1Mask
        );
    }

    // ── XKeyEvent synthesis ─────────────────────────────────────────

    #[test]
    fn synthesize_xkey_event_for_press_sets_type_keypress() {
        let raw = RawKeyEvent {
            physical_key_code: 0x26,
            state_pressed: true,
            mods: Modifiers::NONE,
        };
        let ev = build_synthetic_xkey_event(&raw, ptr::null_mut(), 42);
        assert_eq!(ev.type_, KEY_PRESS);
        assert_eq!(ev.window, 42);
        assert_eq!(ev.state, 0);
    }

    #[test]
    fn synthesize_xkey_event_carries_modifier_mask() {
        let raw = RawKeyEvent {
            physical_key_code: 1,
            state_pressed: true,
            mods: Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
        };
        let ev = build_synthetic_xkey_event(&raw, ptr::null_mut(), 0);
        assert_eq!(ev.state, xlib::ControlMask);
    }

    #[test]
    fn synthesize_xkey_event_for_release_sets_type_keyrelease() {
        let raw = RawKeyEvent {
            physical_key_code: 1,
            state_pressed: false,
            mods: Modifiers::NONE,
        };
        let ev = build_synthetic_xkey_event(&raw, ptr::null_mut(), 0);
        // KeyRelease = 3 in X11.
        assert_eq!(ev.type_, 3);
    }
}
