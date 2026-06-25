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

    #[test]
    fn bracketed_paste_wrap() {
        assert_eq!(
            wrap_bracketed_paste(b"hello\nworld"),
            b"\x1b[200~hello\nworld\x1b[201~"
        );
    }
}
