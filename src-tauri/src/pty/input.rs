//! Key event → PTY bytes encoder.
//!
//! The encoder maps a `(key, modifiers)` pair into the byte sequence that a
//! conventional terminal would emit. Bracketed paste handling is implemented
//! as a separate wrap step on top of plain paste content.
//!
//! Scope (Phase 2): printable ASCII, Enter, Tab, Backspace, Esc, arrows,
//! Home/End/PageUp/PageDown, function keys F1-F12. Phase 4 adds bracketed
//! paste wrapping.
//!
//! References:
//! - DEC VT100/VT220 keyboard reports.
//! - xterm `XTerm Control Sequences`.

#![allow(dead_code)]

/// A small key abstraction independent of any specific windowing crate.
/// `WindowHost` is responsible for translating tao key events into `Key`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Tab,
    Backspace,
    Escape,
    Up,
    Down,
    Right,
    Left,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    Insert,
    F(u8), // F1..F12
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Modifiers {
    pub const NONE: Modifiers = Modifiers {
        ctrl: false,
        shift: false,
        alt: false,
    };

    pub fn ctrl() -> Self {
        Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        }
    }
}

/// Which PTY the encoded bytes will reach.
///
/// The Windows Backspace / Escape encoding has to honour the local
/// ConPTY's `PSEUDOCONSOLE_WIN32_INPUT_MODE` quirks (Win32 Input Mode
/// key-event pairs instead of bare control bytes). When the bytes are
/// forwarded over the wire to a remote POSIX PTY — the canonical case
/// being a Linux `emterm mux` daemon reached over SSH from a Windows
/// GUI — those Win32 Input Mode sequences are just unknown CSI to the
/// remote shell and get echoed verbatim, so we skip the shim and emit
/// the plain VT bytes instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The byte stream feeds the host OS's local PTY. On Windows that
    /// means the ConPTY portable-pty 0.8 opens with
    /// `PSEUDOCONSOLE_WIN32_INPUT_MODE`, so Backspace / Escape / Ctrl+[
    /// must be emitted as Win32 Input Mode key-event pairs.
    HostPty,
    /// The byte stream is forwarded to a remote POSIX PTY (e.g. a Linux
    /// mux daemon via SSH). Emit conventional VT bytes regardless of
    /// the GUI host OS so the remote shell sees what it expects.
    PosixPty,
}

/// Map `Modifiers` to the xterm CSI modifier `<mods>` parameter
/// (`1..=8`). Returns `None` for `Modifiers::NONE` so the caller can
/// keep the legacy modifier-less byte sequence unchanged. xterm
/// convention: `1 + (shift?1:0) + (alt?2:0) + (ctrl?4:0)`.
fn xterm_mods_param(mods: Modifiers) -> Option<u8> {
    if mods == Modifiers::NONE {
        return None;
    }
    let mut n: u8 = 1;
    if mods.shift {
        n += 1;
    }
    if mods.alt {
        n += 2;
    }
    if mods.ctrl {
        n += 4;
    }
    Some(n)
}

/// Build the `ESC[1;<mods>X` form (Arrow / Home / End / F1-F4 modifier
/// sequences). The `final_byte` is the trailing letter (`A`/`B`/`C`/
/// `D`/`H`/`F`/`P`/`Q`/`R`/`S`).
fn csi_mods_letter(mods_param: u8, final_byte: u8) -> Vec<u8> {
    format!("\x1b[1;{mods_param}{}", final_byte as char).into_bytes()
}

/// Build the `ESC[<n>;<mods>~` form (PageUp/PageDown/Insert/Delete/
/// F5-F12 modifier sequences). `base` is the numeric prefix.
fn csi_mods_tilde(base: u16, mods_param: u8) -> Vec<u8> {
    format!("\x1b[{base};{mods_param}~").into_bytes()
}

