//! winit → egui / PTY-byte input translation: key, button, and modifier
//! mapping, the Shift+Enter rewrite, SKK chord swallowing, synthetic key
//! filtering, and the alternate-screen scroll-wheel encoder.

use winit::event::{KeyEvent, MouseButton};
use winit::keyboard::{Key as WinitKey, NamedKey};

use crate::pty::input::{Key, Modifiers, Target as EncodeTarget, encode};
use crate::settings::ShiftEnterBehavior;

use super::mouse_report::MouseButtonId;

/// Translate a winit `MouseButton` to its `egui::PointerButton`
/// equivalent. Returns `None` for buttons egui does not model (e.g.
/// extra side buttons).
pub(super) fn winit_to_egui_button(b: MouseButton) -> Option<egui::PointerButton> {
    match b {
        MouseButton::Left => Some(egui::PointerButton::Primary),
        MouseButton::Right => Some(egui::PointerButton::Secondary),
        MouseButton::Middle => Some(egui::PointerButton::Middle),
        _ => None,
    }
}

/// Translate a winit logical key into the `egui::Key` consumed by
/// `crate::ui::keybinds::dispatch` and `handle_special_chord`. Returns
/// `None` for keys that no chord can reference (the caller falls through
/// to PTY input).
///
/// The mapped set covers every main key the settings-driven keybind
/// parser can produce (`parse_main_key`): ASCII letters / digits, the
/// symbol keys, the navigation / editing named keys, and F1..F12.
pub(super) fn winit_key_to_egui(logical: &WinitKey) -> Option<egui::Key> {
    match logical {
        WinitKey::Character(s) => {
            let mut chars = s.chars();
            let c = chars.next()?;
            let lower = c.to_ascii_lowercase();
            if lower.is_ascii_alphabetic() {
                // Allocation-free mapping — avoids a heap String per keystroke.
                return match lower {
                    'a' => Some(egui::Key::A),
                    'b' => Some(egui::Key::B),
                    'c' => Some(egui::Key::C),
                    'd' => Some(egui::Key::D),
                    'e' => Some(egui::Key::E),
                    'f' => Some(egui::Key::F),
                    'g' => Some(egui::Key::G),
                    'h' => Some(egui::Key::H),
                    'i' => Some(egui::Key::I),
                    'j' => Some(egui::Key::J),
                    'k' => Some(egui::Key::K),
                    'l' => Some(egui::Key::L),
                    'm' => Some(egui::Key::M),
                    'n' => Some(egui::Key::N),
                    'o' => Some(egui::Key::O),
                    'p' => Some(egui::Key::P),
                    'q' => Some(egui::Key::Q),
                    'r' => Some(egui::Key::R),
                    's' => Some(egui::Key::S),
                    't' => Some(egui::Key::T),
                    'u' => Some(egui::Key::U),
                    'v' => Some(egui::Key::V),
                    'w' => Some(egui::Key::W),
                    'x' => Some(egui::Key::X),
                    'y' => Some(egui::Key::Y),
                    'z' => Some(egui::Key::Z),
                    _ => None,
                };
            }
            match lower {
                '0' => Some(egui::Key::Num0),
                '1' => Some(egui::Key::Num1),
                '2' => Some(egui::Key::Num2),
                '3' => Some(egui::Key::Num3),
                '4' => Some(egui::Key::Num4),
                '5' => Some(egui::Key::Num5),
                '6' => Some(egui::Key::Num6),
                '7' => Some(egui::Key::Num7),
                '8' => Some(egui::Key::Num8),
                '9' => Some(egui::Key::Num9),
                '+' => Some(egui::Key::Plus),
                '-' => Some(egui::Key::Minus),
                ',' => Some(egui::Key::Comma),
                '.' => Some(egui::Key::Period),
                '/' => Some(egui::Key::Slash),
                '\\' => Some(egui::Key::Backslash),
                '=' => Some(egui::Key::Equals),
                ';' => Some(egui::Key::Semicolon),
                ':' => Some(egui::Key::Colon),
                // winit 0.31 removed `NamedKey::Space`; the space bar now
                // arrives as `Character(" ")`.
                ' ' => Some(egui::Key::Space),
                _ => None,
            }
        }
        WinitKey::Named(named) => match named {
            NamedKey::Tab => Some(egui::Key::Tab),
            NamedKey::PageUp => Some(egui::Key::PageUp),
            NamedKey::PageDown => Some(egui::Key::PageDown),
            NamedKey::Home => Some(egui::Key::Home),
            NamedKey::End => Some(egui::Key::End),
            NamedKey::ArrowUp => Some(egui::Key::ArrowUp),
            NamedKey::ArrowDown => Some(egui::Key::ArrowDown),
            NamedKey::ArrowLeft => Some(egui::Key::ArrowLeft),
            NamedKey::ArrowRight => Some(egui::Key::ArrowRight),
            NamedKey::Enter => Some(egui::Key::Enter),
            NamedKey::Escape => Some(egui::Key::Escape),
            NamedKey::Backspace => Some(egui::Key::Backspace),
            NamedKey::Delete => Some(egui::Key::Delete),
            NamedKey::Insert => Some(egui::Key::Insert),
            NamedKey::F1 => Some(egui::Key::F1),
            NamedKey::F2 => Some(egui::Key::F2),
            NamedKey::F3 => Some(egui::Key::F3),
            NamedKey::F4 => Some(egui::Key::F4),
            NamedKey::F5 => Some(egui::Key::F5),
            NamedKey::F6 => Some(egui::Key::F6),
            NamedKey::F7 => Some(egui::Key::F7),
            NamedKey::F8 => Some(egui::Key::F8),
            NamedKey::F9 => Some(egui::Key::F9),
            NamedKey::F10 => Some(egui::Key::F10),
            NamedKey::F11 => Some(egui::Key::F11),
            NamedKey::F12 => Some(egui::Key::F12),
            // F13–F20 are accepted by parse_main_key in keybinds.rs;
            // extend here so a configured F13–F20 chord can reach dispatch
            // at runtime instead of silently falling through to PTY input.
            NamedKey::F13 => Some(egui::Key::F13),
            NamedKey::F14 => Some(egui::Key::F14),
            NamedKey::F15 => Some(egui::Key::F15),
            NamedKey::F16 => Some(egui::Key::F16),
            NamedKey::F17 => Some(egui::Key::F17),
            NamedKey::F18 => Some(egui::Key::F18),
            NamedKey::F19 => Some(egui::Key::F19),
            NamedKey::F20 => Some(egui::Key::F20),
            _ => None,
        },
        _ => None,
    }
}

