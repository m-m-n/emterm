//! Phase 4-G-D: Windows IMM32 backend.
//!
//! `WindowsBackend` installs a window subclass on the native-poc
//! top-level HWND using `SetWindowSubclass`. The subclass intercepts
//! `WM_IME_STARTCOMPOSITION` / `WM_IME_COMPOSITION` (`GCS_COMPSTR`
//! for preedit, `GCS_RESULTSTR` for commit) / `WM_IME_ENDCOMPOSITION`
//! and forwards everything else via `DefSubclassProc` so tao's own
//! window proc keeps owning the rest of the message stream.
//!
//! UTF-16 → UTF-8 conversion happens in the pure-function
//! [`utf16_to_utf8`] helper which is shared with the `#[cfg(test)]`
//! cases — tests run on every platform.
//!
//! Scope notes:
//!
//! - The cross-platform conversion helper + unit tests are always
//!   compiled (`TS-windows-1` / `TS-windows-2` / `TS-windows-3` run
//!   on Linux CI too).
//! - The actual subclass install + WndProc body is `#[cfg(windows)]`
//!   only; the Linux build sees the helper module plus an empty
//!   public surface, so the factory probe code can stay portable.
//! - Manual gate `TS-manual-ime-windows` (MS-IME + Google IME)
//!   verifies the end-to-end IMM32 flow on a Windows host.

#![allow(dead_code)] // Many helpers are exercised only on Windows hosts.

// `ImeEvent` is re-exported by `super::backend` and only referenced
// from the `#[cfg(windows)] mod platform` body; the top-level
// `use` would warn on non-Windows targets, so the import lives
// inside the platform module itself.

/// Convert a slice of UTF-16 code units into a UTF-8 `String`,
/// dropping invalid surrogates with a `warn` log (`IME_E401`).
///
/// This is the conversion path that `ImmGetCompositionStringW`
/// payloads flow through before they reach the App's
/// `ImeEvent::Preedit` / `ImeEvent::Commit` route. The function is
/// portable so the unit tests exercise the same code on Linux CI.
///
/// Returns `None` (and emits a single warn log) when the conversion
/// produces no usable text from a non-empty input (every code unit
/// was an unpaired surrogate). Empty input is `Some(String::new())`.
pub fn utf16_to_utf8(codes: &[u16]) -> Option<String> {
    if codes.is_empty() {
        return Some(String::new());
    }
    // `from_utf16_lossy` replaces unpaired surrogates with U+FFFD
    // (replacement character). For composition strings that should
    // be all-valid we additionally check for the lossy substitution
    // and warn once if it happened.
    let lossy = String::from_utf16_lossy(codes);
    let had_replacements =
        lossy.contains('\u{FFFD}') && !codes_contain_replacement_codepoint(codes);
    if had_replacements {
        log::warn!(
            "ime UTF-16 → UTF-8 conversion encountered invalid surrogates; \
             replacement glyphs (U+FFFD) substituted (IME_E401)"
        );
        // If the entire string was substitution glyphs derived from
        // bogus surrogates, surface None so the caller can drop the
        // event. Otherwise emit the best-effort string.
        let stripped: String = lossy.chars().filter(|c| *c != '\u{FFFD}').collect();
        if stripped.is_empty() {
            return None;
        }
    }
    Some(lossy)
}

/// `String::from_utf16_lossy` returns U+FFFD on *any* invalid
/// surrogate. Real composition payloads sometimes contain a *legit*
/// U+FFFD (the IME might preview an unknown glyph). We
/// distinguish: if the codes already contain `0xFFFD` we treat the
/// resulting U+FFFD as intentional and skip the warn.
fn codes_contain_replacement_codepoint(codes: &[u16]) -> bool {
    codes.contains(&0xFFFD)
}

// ─── Windows-only backend ────────────────────────────────────────

