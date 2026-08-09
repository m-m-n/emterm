//! winit → egui / PTY-byte input translation: key, button, and modifier
//! mapping, the Shift+Enter rewrite, SKK chord swallowing, synthetic key
//! filtering, and the alternate-screen scroll-wheel encoder.

use winit::event::{KeyEvent, MouseButton};
use winit::keyboard::{Key as WinitKey, NamedKey};

use crate::pty::input::{Key, Modifiers, Target as EncodeTarget, encode};
use crate::settings::ShiftEnterBehavior;

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