/// Extract the OS-level physical key / scan code from a winit `KeyEvent`.
/// Phase 4-G-A captures it into [`RawKeyEvent`] so any future IME backend
/// can stash the original scan code without re-querying winit internals.
///
/// winit does not expose the raw scancode publicly on every platform, so
/// we hash the `PhysicalKey` debug representation as a stable stand-in.
/// The exact value is opaque to the App; backends that actually need a
/// real X11 keycode reconstruct it from their own platform layer. The
/// Phase 4-G-3 `WinitImeBridge` ignores this field — winit hands `KeyEvent`
/// directly through `dispatch_key_event_via_ime` if/when needed.
pub(super) fn winit_physical_key_code(event: &KeyEvent) -> u32 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    format!("{:?}", event.physical_key).hash(&mut h);
    h.finish() as u32
}

/// Synthetic key press gate (task0002, IMPLEMENTATION.md Shared Components
/// "Synthetic key press gate"). Winit flags a `KeyboardInput` event
/// `is_synthetic` when it is generated internally rather than from a real
/// hardware press — notably X11 `FocusIn` replays of keys already held down,
/// which produced the stray-`q`-class bugs (see project memory
/// `project_stray_q_xwayland_synthetic_press`). Returns `true` when the
/// event must be dropped before any state mutation, keybinding dispatch, IME
/// forwarding, or PTY write. Applies identically at both call sites (the
/// `Pressed` and `Released` `KeyboardInput` arms): a synthetic release is
/// dropped by the same rule as a synthetic press.
pub(super) fn should_drop_synthetic_key_event(is_synthetic: bool) -> bool {
    is_synthetic
}