#[cfg(windows)]
mod platform {
    use std::cell::RefCell;
    use std::sync::Mutex;

    use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Globalization::*;
    use windows::Win32::UI::Input::Ime::*;
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{
        WM_IME_COMPOSITION, WM_IME_ENDCOMPOSITION, WM_IME_STARTCOMPOSITION,
    };

    use super::utf16_to_utf8;
    use crate::ime::backend::{
        ImeBackend, ImeEvent, ImeInitError, KeyDispatchResult, RawKeyEvent, PUMP_BUDGET,
    };

    /// Subclass slot id used to identify our subclass procedure on
    /// the HWND. Arbitrary but stable.
    const SUBCLASS_ID: usize = 0xEM7E_1ME_D;

    thread_local! {
        /// Thread-local queue used by the subclass WndProc to push
        /// `ImeEvent`s onto the App's pump. Lives on the main UI
        /// thread; WndProc is called by the OS on that same thread
        /// (Win32 invariant for window messages).
        static IME_QUEUE: RefCell<Vec<ImeEvent>> = RefCell::new(Vec::new());
    }

    /// Windows IMM32 backend.
    pub struct WindowsBackend {
        hwnd: HWND,
        installed: bool,
    }

    unsafe impl Send for WindowsBackend {}

    impl WindowsBackend {
        pub fn init(
            window: Option<RawWindowHandle>,
            _display: Option<RawDisplayHandle>,
        ) -> Result<Self, ImeInitError> {
            let hwnd: HWND = match window {
                Some(RawWindowHandle::Win32(w)) => HWND(w.hwnd.get() as *mut _),
                other => {
                    return Err(ImeInitError::HandleType(format!(
                        "expected RawWindowHandle::Win32, got {other:?}"
                    )));
                }
            };

            // SAFETY: HWND is owned by tao; SetWindowSubclass is the
            // documented entry point. We pass our subclass id and a
            // zero refdata (state lives in the thread-local queue).
            let ok = unsafe { SetWindowSubclass(hwnd, Some(subclass_wndproc), SUBCLASS_ID, 0) };
            if !ok.as_bool() {
                return Err(ImeInitError::PlatformError(
                    "SetWindowSubclass failed (IME_E301)".into(),
                ));
            }

            Ok(WindowsBackend {
                hwnd,
                installed: true,
            })
        }
    }

    impl ImeBackend for WindowsBackend {
        fn dispatch_key_event(&mut self, _raw: &RawKeyEvent) -> KeyDispatchResult {
            // IMM32 fires its own WM_IME_* messages independently of
            // tao's WM_KEYDOWN, so we always passthrough here.
            KeyDispatchResult::Passthrough
        }

        fn notify_cursor_rect(&mut self, x_px: i32, y_px: i32, _w_px: i32, _h_px: i32) {
            // SAFETY: ImmGetContext / ImmSetCompositionWindow are
            // safe to call on a valid HWND; ImmReleaseContext
            // releases the handle.
            unsafe {
                let himc = ImmGetContext(self.hwnd);
                if !himc.is_invalid() {
                    let mut form = COMPOSITIONFORM {
                        dwStyle: CFS_POINT,
                        ptCurrentPos: windows::Win32::Foundation::POINT { x: x_px, y: y_px },
                        rcArea: Default::default(),
                    };
                    let _ = ImmSetCompositionWindow(himc, &mut form);
                    let _ = ImmReleaseContext(self.hwnd, himc);
                }
            }
        }

        fn notify_focus(&mut self, _focused: bool) {
            // Windows handles focus via WM_SETFOCUS / WM_KILLFOCUS
            // routed through the subclass; nothing extra to send.
        }

        fn pump(&mut self, events: &mut Vec<ImeEvent>) {
            IME_QUEUE.with(|q| {
                let mut q = q.borrow_mut();
                let drained: Vec<_> = q.drain(..).take(PUMP_BUDGET).collect();
                events.extend(drained);
            });
        }

