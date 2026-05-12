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

/// Encode a key event to the byte sequence a normal terminal would write.
///
/// Returns an empty buffer for unsupported combinations rather than panicking
/// so the caller can fall back silently. Unknown chord combinations log a
/// warning at the call site.
pub fn encode(key: Key, mods: Modifiers) -> Vec<u8> {
    use Key::*;

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
        Tab => out.push(b'\t'),
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

    #[test]
    fn printable_ascii() {
        assert_eq!(encode(Key::Char('a'), Modifiers::NONE), b"a");
        assert_eq!(encode(Key::Char('Z'), Modifiers::NONE), b"Z");
        assert_eq!(encode(Key::Char('1'), Modifiers::NONE), b"1");
    }

    #[test]
    fn enter_tab_backspace_escape() {
        assert_eq!(encode(Key::Enter, Modifiers::NONE), b"\r");
        assert_eq!(encode(Key::Tab, Modifiers::NONE), b"\t");
        assert_eq!(encode(Key::Backspace, Modifiers::NONE), b"\x7f");
        assert_eq!(encode(Key::Escape, Modifiers::NONE), b"\x1b");
    }

    #[test]
    fn arrow_keys() {
        assert_eq!(encode(Key::Up, Modifiers::NONE), b"\x1b[A");
        assert_eq!(encode(Key::Down, Modifiers::NONE), b"\x1b[B");
        assert_eq!(encode(Key::Right, Modifiers::NONE), b"\x1b[C");
        assert_eq!(encode(Key::Left, Modifiers::NONE), b"\x1b[D");
    }

    #[test]
    fn nav_and_function_keys() {
        assert_eq!(encode(Key::Home, Modifiers::NONE), b"\x1b[H");
        assert_eq!(encode(Key::End, Modifiers::NONE), b"\x1b[F");
        assert_eq!(encode(Key::PageUp, Modifiers::NONE), b"\x1b[5~");
        assert_eq!(encode(Key::PageDown, Modifiers::NONE), b"\x1b[6~");
        assert_eq!(encode(Key::Delete, Modifiers::NONE), b"\x1b[3~");
        assert_eq!(encode(Key::Insert, Modifiers::NONE), b"\x1b[2~");
        assert_eq!(encode(Key::F(1), Modifiers::NONE), b"\x1bOP");
        assert_eq!(encode(Key::F(5), Modifiers::NONE), b"\x1b[15~");
        assert_eq!(encode(Key::F(12), Modifiers::NONE), b"\x1b[24~");
        assert_eq!(encode(Key::F(99), Modifiers::NONE), b"");
    }

    #[test]
    fn ctrl_letters() {
        assert_eq!(encode(Key::Char('c'), Modifiers::ctrl()), b"\x03");
        assert_eq!(encode(Key::Char('C'), Modifiers::ctrl()), b"\x03");
        assert_eq!(encode(Key::Char('a'), Modifiers::ctrl()), b"\x01");
        assert_eq!(encode(Key::Char('z'), Modifiers::ctrl()), b"\x1a");
    }

    #[test]
    fn ctrl_extras() {
        assert_eq!(encode(Key::Char('['), Modifiers::ctrl()), b"\x1b");
        assert_eq!(encode(Key::Char(' '), Modifiers::ctrl()), b"\x00");
        assert_eq!(encode(Key::Char('\\'), Modifiers::ctrl()), b"\x1c");
    }

    #[test]
    fn alt_prefixes_esc() {
        let mods = Modifiers {
            alt: true,
            ..Modifiers::NONE
        };
        assert_eq!(encode(Key::Char('b'), mods), b"\x1bb");
    }

    #[test]
    fn bracketed_paste_wrap() {
        assert_eq!(
            wrap_bracketed_paste(b"hello\nworld"),
            b"\x1b[200~hello\nworld\x1b[201~"
        );
    }
}