/// Translate a winit `KeyEvent` into the PoC's `(Key, Modifiers)` pair and
/// produce the PTY byte sequence. Returns `None` for events that should be
/// ignored (e.g. modifier-only presses).
///
/// On winit the printable text of a key press is exposed via
/// `KeyEvent::text` (already UTF-8). For non-chord plain text (no
/// Ctrl/Alt held) we forward that string verbatim so layout-specific
/// glyphs, dead-key composition results, and shifted symbols all reach
/// the PTY. For chords (Ctrl+C, Alt+b) we go through the `encode`
/// path with the named-key dispatch table.
/// `skk_mode`: whether this press is the bare `Ctrl+J` chord that must be
/// withheld from the PTY. Emacs-style IMEs (SKK) bind `Ctrl+J` for mode
/// switching; without the skip the chord encodes to LF (`0x0A`) and inserts
/// unwanted newlines. Mirrors the WebView build's keyboard-handler skip
/// (`src/terminal-app/handlers/keyboard.ts`): Ctrl held, no Alt/Shift, key
/// `j` (case-insensitive).
pub(super) fn is_skk_swallowed_chord(logical_key: &WinitKey, mods: Modifiers) -> bool {
    mods.ctrl
        && !mods.alt
        && !mods.shift
        && matches!(logical_key, WinitKey::Character(s) if s.eq_ignore_ascii_case("j"))
}

/// Outcome of the [`shift_enter_rewrite`] decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShiftEnterRewrite {
    /// Not a bare Shift+Enter press (or the behavior is out of scope):
    /// encode normally with the original modifiers.
    Unchanged,
    /// Encode normally after substituting these modifiers for the
    /// original ones (`none` drops Shift; `alt_enter` drops Shift and
    /// sets Alt).
    Modifiers(Modifiers),
    /// Bypass the key encoder and write this literal byte sequence
    /// (`kitty_csi_u`, `lf`).
    RawBytes(&'static [u8]),
}

/// Literal Kitty keyboard protocol CSI u sequence for Enter (Unicode key
/// code 13) with the Shift modifier (xterm modifier parameter 2):
/// `ESC [ 1 3 ; 2 u`. See task0001 design D1.
const KITTY_CSI_U_SHIFT_ENTER: [u8; 7] = [0x1B, b'[', b'1', b'3', b';', b'2', b'u'];

/// Literal single-byte line feed (0x0a) emitted for `lf`. See task0001
/// design D1.
const LF_SHIFT_ENTER: [u8; 1] = [0x0A];

/// Pure decision table for the `shift_enter_behavior` key rewrite
/// (task0001 design D1). `is_enter` / `mods` describe the pressed key;
/// the call site only reaches this after UI-layer handlers (search bar,
/// keybind dispatch, SKK swallow) have already run. Rewrite applies only
/// when the modifier state is exactly Shift (no Ctrl, no Alt).
pub(super) fn shift_enter_rewrite(
    is_enter: bool,
    mods: Modifiers,
    behavior: ShiftEnterBehavior,
) -> ShiftEnterRewrite {
    if !is_enter || !mods.shift || mods.ctrl || mods.alt {
        return ShiftEnterRewrite::Unchanged;
    }
    match behavior {
        ShiftEnterBehavior::None => ShiftEnterRewrite::Modifiers(Modifiers {
            shift: false,
            ..mods
        }),
        ShiftEnterBehavior::AltEnter => ShiftEnterRewrite::Modifiers(Modifiers {
            shift: false,
            alt: true,
            ..mods
        }),
        ShiftEnterBehavior::KittyCsiU => ShiftEnterRewrite::RawBytes(&KITTY_CSI_U_SHIFT_ENTER),
        ShiftEnterBehavior::Lf => ShiftEnterRewrite::RawBytes(&LF_SHIFT_ENTER),
    }
}