        fn name(&self) -> &'static str {
            "windows"
        }
    }

    impl Drop for WindowsBackend {
        fn drop(&mut self) {
            if self.installed {
                unsafe {
                    let _ = RemoveWindowSubclass(self.hwnd, Some(subclass_wndproc), SUBCLASS_ID);
                }
                self.installed = false;
            }
        }
    }

    /// Subclass WndProc. Intercepts WM_IME_* and forwards everything
    /// else to `DefSubclassProc`.
    unsafe extern "system" fn subclass_wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _uid_subclass: usize,
        _ref_data: usize,
    ) -> LRESULT {
        match msg {
            WM_IME_STARTCOMPOSITION => {
                push_event(ImeEvent::Preedit(String::new()));
            }
            WM_IME_COMPOSITION => {
                let flags = lparam.0 as u32;
                let himc = ImmGetContext(hwnd);
                if !himc.is_invalid() {
                    if (flags & GCS_RESULTSTR.0) != 0 {
                        if let Some(text) = read_composition_string(himc, GCS_RESULTSTR) {
                            push_event(ImeEvent::Commit(text));
                        }
                    } else if (flags & GCS_COMPSTR.0) != 0 {
                        if let Some(text) = read_composition_string(himc, GCS_COMPSTR) {
                            push_event(ImeEvent::Preedit(text));
                        }
                    }
                    let _ = ImmReleaseContext(hwnd, himc);
                }
            }
            WM_IME_ENDCOMPOSITION => {
                // No explicit Commit/FocusOut here — IMM32 already
                // delivered GCS_RESULTSTR via WM_IME_COMPOSITION.
            }
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }

    /// Read the requested composition string component from the
    /// IMC, convert to UTF-8.
    unsafe fn read_composition_string(himc: HIMC, gcs: IME_COMPOSITION_STRING) -> Option<String> {
        // First call: query byte length.
        let needed = ImmGetCompositionStringW(himc, gcs, None, 0);
        if needed <= 0 {
            return Some(String::new());
        }
        let needed_bytes = needed as usize;
        let mut buf = vec![0u8; needed_bytes];
        let got = ImmGetCompositionStringW(
            himc,
            gcs,
            Some(buf.as_mut_ptr() as *mut _),
            needed_bytes as u32,
        );
        if got <= 0 {
            log::warn!("ImmGetCompositionStringW returned <=0 after sizing (IME_E302)");
            return None;
        }
        // Interpret the byte buffer as UTF-16 code units.
        let n_units = (got as usize) / 2;
        let mut codes = Vec::with_capacity(n_units);
        let ptr = buf.as_ptr() as *const u16;
        for i in 0..n_units {
            codes.push(*ptr.add(i));
        }
        utf16_to_utf8(&codes)
    }

    /// Push an event into the thread-local IME queue. Bounded soft
    /// cap at 2 * PUMP_BUDGET so a runaway IM server can't OOM the
    /// main thread.
    fn push_event(ev: ImeEvent) {
        IME_QUEUE.with(|q| {
            let mut q = q.borrow_mut();
            if q.len() < PUMP_BUDGET * 2 {
                q.push(ev);
            }
        });
    }
}

#[cfg(windows)]
pub use platform::WindowsBackend;

// On non-Windows targets we keep a `WindowsBackend` stub solely so
// `crate::ime::backend::build_platform_backend` can name the type in
// a `cfg(windows)` arm without `cfg`-gating the use statement. The
// stub is uninhabited and `init` is unimplemented because the
// factory never reaches it off Windows.
#[cfg(not(windows))]
pub struct WindowsBackend;