/// Encode a key event to the byte sequence a normal terminal would write.
///
/// `target` controls Windows-specific encoding: `HostPty` engages the
/// Win32 Input Mode shim for Backspace / Escape / Ctrl+[, while
/// `PosixPty` always emits the plain VT bytes (used when the bytes
/// will reach a remote Linux PTY through mux). On non-Windows hosts the
/// parameter has no effect.
///
/// Returns an empty buffer for unsupported combinations rather than panicking
/// so the caller can fall back silently. Unknown chord combinations log a
/// warning at the call site.
pub fn encode(key: Key, mods: Modifiers, target: Target) -> Vec<u8> {
    use Key::*;

    // The `target` discriminator only branches the Windows-only shim
    // below; on Unix it has no effect (the byte stream is already POSIX
    // VT-shaped). Silence the unused-variable warning there.
    #[cfg(not(windows))]
    let _ = target;

    // Windows Backspace bypasses the xterm Alt-ESC prefix: the Win32
    // Input Mode sequence carries modifiers inline, and prepending the
    // ESC byte would corrupt the CSI. Only applies when the bytes will
    // reach the local ConPTY — the shim is wrong for remote POSIX PTYs
    // (the Linux mux daemon case).
    #[cfg(windows)]
    if target == Target::HostPty && matches!(key, Backspace) {
        return encode_backspace_win32(mods);
    }

    // Windows Escape: same reasoning as Backspace above. A bare 0x1b on
    // the WIN32_INPUT_MODE channel is not reliably delivered as a real
    // Escape key event, so vim's insert→normal transition fails. Emit
    // the full Win32 Input Mode key event pair instead. This path also
    // bypasses the xterm Alt-ESC prefix because the modifier state is
    // carried inline in the `Cs` field.
    #[cfg(windows)]
    if target == Target::HostPty && matches!(key, Escape) {
        return encode_escape_win32(mods);
    }

    // Windows `Ctrl+[` aliases Escape (vim's `i_CTRL-[`) and currently
    // hits the printable-char Ctrl branch below, which would push the
    // bare `0x1b` byte that WIN32_INPUT_MODE refuses to deliver as a
    // real Escape key event — the exact failure mode the Escape shim
    // above fixes. Route any Ctrl-chord whose canonical control byte
    // is `0x1b` through `encode_escape_win32` instead. `ctrl_byte('[')`
    // is the only mapping that returns `0x1b`, so this is effectively
    // a `Ctrl+[ -> Escape` redirect (Ctrl+3 and the like never reach
    // `ctrl_byte`'s 0x1b arm). Other modifiers (Alt, Shift) are passed
    // through into the `Cs` bitmask just like the Escape shim does.
    #[cfg(windows)]
    if target == Target::HostPty {
        if let Char(c) = key {
            if mods.ctrl && ctrl_byte(c) == Some(0x1b) {
                return encode_escape_win32(mods);
            }
        }
    }

    // FR2: xterm CSI modifier extension. When at least one of
    // Ctrl/Shift/Alt is held AND the key is one of the navigation /
    // function keys that has a documented modified form, emit
    // `ESC[<base>;<mods>X` instead of the legacy short form. The Alt
    // ESC-prefix block further down is intentionally skipped here so
    // Alt does not double-encode (an `ESC` byte before the CSI form
    // would corrupt the sequence). Plain (NONE) modifiers fall through
    // to the legacy bytes below, byte-identical to before this change.
    if let Some(m) = xterm_mods_param(mods) {
        match key {
            Up => return csi_mods_letter(m, b'A'),
            Down => return csi_mods_letter(m, b'B'),
            Right => return csi_mods_letter(m, b'C'),
            Left => return csi_mods_letter(m, b'D'),
            Home => return csi_mods_letter(m, b'H'),
            End => return csi_mods_letter(m, b'F'),
            PageUp => return csi_mods_tilde(5, m),
            PageDown => return csi_mods_tilde(6, m),
            Insert => return csi_mods_tilde(2, m),
            Delete => return csi_mods_tilde(3, m),
            F(n) => match n {
                1 => return csi_mods_letter(m, b'P'),
                2 => return csi_mods_letter(m, b'Q'),
                3 => return csi_mods_letter(m, b'R'),
                4 => return csi_mods_letter(m, b'S'),
                5 => return csi_mods_tilde(15, m),
                6 => return csi_mods_tilde(17, m),
                7 => return csi_mods_tilde(18, m),
                8 => return csi_mods_tilde(19, m),
                9 => return csi_mods_tilde(20, m),
                10 => return csi_mods_tilde(21, m),
                11 => return csi_mods_tilde(23, m),
                12 => return csi_mods_tilde(24, m),
                _ => {} // out-of-range F-key: fall through to legacy
            },
            // Keys NOT in the modifier-eligible set (Char, Enter, Tab,
            // Backspace, Escape) keep their existing behaviour, which
            // already honours modifiers via the legacy branches below
            // (Ctrl-letter → control byte, Shift+Tab → CSI Z, etc.).
            _ => {}
        }
    }

    // ESC prefix for Alt-modified keys (xterm convention).
    let mut out: Vec<u8> = Vec::new();
    if mods.alt {
        out.push(0x1b);
    }

    match key {
        Char(c) => {
            if mods.ctrl {
                if let Some(b) = ctrl_byte(c) {
                    out.push(b);
                    return out;
                }
            }
            // Plain (or Shift-only) printable: emit UTF-8 of the character.
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
        Enter => out.push(b'\r'),
        // Shift+Tab → CSI Z (back-tab); plain Tab → 0x09. Claude Code's
        // mode-switch chord and readline reverse-completion both rely on
        // the back-tab sequence.
        Tab => {
            if mods.shift {
                out.extend_from_slice(b"\x1b[Z");
            } else {
                out.push(b'\t');
            }
        }
        // Linux/macOS: 0x7f (DEL) matches xterm convention (terminfo
        // kbs=^?), required by canonical-mode line editors (sudo prompt,
        // ssh password, stty default erase character).
        // Windows with `Target::HostPty` is handled by
        // `encode_backspace_win32` early-return above; with
        // `Target::PosixPty` (mux → remote Linux daemon) Windows also
        // falls through here so the remote shell sees plain DEL.
        Backspace => out.push(0x7f),
        Escape => out.push(0x1b),
        Up => out.extend_from_slice(b"\x1b[A"),
        Down => out.extend_from_slice(b"\x1b[B"),
        Right => out.extend_from_slice(b"\x1b[C"),
        Left => out.extend_from_slice(b"\x1b[D"),
        Home => out.extend_from_slice(b"\x1b[H"),
        End => out.extend_from_slice(b"\x1b[F"),
        PageUp => out.extend_from_slice(b"\x1b[5~"),
        PageDown => out.extend_from_slice(b"\x1b[6~"),
        Delete => out.extend_from_slice(b"\x1b[3~"),
        Insert => out.extend_from_slice(b"\x1b[2~"),
        F(n) => match n {
            1 => out.extend_from_slice(b"\x1bOP"),
            2 => out.extend_from_slice(b"\x1bOQ"),
            3 => out.extend_from_slice(b"\x1bOR"),
            4 => out.extend_from_slice(b"\x1bOS"),
            5 => out.extend_from_slice(b"\x1b[15~"),
            6 => out.extend_from_slice(b"\x1b[17~"),
            7 => out.extend_from_slice(b"\x1b[18~"),
            8 => out.extend_from_slice(b"\x1b[19~"),
            9 => out.extend_from_slice(b"\x1b[20~"),
            10 => out.extend_from_slice(b"\x1b[21~"),
            11 => out.extend_from_slice(b"\x1b[23~"),
            12 => out.extend_from_slice(b"\x1b[24~"),
            _ => {} // out of range; ignore
        },
    }
    out
}

/// Encode Backspace for ConPTY in `PSEUDOCONSOLE_WIN32_INPUT_MODE`.
///
/// portable-pty 0.8 opens every Windows PTY with that flag (see
/// `portable-pty-0.8.1/src/win/psuedocon.rs`), so ConPTY interprets
/// incoming bytes as Win32 Input Mode VT key sequences rather than
/// plain control characters. A raw `0x08` on this channel is decoded
/// as `Ctrl+Backspace` and triggers PSReadLine / cmd's word-delete
/// (`ssh-keygen[BS]` → `ssh-`). The full key-event sequence below
/// keeps Backspace as a one-character delete and pipes modifier state
/// through as proper INPUT_RECORD bits.
///
/// Sequence: `ESC [ Vk;Sc;Uc;Kd;Cs;Rc _`
///   Vk=8 (VK_BACK), Sc=14 (0x0E scan code), Uc=8, Kd=1/0 (down/up),
///   Cs=ControlKeyState bitmask, Rc=1 (repeat count).
///
/// Reference: Microsoft Terminal spec #4999 (Improved keyboard handling
/// in conpty).
#[cfg(windows)]
fn encode_backspace_win32(mods: Modifiers) -> Vec<u8> {
    // Bias toward the left-side modifier codes to match what an English
    // PC keyboard's leftmost Shift/Ctrl/Alt physically reports.
    let mut cs: u32 = 0;
    if mods.shift {
        cs |= 0x10; // SHIFT_PRESSED
    }
    if mods.ctrl {
        cs |= 0x08; // LEFT_CTRL_PRESSED
    }
    if mods.alt {
        cs |= 0x02; // LEFT_ALT_PRESSED
    }
    format!("\x1b[8;14;8;1;{cs};1_\x1b[8;14;8;0;{cs};1_").into_bytes()
}

/// Encode Escape for ConPTY in `PSEUDOCONSOLE_WIN32_INPUT_MODE`.
///
/// portable-pty 0.8 opens every Windows PTY with that flag (see
/// `portable-pty-0.8.1/src/win/psuedocon.rs`), so ConPTY interprets
/// incoming bytes as Win32 Input Mode VT key sequences rather than
/// raw control characters. A bare `0x1b` on this channel is not
/// reliably delivered as a real Escape key event, so vim's
/// insert→normal transition fails and other modal TUIs misbehave.
/// The full key-event sequence below sends Escape as a proper
/// `KEY_EVENT_RECORD` with modifier state threaded through as
/// INPUT_RECORD `ControlKeyState` bits.
///
/// Sequence: `ESC [ Vk;Sc;Uc;Kd;Cs;Rc _`
///   Vk=27 (VK_ESCAPE), Sc=1 (0x01 scan code), Uc=27, Kd=1/0
///   (down/up), Cs=ControlKeyState bitmask, Rc=1 (repeat count).
///
/// Reference: Microsoft Terminal spec #4999 (Improved keyboard handling
/// in conpty).
#[cfg(windows)]
fn encode_escape_win32(mods: Modifiers) -> Vec<u8> {
    // Bias toward the left-side modifier codes to match what an English
    // PC keyboard's leftmost Shift/Ctrl/Alt physically reports.
    let mut cs: u32 = 0;
    if mods.shift {
        cs |= 0x10; // SHIFT_PRESSED
    }
    if mods.ctrl {
        cs |= 0x08; // LEFT_CTRL_PRESSED
    }
    if mods.alt {
        cs |= 0x02; // LEFT_ALT_PRESSED
    }
    format!("\x1b[27;1;27;1;{cs};1_\x1b[27;1;27;0;{cs};1_").into_bytes()
}

/// Map a printable character to its Ctrl-modified byte (0x00..0x1f).
/// Returns `None` if the character does not have a defined control mapping.
fn ctrl_byte(c: char) -> Option<u8> {
    // Letters: Ctrl+A = 0x01 .. Ctrl+Z = 0x1a.
    if c.is_ascii_lowercase() {
        return Some((c as u8) - b'a' + 1);
    }
    if c.is_ascii_uppercase() {
        return Some((c as u8) - b'A' + 1);
    }
    // Common extras.
    Some(match c {
        '@' | ' ' => 0x00, // Ctrl+Space → NUL
        '[' => 0x1b,       // Ctrl+[ → ESC
        '\\' => 0x1c,
        ']' => 0x1d,
        '^' => 0x1e,
        '_' | '?' => 0x1f,
        _ => return None,
    })
}

/// Wrap raw paste content with bracketed-paste sentinels.
/// The caller decides whether bracketed paste is active.
pub fn wrap_bracketed_paste(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + b"\x1b[200~\x1b[201~".len());
    out.extend_from_slice(b"\x1b[200~");
    out.extend_from_slice(payload);
    out.extend_from_slice(b"\x1b[201~");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Most assertions use `Target::HostPty` because that's the
    // historically-tested path. `Target::PosixPty` gets dedicated
    // coverage below for the Windows Backspace / Escape / Ctrl+[ trio
    // where the two targets diverge.
    const HOST: Target = Target::HostPty;
    const POSIX: Target = Target::PosixPty;

    #[test]
    fn printable_ascii() {
        assert_eq!(encode(Key::Char('a'), Modifiers::NONE, HOST), b"a");
        assert_eq!(encode(Key::Char('Z'), Modifiers::NONE, HOST), b"Z");
        assert_eq!(encode(Key::Char('1'), Modifiers::NONE, HOST), b"1");
    }

    #[test]
    fn enter_tab_backspace_escape() {
        assert_eq!(encode(Key::Enter, Modifiers::NONE, HOST), b"\r");
        assert_eq!(encode(Key::Tab, Modifiers::NONE, HOST), b"\t");
        #[cfg(not(windows))]
        assert_eq!(encode(Key::Backspace, Modifiers::NONE, HOST), b"\x7f");
        #[cfg(not(windows))]
        assert_eq!(encode(Key::Escape, Modifiers::NONE, HOST), b"\x1b");
    }

    /// Dedicated Unix Escape assertion. Pinned separately from
    /// `enter_tab_backspace_escape` per SPEC.md Test Scenarios so the
    /// non-Windows bit-identical Esc encoding (FR4 / NFR4) is a
    /// standalone test rather than a single line buried inside a
    /// multi-key suite.
    #[cfg(not(windows))]
    #[test]
    fn escape_emits_bare_1b_on_unix() {
        assert_eq!(encode(Key::Escape, Modifiers::NONE, HOST), b"\x1b");
    }

    #[test]
    fn shift_tab_emits_back_tab() {
        let shift = Modifiers {
            shift: true,
            ..Modifiers::NONE
        };
        // Shift+Tab must send CSI Z (back-tab), not a plain Tab.
        assert_eq!(encode(Key::Tab, shift, HOST), b"\x1b[Z");
        // Plain Tab is unaffected.
        assert_eq!(encode(Key::Tab, Modifiers::NONE, HOST), b"\t");
    }

    /// Windows Backspace must emit a Win32 Input Mode key event pair
    /// (keyDown + keyUp) instead of a bare 0x08; ConPTY in
    /// `PSEUDOCONSOLE_WIN32_INPUT_MODE` reinterprets the bare byte as
    /// `Ctrl+Backspace`, which triggers PSReadLine / cmd word-delete.
    #[cfg(windows)]
    #[test]
    fn backspace_emits_win32_input_mode_pair() {
        assert_eq!(
            encode(Key::Backspace, Modifiers::NONE, HOST),
            b"\x1b[8;14;8;1;0;1_\x1b[8;14;8;0;0;1_"
        );
    }

    /// Modifiers must thread through the Win32 Input Mode `Cs` field so
    /// chords like `Shift+Backspace` reach PSReadLine intact.
    #[cfg(windows)]
    #[test]
    fn backspace_win32_includes_modifier_bits() {
        let bytes = encode(
            Key::Backspace,
            Modifiers {
                ctrl: true,
                shift: false,
                alt: false,
            },
            HOST,
        );
        // Left Ctrl pressed = 0x08
        assert_eq!(bytes, b"\x1b[8;14;8;1;8;1_\x1b[8;14;8;0;8;1_");
    }

    /// Windows Escape must emit a Win32 Input Mode key event pair
    /// (keyDown + keyUp) instead of a bare 0x1b; ConPTY in
    /// `PSEUDOCONSOLE_WIN32_INPUT_MODE` does not reliably deliver the
    /// bare byte as a real Escape key event, breaking vim insert-mode
    /// exit and other TUI modal behavior.
    #[cfg(windows)]
    #[test]
    fn escape_emits_win32_input_mode_pair() {
        assert_eq!(
            encode(Key::Escape, Modifiers::NONE, HOST),
            b"\x1b[27;1;27;1;0;1_\x1b[27;1;27;0;0;1_"
        );
    }

    /// Ctrl+Esc must set `LEFT_CTRL_PRESSED` (0x08) in the `Cs` field.
    #[cfg(windows)]
    #[test]
    fn escape_win32_includes_ctrl_modifier() {
        let bytes = encode(
            Key::Escape,
            Modifiers {
                ctrl: true,
                shift: false,
                alt: false,
            },
            HOST,
        );
        assert_eq!(bytes, b"\x1b[27;1;27;1;8;1_\x1b[27;1;27;0;8;1_");
    }

    /// Alt+Esc must set `LEFT_ALT_PRESSED` (0x02) in the `Cs` field.
    #[cfg(windows)]
    #[test]
    fn escape_win32_includes_alt_modifier() {
        let bytes = encode(
            Key::Escape,
            Modifiers {
                ctrl: false,
                shift: false,
                alt: true,
            },
            HOST,
        );
        assert_eq!(bytes, b"\x1b[27;1;27;1;2;1_\x1b[27;1;27;0;2;1_");
    }

    /// Shift+Esc must set `SHIFT_PRESSED` (0x10) in the `Cs` field.
    #[cfg(windows)]
    #[test]
    fn escape_win32_includes_shift_modifier() {
        let bytes = encode(
            Key::Escape,
            Modifiers {
                ctrl: false,
                shift: true,
                alt: false,
            },
            HOST,
        );
        assert_eq!(bytes, b"\x1b[27;1;27;1;16;1_\x1b[27;1;27;0;16;1_");
    }

    /// Combined Ctrl+Shift+Esc must OR `LEFT_CTRL_PRESSED` (0x08) and
    /// `SHIFT_PRESSED` (0x10) into `Cs` = 0x18 (24).
    #[cfg(windows)]
    #[test]
    fn escape_win32_combined_modifiers() {
        let bytes = encode(
            Key::Escape,
            Modifiers {
                ctrl: true,
                shift: true,
                alt: false,
            },
            HOST,
        );
        assert_eq!(bytes, b"\x1b[27;1;27;1;24;1_\x1b[27;1;27;0;24;1_");
    }

    /// `Target::PosixPty` on Windows must bypass the Win32 Input Mode
    /// shim — the bytes are heading to a remote Linux PTY (mux daemon
    /// over SSH), which would otherwise echo the unknown CSI verbatim.
    #[cfg(windows)]
    #[test]
    fn backspace_posix_target_skips_win32_input_mode_on_windows() {
        assert_eq!(encode(Key::Backspace, Modifiers::NONE, POSIX), b"\x7f");
    }

    /// Same as above but for Escape — the remote shell needs the plain
    /// 0x1b that VT/terminfo prescribes.
    #[cfg(windows)]
    #[test]
    fn escape_posix_target_skips_win32_input_mode_on_windows() {
        assert_eq!(encode(Key::Escape, Modifiers::NONE, POSIX), b"\x1b");
    }

    /// Ctrl+[ aliases Escape; under `PosixPty` Windows must emit the
    /// same bare 0x1b the Unix branch produces, not the Win32 Input
    /// Mode pair.
    #[cfg(windows)]
    #[test]
    fn ctrl_bracket_posix_target_emits_bare_esc_on_windows() {
        assert_eq!(encode(Key::Char('['), Modifiers::ctrl(), POSIX), b"\x1b");
    }

    /// `Target::PosixPty` must be a no-op on Unix hosts — the encoding
    /// is already POSIX-shaped there, so both targets produce identical
    /// bytes for the keys the Windows shim would otherwise rewrite.
    #[cfg(not(windows))]
    #[test]
    fn posix_target_matches_host_target_on_unix() {
        assert_eq!(
            encode(Key::Backspace, Modifiers::NONE, POSIX),
            encode(Key::Backspace, Modifiers::NONE, HOST)
        );
        assert_eq!(
            encode(Key::Escape, Modifiers::NONE, POSIX),
            encode(Key::Escape, Modifiers::NONE, HOST)
        );
        assert_eq!(
            encode(Key::Char('['), Modifiers::ctrl(), POSIX),
            encode(Key::Char('['), Modifiers::ctrl(), HOST)
        );
    }

    #[test]
    fn arrow_keys() {
        assert_eq!(encode(Key::Up, Modifiers::NONE, HOST), b"\x1b[A");
        assert_eq!(encode(Key::Down, Modifiers::NONE, HOST), b"\x1b[B");
        assert_eq!(encode(Key::Right, Modifiers::NONE, HOST), b"\x1b[C");
        assert_eq!(encode(Key::Left, Modifiers::NONE, HOST), b"\x1b[D");
    }

    #[test]
    fn nav_and_function_keys() {
        assert_eq!(encode(Key::Home, Modifiers::NONE, HOST), b"\x1b[H");
        assert_eq!(encode(Key::End, Modifiers::NONE, HOST), b"\x1b[F");
        assert_eq!(encode(Key::PageUp, Modifiers::NONE, HOST), b"\x1b[5~");
        assert_eq!(encode(Key::PageDown, Modifiers::NONE, HOST), b"\x1b[6~");
        assert_eq!(encode(Key::Delete, Modifiers::NONE, HOST), b"\x1b[3~");
        assert_eq!(encode(Key::Insert, Modifiers::NONE, HOST), b"\x1b[2~");
        assert_eq!(encode(Key::F(1), Modifiers::NONE, HOST), b"\x1bOP");
        assert_eq!(encode(Key::F(5), Modifiers::NONE, HOST), b"\x1b[15~");
        assert_eq!(encode(Key::F(12), Modifiers::NONE, HOST), b"\x1b[24~");
        assert_eq!(encode(Key::F(99), Modifiers::NONE, HOST), b"");
    }

    #[test]
    fn ctrl_letters() {
        assert_eq!(encode(Key::Char('c'), Modifiers::ctrl(), HOST), b"\x03");
        assert_eq!(encode(Key::Char('C'), Modifiers::ctrl(), HOST), b"\x03");
        assert_eq!(encode(Key::Char('a'), Modifiers::ctrl(), HOST), b"\x01");
        assert_eq!(encode(Key::Char('z'), Modifiers::ctrl(), HOST), b"\x1a");
    }

    #[test]
    fn ctrl_extras() {
        // Ctrl+[ aliases Escape; on Windows + HostPty it is rerouted
        // through the Win32 Input Mode shim (see
        // ctrl_bracket_emits_escape_win32_input_mode_pair below). On
        // non-Windows (and on Windows + PosixPty) it still emits the
        // bare 0x1b that canonical-mode line editors expect.
        #[cfg(not(windows))]
        assert_eq!(encode(Key::Char('['), Modifiers::ctrl(), HOST), b"\x1b");
        assert_eq!(encode(Key::Char(' '), Modifiers::ctrl(), HOST), b"\x00");
        assert_eq!(encode(Key::Char('\\'), Modifiers::ctrl(), HOST), b"\x1c");
    }

    /// Ctrl+[ is a documented vim alias for Escape (`:help i_CTRL-[`).
    /// On Windows it must produce the same Win32 Input Mode key event
    /// pair as a bare Escape press; otherwise the same insert-mode
    /// regression that motivated `encode_escape_win32` would recur for
    /// Ctrl+[ users.
    #[cfg(windows)]
    #[test]
    fn ctrl_bracket_emits_escape_win32_input_mode_pair() {
        // Modifier-bits include Ctrl because the user did press Ctrl;
        // the encoded sequence is still the Escape key event so vim
        // observes it as Escape, with Cs surfacing the held Ctrl.
        let bytes = encode(Key::Char('['), Modifiers::ctrl(), HOST);
        // Cs = LEFT_CTRL_PRESSED = 0x08 = 8
        assert_eq!(bytes, b"\x1b[27;1;27;1;8;1_\x1b[27;1;27;0;8;1_");
    }

    #[test]
    fn alt_prefixes_esc() {
        let mods = Modifiers {
            alt: true,
            ..Modifiers::NONE
        };
        assert_eq!(encode(Key::Char('b'), mods, HOST), b"\x1bb");
    }

    // ── FR2: xterm CSI modifier extension ────────────────────

    /// Helper: build a `Modifiers` from named flags.
    fn mods(ctrl: bool, shift: bool, alt: bool) -> Modifiers {
        Modifiers { ctrl, shift, alt }
    }

    /// TS-9: `Ctrl+Home` → `ESC[1;5H`.
    #[test]
    fn ctrl_home_emits_csi_modifier_form() {
        assert_eq!(
            encode(Key::Home, mods(true, false, false), HOST),
            b"\x1b[1;5H"
        );
    }

    /// TS-10: `Ctrl+End` → `ESC[1;5F`.
    #[test]
    fn ctrl_end_emits_csi_modifier_form() {
        assert_eq!(
            encode(Key::End, mods(true, false, false), HOST),
            b"\x1b[1;5F"
        );
    }

    /// TS-11: `Ctrl+PageUp` → `ESC[5;5~`.
    #[test]
    fn ctrl_pageup_emits_csi_modifier_form() {
        assert_eq!(
            encode(Key::PageUp, mods(true, false, false), HOST),
            b"\x1b[5;5~"
        );
    }

    /// TS-12: `Ctrl+Shift+PageDown` → `ESC[6;6~` (mods = 1 + 1 + 4 = 6).
    #[test]
    fn ctrl_shift_pagedown_emits_csi_modifier_form() {
        assert_eq!(
            encode(Key::PageDown, mods(true, true, false), HOST),
            b"\x1b[6;6~"
        );
    }

    /// TS-13: `Ctrl+ArrowUp` → `ESC[1;5A`.
    #[test]
    fn ctrl_arrow_up_emits_csi_modifier_form() {
        assert_eq!(
            encode(Key::Up, mods(true, false, false), HOST),
            b"\x1b[1;5A"
        );
    }

    /// FR2: cover all four arrows for completeness.
    #[test]
    fn ctrl_arrow_keys_emit_csi_modifier_form() {
        let ctrl = mods(true, false, false);
        assert_eq!(encode(Key::Up, ctrl, HOST), b"\x1b[1;5A");
        assert_eq!(encode(Key::Down, ctrl, HOST), b"\x1b[1;5B");
        assert_eq!(encode(Key::Right, ctrl, HOST), b"\x1b[1;5C");
        assert_eq!(encode(Key::Left, ctrl, HOST), b"\x1b[1;5D");
    }

    /// TS-14: `Shift+F1` → `ESC[1;2P` (mods = 1 + 1 = 2).
    #[test]
    fn shift_f1_emits_csi_modifier_form() {
        assert_eq!(
            encode(Key::F(1), mods(false, true, false), HOST),
            b"\x1b[1;2P"
        );
    }

    /// TS-15: `Ctrl+Alt+F5` → `ESC[15;7~` (mods = 1 + 2 + 4 = 7).
    #[test]
    fn ctrl_alt_f5_emits_csi_modifier_form() {
        assert_eq!(
            encode(Key::F(5), mods(true, false, true), HOST),
            b"\x1b[15;7~"
        );
    }

    /// FR2: F1-F4 use the letter form, F5-F12 use the tilde form.
    #[test]
    fn ctrl_function_keys_use_correct_modifier_form() {
        let ctrl = mods(true, false, false);
        assert_eq!(encode(Key::F(1), ctrl, HOST), b"\x1b[1;5P");
        assert_eq!(encode(Key::F(2), ctrl, HOST), b"\x1b[1;5Q");
        assert_eq!(encode(Key::F(3), ctrl, HOST), b"\x1b[1;5R");
        assert_eq!(encode(Key::F(4), ctrl, HOST), b"\x1b[1;5S");
        assert_eq!(encode(Key::F(5), ctrl, HOST), b"\x1b[15;5~");
        assert_eq!(encode(Key::F(6), ctrl, HOST), b"\x1b[17;5~");
        assert_eq!(encode(Key::F(12), ctrl, HOST), b"\x1b[24;5~");
    }

    /// FR2: Alt-modified nav keys must take the CSI form, NOT the
    /// legacy `ESC + [seq]` Alt-prefix path which would corrupt the
    /// sequence. `Alt+Home` mods param is 1 + 2 = 3.
    #[test]
    fn alt_home_uses_csi_modifier_form_not_esc_prefix() {
        assert_eq!(
            encode(Key::Home, mods(false, false, true), HOST),
            b"\x1b[1;3H"
        );
    }

    /// FR2: `Alt+ArrowUp` mods param is 3; ensures the Alt ESC-prefix
    /// block did not fire (which would have produced `ESC\x1b[A`).
    #[test]
    fn alt_arrow_up_uses_csi_modifier_form_not_esc_prefix() {
        assert_eq!(
            encode(Key::Up, mods(false, false, true), HOST),
            b"\x1b[1;3A"
        );
    }

    /// FR2: `Insert` / `Delete` with modifiers use the tilde form.
    #[test]
    fn ctrl_insert_delete_use_tilde_form() {
        let ctrl = mods(true, false, false);
        assert_eq!(encode(Key::Insert, ctrl, HOST), b"\x1b[2;5~");
        assert_eq!(encode(Key::Delete, ctrl, HOST), b"\x1b[3;5~");
    }

    // ── FR2 regression: plain (NONE) bytes unchanged ─────────

    /// TS-16: `Home` with no modifier still emits `ESC[H`.
    #[test]
    fn plain_home_unchanged_regression() {
        assert_eq!(encode(Key::Home, Modifiers::NONE, HOST), b"\x1b[H");
    }

    /// TS-17: `PageUp` with no modifier still emits `ESC[5~`.
    #[test]
    fn plain_pageup_unchanged_regression() {
        assert_eq!(encode(Key::PageUp, Modifiers::NONE, HOST), b"\x1b[5~");
    }

    /// TS-18: `F1` with no modifier still emits `ESC OP` (SS3 legacy).
    #[test]
    fn plain_f1_unchanged_regression() {
        assert_eq!(encode(Key::F(1), Modifiers::NONE, HOST), b"\x1bOP");
    }

    #[test]
    fn bracketed_paste_wrap() {
        assert_eq!(
            wrap_bracketed_paste(b"hello\nworld"),
            b"\x1b[200~hello\nworld\x1b[201~"
        );
    }
}