/// Pure decision for whether a key event that has just been handled
/// should clear the mouse selection (feature selection-clear-on-enter-
/// copy, IMPLEMENTATION.md Shared Components). `forwarded` is whether the
/// event produced bytes written to the PTY; `is_enter` is whether the
/// logical key is the named Enter key, captured before any Shift+Enter
/// rewrite is applied. True only when both are true; reads no state,
/// mutates nothing, and is safe to call any number of times.
pub(super) fn should_clear_selection_on_forward(forwarded: bool, is_enter: bool) -> bool {
    forwarded && is_enter
}

pub(super) fn winit_key_to_bytes(
    event: &KeyEvent,
    mods: Modifiers,
    target: EncodeTarget,
) -> Option<Vec<u8>> {
    // Named keys take precedence over the printable fast path. winit on
    // Windows fills `event.text` for Backspace with `"\x7f"` (DEL); if we
    // routed that through the fast path the PTY would receive DEL, which
    // ConPTY converts to a `Backspace + Ctrl` INPUT_RECORD that PSReadLine
    // binds to BackwardKillWord — `ssh[BS]` then wipes the whole token.
    // Resolving named keys first sends 0x08 (BS, Ctrl+H) instead, which
    // ConPTY passes through as a plain Backspace.
    let named_key: Option<Key> = match &event.logical_key {
        WinitKey::Named(NamedKey::Enter) => Some(Key::Enter),
        WinitKey::Named(NamedKey::Tab) => Some(Key::Tab),
        WinitKey::Named(NamedKey::Backspace) => Some(Key::Backspace),
        WinitKey::Named(NamedKey::Escape) => Some(Key::Escape),
        WinitKey::Named(NamedKey::ArrowUp) => Some(Key::Up),
        WinitKey::Named(NamedKey::ArrowDown) => Some(Key::Down),
        WinitKey::Named(NamedKey::ArrowLeft) => Some(Key::Left),
        WinitKey::Named(NamedKey::ArrowRight) => Some(Key::Right),
        WinitKey::Named(NamedKey::Home) => Some(Key::Home),
        WinitKey::Named(NamedKey::End) => Some(Key::End),
        WinitKey::Named(NamedKey::PageUp) => Some(Key::PageUp),
        WinitKey::Named(NamedKey::PageDown) => Some(Key::PageDown),
        WinitKey::Named(NamedKey::Delete) => Some(Key::Delete),
        WinitKey::Named(NamedKey::Insert) => Some(Key::Insert),
        WinitKey::Named(NamedKey::F1) => Some(Key::F(1)),
        WinitKey::Named(NamedKey::F2) => Some(Key::F(2)),
        WinitKey::Named(NamedKey::F3) => Some(Key::F(3)),
        WinitKey::Named(NamedKey::F4) => Some(Key::F(4)),
        WinitKey::Named(NamedKey::F5) => Some(Key::F(5)),
        WinitKey::Named(NamedKey::F6) => Some(Key::F(6)),
        WinitKey::Named(NamedKey::F7) => Some(Key::F(7)),
        WinitKey::Named(NamedKey::F8) => Some(Key::F(8)),
        WinitKey::Named(NamedKey::F9) => Some(Key::F(9)),
        WinitKey::Named(NamedKey::F10) => Some(Key::F(10)),
        WinitKey::Named(NamedKey::F11) => Some(Key::F(11)),
        WinitKey::Named(NamedKey::F12) => Some(Key::F(12)),
        _ => None,
    };
    if let Some(key) = named_key {
        let bytes = encode(key, mods, target);
        return if bytes.is_empty() { None } else { Some(bytes) };
    }

    // Fast path for plain printable text — winit already accounts for the
    // current keyboard layout (X11 / Wayland / Win32). When IME is
    // composing, winit suppresses `text` and routes the result via
    // `WindowEvent::Ime` instead, so this branch never double-delivers.
    if !mods.ctrl && !mods.alt {
        if let Some(text) = &event.text {
            if !text.is_empty() {
                return Some(text.as_bytes().to_vec());
            }
        }
    }

    let key = match &event.logical_key {
        // winit 0.31 removed `NamedKey::Space`; the space bar now arrives
        // as `Character(" ")`, already covered by this arm.
        WinitKey::Character(s) => {
            let mut chars = s.chars();
            let c = chars.next()?;
            Key::Char(c)
        }
        _ => return None,
    };
    let bytes = encode(key, mods, target);
    if bytes.is_empty() { None } else { Some(bytes) }
}