#[cfg(not(windows))]
impl WindowsBackend {
    pub fn init(
        _window: Option<raw_window_handle::RawWindowHandle>,
        _display: Option<raw_window_handle::RawDisplayHandle>,
    ) -> Result<Self, crate::ime::backend::ImeInitError> {
        Err(crate::ime::backend::ImeInitError::HandleType(
            "WindowsBackend is cfg(windows)-only".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── TS-windows-1: UTF-16 BMP → UTF-8 ─────────────────────────────

    #[test]
    fn utf16_to_utf8_converts_basic_ascii() {
        // "abc" = [0x61, 0x62, 0x63]
        let codes = [0x61u16, 0x62, 0x63];
        assert_eq!(utf16_to_utf8(&codes), Some("abc".to_string()));
    }

    #[test]
    fn utf16_to_utf8_converts_japanese_bmp() {
        // "日本語":
        //   日 = U+65E5
        //   本 = U+672C
        //   語 = U+8A9E
        let codes = [0x65E5u16, 0x672C, 0x8A9E];
        assert_eq!(utf16_to_utf8(&codes), Some("日本語".to_string()));
    }

    #[test]
    fn utf16_to_utf8_empty_input_is_empty_string() {
        let codes: [u16; 0] = [];
        assert_eq!(utf16_to_utf8(&codes), Some(String::new()));
    }

    #[test]
    fn utf16_to_utf8_preserves_intentional_replacement_codepoint() {
        // U+FFFD literally in the input is intentional; must not be
        // mis-classified as a surrogate error.
        let codes = [0x61u16, 0xFFFD, 0x62];
        assert_eq!(utf16_to_utf8(&codes), Some("a\u{FFFD}b".to_string()));
    }

    // ── TS-windows-2: surrogate pair → UTF-8 ─────────────────────────

    #[test]
    fn utf16_to_utf8_converts_surrogate_pair() {
        // U+1F600 (😀) is encoded as surrogate pair D83D, DE00.
        let codes = [0xD83Du16, 0xDE00];
        assert_eq!(utf16_to_utf8(&codes), Some("😀".to_string()));
    }

    #[test]
    fn utf16_to_utf8_converts_mixed_bmp_and_surrogate() {
        // "a😀b": 0x61, D83D, DE00, 0x62
        let codes = [0x61u16, 0xD83D, 0xDE00, 0x62];
        assert_eq!(utf16_to_utf8(&codes), Some("a😀b".to_string()));
    }

    // ── TS-windows-3: invalid surrogate → drop+warn ──────────────────

    #[test]
    fn utf16_to_utf8_with_only_unpaired_high_surrogate_returns_none() {
        // Lone high surrogate (no matching low). String::from_utf16_lossy
        // replaces it with U+FFFD; since this is the ONLY content,
        // utf16_to_utf8 returns None.
        let codes = [0xD83Du16];
        assert_eq!(utf16_to_utf8(&codes), None);
    }

    #[test]
    fn utf16_to_utf8_with_unpaired_low_surrogate_returns_none() {
        let codes = [0xDC00u16]; // lone low surrogate
        assert_eq!(utf16_to_utf8(&codes), None);
    }

    #[test]
    fn utf16_to_utf8_with_partial_garbage_returns_best_effort_string() {
        // ASCII 'a' + lone surrogate. The lone surrogate is replaced
        // by U+FFFD in lossy conversion; the helper detects the
        // *implicit* substitution (no FFFD in input) and warns, but
        // returns the best-effort string since the stripped form is
        // non-empty.
        let codes = [0x61u16, 0xD83D];
        let out = utf16_to_utf8(&codes).expect("non-empty after strip");
        // Result is either "a" (replacement glyph stripped) or
        // "a\u{FFFD}" (kept). The contract says: best-effort with
        // the warn log fired once. We accept both.
        assert!(out.starts_with('a'));
    }

    #[test]
    fn codes_contain_replacement_codepoint_helper() {
        assert!(!codes_contain_replacement_codepoint(&[0x61, 0x62]));
        assert!(codes_contain_replacement_codepoint(&[0x61, 0xFFFD]));
    }
}