/// Upper bound for a single wheel event's arrow-key emission; protects against runaway/non-finite delta inputs.
pub(super) const MAX_ALT_SCROLL_NOTCHES: u32 = 100;

/// Accumulate a fractional wheel delta `lines` into `acc` and return
/// `(consumed_whole, new_accum)`. `consumed_whole` is the integer
/// portion of the new total (the "ready to fire" line count); `new_accum`
/// is the leftover fractional remainder the caller should store back.
/// Both signs are preserved: a downward scroll accumulates a negative
/// whole and returns a negative `consumed_whole`.
pub(super) fn accumulate_alt_scroll_lines(acc: f32, lines: f32) -> (f32, f32) {
    let new_acc = acc + lines;
    let whole = if new_acc >= 0.0 {
        new_acc.floor()
    } else {
        new_acc.ceil()
    };
    let frac = new_acc - whole;
    (whole, frac)
}

/// FR1 (DECSET 1007): compute the PTY bytes to emit for one wheel
/// event, or `None` when the gates do not let alternate-scroll
/// translation fire (the caller then falls back to the existing
/// scrollback-view branch). All three gates must be ON: AltScreen is
/// active, the terminal-side `MODE_ALTERNATE_SCROLL` bit is set, and
/// the user setting `alternate_scroll_enabled` is true. `lines` is the
/// y-axis wheel delta in cell rows (positive = wheel-up). Sub-notch
/// fractional pixel deltas (|lines| < 1.0) are treated as no-ops to
/// match a discrete wheel click. xterm convention: 3 arrow bytes per
/// notch, Shift modifier is intentionally ignored at the call site.
pub(super) fn alternate_scroll_wheel_bytes(
    lines: f32,
    alt_screen: bool,
    mode_bit_on: bool,
    setting_on: bool,
) -> Option<Vec<u8>> {
    if !lines.is_finite() {
        return None;
    }
    if !alt_screen || !mode_bit_on || !setting_on {
        return None;
    }
    let notches = (lines.abs().floor() as u32).min(MAX_ALT_SCROLL_NOTCHES);
    if notches == 0 {
        return None;
    }
    let arrow: &[u8] = if lines > 0.0 { b"\x1b[A" } else { b"\x1b[B" };
    let count = (notches as usize) * 3;
    let mut buf = Vec::with_capacity(arrow.len() * count);
    for _ in 0..count {
        buf.extend_from_slice(arrow);
    }
    Some(buf)
}

/// task0002 (mouse-reporting) SC-6: which of the three mutually exclusive
/// consumers a wheel notch should reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // no caller yet in this worktree; task0003 (L3) wires this in.
pub(super) enum WheelConsumer {
    /// Encode and send a mouse report to the tracking application.
    ReportToApplication,
    /// Emit the alternate-scroll arrow-key bytes ([`alternate_scroll_wheel_bytes`]).
    TranslateToArrows,
    /// Scroll eMterm's own scrollback view; no PTY bytes.
    ScrollScrollback,
}

/// task0002 SC-6: choose exactly one wheel consumer for one wheel notch.
///
/// Branches on `tracking_active` FIRST, so the two branches share no rows
/// (IMPLEMENTATION.md D5) — while an application is tracking the mouse, no
/// wheel notch ever produces arrow bytes, on either screen.
///
/// **Tracking active**: `shift_held` -> [`WheelConsumer::ScrollScrollback`];
/// otherwise -> [`WheelConsumer::ReportToApplication`].
/// [`WheelConsumer::TranslateToArrows`] is unreachable in this branch.
///
/// **Tracking inactive**: today's matrix, reproduced unchanged and without
/// consulting `shift_held` at all — [`WheelConsumer::TranslateToArrows`]
/// when `on_alt_screen && alt_scroll_mode_bit && alt_scroll_setting`,
/// [`WheelConsumer::ScrollScrollback`] otherwise. A scroll-scrollback
/// outcome on a screen with no scrollback to move is a pre-existing no-op,
/// not special-cased here.
#[allow(dead_code)] // no caller yet in this worktree; task0003 (L3) wires this in.
pub(super) fn wheel_consumer(
    tracking_active: bool,
    shift_held: bool,
    on_alt_screen: bool,
    alt_scroll_mode_bit: bool,
    alt_scroll_setting: bool,
) -> WheelConsumer {
    if tracking_active {
        if shift_held {
            WheelConsumer::ScrollScrollback
        } else {
            WheelConsumer::ReportToApplication
        }
    } else if on_alt_screen && alt_scroll_mode_bit && alt_scroll_setting {
        WheelConsumer::TranslateToArrows
    } else {
        WheelConsumer::ScrollScrollback
    }
}

/// task0003 (L3): map a winit mouse button to the identity `mouse_report`
/// (L2) reports in. Side buttons (`Back`/`Forward`/`Button6..10`) have no
/// DEC mouse-reporting encoding and are never routed to the report path.
pub(super) fn winit_button_to_report_identity(button: MouseButton) -> Option<MouseButtonId> {
    match button {
        MouseButton::Left => Some(MouseButtonId::Left),
        MouseButton::Middle => Some(MouseButtonId::Middle),
        MouseButton::Right => Some(MouseButtonId::Right),
        _ => None,
    }
}

/// Upper bound on the notch count a single wheel event's mouse report can
/// carry. This is a security property, not a feel-tuning knob: it bounds
/// both [`wheel_report_notches`]'s output (this file) and the duplication
/// step that turns notches into a PTY write
/// (`pointer_routing::bounded_wheel_report_duplicate`) — two independent
/// clamp layers referencing the SAME constant (task0001 D2), so removing
/// either layer individually still leaves the overall bound intact. It is
/// deliberately never aliased to, nor derived from, [`MAX_ALT_SCROLL_NOTCHES`]
/// — that constant is tuned for feel on an unrelated path (task0001 D3).
pub(super) const MAX_WHEEL_REPORT_NOTCHES: u32 = 100;

/// task0003 (L3): convert a wheel delta (already normalized to "lines") into
/// a signed whole-notch count. Fractional deltas smaller than one full line
/// (high-precision trackpads) round toward zero and produce no report,
/// rather than reporting a partial notch.
///
/// Security property (task0001 D2/D4): the result's absolute value is at
/// most [`MAX_WHEEL_REPORT_NOTCHES`] for every possible `f32` input,
/// including non-finite and extreme finite values. The magnitude is
/// saturated at the cap while it is still a floating-point value, ahead of
/// the float-to-integer conversion — casting an unclamped huge magnitude to
/// `i32` would itself saturate at `i32::MAX`, which would make this
/// function's cap postcondition false for the window between that cast and
/// any later clamp.
///
/// task0001 (wheel-report-fraction-accum): superseded at its one former
/// call site (`pointer_routing::handle_mouse_wheel`) by
/// [`accumulate_wheel_report_lines`], which folds in the carried
/// remainder instead of discarding a sub-notch delta outright. Left
/// defined and unedited (AC-9: "the existing notch-conversion
/// expectations... still hold without editing them") so its existing
/// tests keep proving the stateless conversion behaves identically to
/// before this feature.
#[allow(dead_code)] // AC-9 regression coverage; no production caller after this task.
pub(super) fn wheel_report_notches(lines: f32) -> i32 {
    if !lines.is_finite() {
        return 0;
    }
    let magnitude = lines.abs().floor().min(MAX_WHEEL_REPORT_NOTCHES as f32) as i32;
    if lines >= 0.0 { magnitude } else { -magnitude }
}

/// task0001 (wheel-report-fraction-accum) IMPLEMENTATION.md Shared
/// Components, "Report-path accumulate-and-consume unit": fold one wheel
/// event's line delta into the carried report-path fraction and return
/// `(consumed_notches, new_accum)` — the signed whole-notch count to report
/// and the fraction the caller stores back. Sits beside
/// [`accumulate_alt_scroll_lines`] but is an entirely independent unit with
/// its own cap constant ([`MAX_WHEEL_REPORT_NOTCHES`]), never aliased to,
/// nor derived from, [`MAX_ALT_SCROLL_NOTCHES`] (D3, FR7).
///
/// **Precondition**: `acc` is finite and `acc.abs() < 1.0`; `lines` may be
/// any `f32`, including non-finite.
///
/// **Postconditions** (D4, D6):
/// - (a) a non-finite `lines` returns `(0, acc)` — `acc` bit-identical,
///   nothing folded in, nothing else touched.
/// - (b) otherwise the whole-notch count is the sign-preserving truncation
///   toward zero of `acc + lines` (round down when the total is
///   non-negative, round up when it is negative — the same rule
///   [`accumulate_alt_scroll_lines`] already uses).
/// - (c) the notch magnitude is saturated at [`MAX_WHEEL_REPORT_NOTCHES`]
///   while still an `f32`, ahead of any conversion to `i32` — mirrors
///   [`wheel_report_notches`]'s float-domain-first clamp, for the same
///   reason (a raw cast of an unclamped magnitude would itself saturate at
///   `i32::MAX`, making the cap postcondition momentarily false).
/// - (d) magnitude clipped away by the saturation is discarded: the
///   returned fraction is exactly `0.0` whenever saturation fires, never a
///   leftover sliver of the clipped delta (D4) — the excess is neither
///   reported as notches nor banked for a later event.
/// - (e) the returned fraction is always finite and its magnitude is
///   strictly below `1.0`.
pub(super) fn accumulate_wheel_report_lines(acc: f32, lines: f32) -> (i32, f32) {
    if !lines.is_finite() {
        return (0, acc);
    }
    let total = acc + lines;
    let whole = if total >= 0.0 { total.floor() } else { total.ceil() };
    let whole_abs = whole.abs();
    let saturated = whole_abs > MAX_WHEEL_REPORT_NOTCHES as f32;
    let magnitude = whole_abs.min(MAX_WHEEL_REPORT_NOTCHES as f32) as i32;
    let notches = if whole >= 0.0 { magnitude } else { -magnitude };
    let frac = if saturated { 0.0 } else { total - whole };
    (notches, frac)
}

/// Convert the PTY-side [`Modifiers`] (`input::Modifiers`) into the
/// `egui::Modifiers` shape egui events / `RawInput` expect. `command` /
/// `mac_cmd` are always false — native-poc targets Linux + Windows only.
pub(super) fn input_mods_to_egui(mods: Modifiers) -> egui::Modifiers {
    egui::Modifiers {
        ctrl: mods.ctrl,
        shift: mods.shift,
        alt: mods.alt,
        command: false,
        mac_cmd: false,
    }
}

#[cfg(test)]
mod wheel_consumer_tests {
    use super::*;

    // ── AC-7 (TS-8): wheel-consumer decision, task0002 SC-6 ──────────

    /// D5's collision cell: tracking active, Shift held, on the alternate
    /// screen with both the alt-scroll mode bit and setting on. Must yield
    /// scroll-scrollback — never arrow translation.
    #[test]
    fn tracking_active_shift_held_on_alt_screen_with_alt_scroll_on_still_scrolls_scrollback() {
        assert_eq!(
            wheel_consumer(true, true, true, true, true),
            WheelConsumer::ScrollScrollback
        );
    }

    /// Same alt-scroll gates as above, but tracking inactive: today's
    /// unchanged behaviour (AC7 of SPEC.md) — arrow translation.
    #[test]
    fn tracking_inactive_same_alt_scroll_gates_still_yields_arrow_translation() {
        assert_eq!(
            wheel_consumer(false, true, true, true, true),
            WheelConsumer::TranslateToArrows
        );
    }

    #[test]
    fn tracking_active_without_shift_reports_to_application() {
        assert_eq!(
            wheel_consumer(true, false, false, false, false),
            WheelConsumer::ReportToApplication
        );
        assert_eq!(
            wheel_consumer(true, false, true, true, true),
            WheelConsumer::ReportToApplication
        );
    }

    #[test]
    fn tracking_inactive_reproduces_todays_matrix_without_shift() {
        assert_eq!(
            wheel_consumer(false, false, true, true, true),
            WheelConsumer::TranslateToArrows
        );
        assert_eq!(
            wheel_consumer(false, false, false, true, true),
            WheelConsumer::ScrollScrollback
        );
        assert_eq!(
            wheel_consumer(false, false, true, false, true),
            WheelConsumer::ScrollScrollback
        );
        assert_eq!(
            wheel_consumer(false, false, true, true, false),
            WheelConsumer::ScrollScrollback
        );
    }

    /// Exhaustive over all five boolean inputs (32 combinations): every
    /// combination yields exactly one of the three consumers, arrow
    /// translation is unreachable while tracking, and the tracking-inactive
    /// branch never varies with `shift_held`.
    #[test]
    fn wheel_consumer_is_total_and_exhaustively_matches_the_two_matrices() {
        for tracking_active in [false, true] {
            for shift_held in [false, true] {
                for on_alt_screen in [false, true] {
                    for alt_scroll_mode_bit in [false, true] {
                        for alt_scroll_setting in [false, true] {
                            let got = wheel_consumer(
                                tracking_active,
                                shift_held,
                                on_alt_screen,
                                alt_scroll_mode_bit,
                                alt_scroll_setting,
                            );
                            if tracking_active {
                                assert_ne!(
                                    got,
                                    WheelConsumer::TranslateToArrows,
                                    "arrow translation must be unreachable while tracking is active"
                                );
                                let expected = if shift_held {
                                    WheelConsumer::ScrollScrollback
                                } else {
                                    WheelConsumer::ReportToApplication
                                };
                                assert_eq!(got, expected);
                            } else {
                                // The inactive branch must not consult shift at all.
                                let with_other_shift = wheel_consumer(
                                    tracking_active,
                                    !shift_held,
                                    on_alt_screen,
                                    alt_scroll_mode_bit,
                                    alt_scroll_setting,
                                );
                                assert_eq!(
                                    got, with_other_shift,
                                    "tracking-inactive branch must not consult shift"
                                );
                                let expected = if on_alt_screen
                                    && alt_scroll_mode_bit
                                    && alt_scroll_setting
                                {
                                    WheelConsumer::TranslateToArrows
                                } else {
                                    WheelConsumer::ScrollScrollback
                                };
                                assert_eq!(got, expected);
                            }
                        }
                    }
                }
            }
        }
    }
}
