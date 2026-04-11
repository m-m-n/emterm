//! Device query scanner for OSC color queries.
//!
//! Scans raw PTY output for OSC 10/11/12 color query sequences
//! (`ESC ] <10|11|12> ; ? <BEL|ESC\>`) and writes responses directly to the
//! PTY master fd via `libc::write()`. This bypasses ALL intermediate layers
//! (writer channel, writer thread, WebView, WASM, Tauri IPC), providing true
//! zero-latency response delivery from within the reader thread's read loop.
//!
//! Without this, the response goes through:
//!   WASM → JS callback → invoke("pty_write") → writer channel → PTY
//! which takes milliseconds due to Tauri IPC, allowing the CLI to exit
//! first and the shell to restore cooked mode (ECHO on), causing the
//! response to be echoed as garbage text on screen.
//!
//! Supported queries (single-color form only):
//!   - `ESC ] 10 ; ? ST` → default foreground color
//!   - `ESC ] 11 ; ? ST` → default background color
//!   - `ESC ] 12 ; ? ST` → default cursor color
//! where ST is either `BEL` (0x07) or `ESC \`.
//!
//! Chained queries (e.g. `ESC ] 10 ; ? ; ? ; ? ST`) and non-query SET forms
//! are intentionally ignored; they fall through to the normal WASM path.

/// State machine for scanning PTY output bytes for OSC color queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanState {
    /// Normal output, scanning for ESC (0x1B).
    Normal,
    /// Saw ESC, waiting for `]` (OSC start) or other.
    EscSeen,
    /// In OSC parameter phase, reading decimal digits before `;`.
    OscParam,
    /// Saw the first `;` after the parameter, expecting `?` (query marker).
    OscAfterSemi,
    /// Saw `?` after `;`, expecting terminator (BEL or ESC\).
    OscQueryEnd,
    /// Saw ESC after `?`, expecting `\` for ST.
    OscQueryEsc,
    /// OSC payload is not a simple single-color query — skip to terminator.
    OscDiscard,
    /// Saw ESC while discarding, expecting `\` for ST.
    OscDiscardEsc,
}

/// Scans raw PTY output for OSC color query sequences.
///
/// When a complete OSC 10/11/12 query is detected, a hardcoded response is
/// written directly to the PTY master fd via `libc::write()`, providing
/// true zero-latency delivery from the reader thread.
pub struct DeviceQueryScanner {
    state: ScanState,
    /// Parameter value accumulated during `OscParam` (e.g. 10, 11, 12).
    param: u16,
    /// Whether we've overflowed the parameter (ignore this OSC if true).
    param_overflow: bool,
}

impl Default for DeviceQueryScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl DeviceQueryScanner {
    pub fn new() -> Self {
        Self {
            state: ScanState::Normal,
            param: 0,
            param_overflow: false,
        }
    }

    /// Process a chunk of bytes from PTY output.
    ///
    /// When a complete OSC color query is detected, its response is written
    /// directly to the PTY master fd via `libc::write()`.
    ///
    /// # Arguments
    /// * `data` - Raw bytes from PTY read
    /// * `master_fd` - The PTY master file descriptor for direct write
    pub fn process(&mut self, data: &[u8], master_fd: i32) {
        for &byte in data {
            self.state = match self.state {
                ScanState::Normal => {
                    if byte == 0x1B {
                        ScanState::EscSeen
                    } else {
                        ScanState::Normal
                    }
                }

                ScanState::EscSeen => {
                    if byte == b']' {
                        // OSC start
                        self.param = 0;
                        self.param_overflow = false;
                        ScanState::OscParam
                    } else if byte == 0x1B {
                        // Consecutive ESC — stay in EscSeen
                        ScanState::EscSeen
                    } else {
                        // Not OSC
                        ScanState::Normal
                    }
                }

                ScanState::OscParam => {
                    if byte.is_ascii_digit() {
                        let d = (byte - b'0') as u16;
                        match self.param.checked_mul(10).and_then(|v| v.checked_add(d)) {
                            Some(v) => self.param = v,
                            None => self.param_overflow = true,
                        }
                        ScanState::OscParam
                    } else if byte == b';' {
                        ScanState::OscAfterSemi
                    } else if byte == 0x07 {
                        // BEL — OSC terminated without semicolon (no data)
                        ScanState::Normal
                    } else if byte == 0x1B {
                        // Unexpected ESC in param; could be ST — but no data, nothing to respond
                        ScanState::OscDiscardEsc
                    } else {
                        // Invalid param char — bail out, skip to terminator
                        ScanState::OscDiscard
                    }
                }

                ScanState::OscAfterSemi => {
                    if byte == b'?' {
                        ScanState::OscQueryEnd
                    } else {
                        // Not a query — could be a SET or something else. Skip to terminator.
                        ScanState::OscDiscard
                    }
                }

                ScanState::OscQueryEnd => {
                    if byte == 0x07 {
                        // BEL terminator
                        self.maybe_dispatch(master_fd);
                        ScanState::Normal
                    } else if byte == 0x1B {
                        ScanState::OscQueryEsc
                    } else {
                        // Additional data after `?` (e.g. chained query `?;?;?`) — not our
                        // simple single-color form. Discard.
                        ScanState::OscDiscard
                    }
                }

                ScanState::OscQueryEsc => {
                    if byte == b'\\' {
                        // ST terminator
                        self.maybe_dispatch(master_fd);
                        ScanState::Normal
                    } else {
                        // Not ST — bail out to discard
                        ScanState::OscDiscard
                    }
                }

                ScanState::OscDiscard => {
                    if byte == 0x07 {
                        ScanState::Normal
                    } else if byte == 0x1B {
                        ScanState::OscDiscardEsc
                    } else {
                        ScanState::OscDiscard
                    }
                }

                ScanState::OscDiscardEsc => {
                    if byte == b'\\' {
                        ScanState::Normal
                    } else if byte == 0x1B {
                        ScanState::OscDiscardEsc
                    } else {
                        ScanState::OscDiscard
                    }
                }
            };
        }
    }

    /// Emit the hardcoded response for the current OSC parameter, if supported.
    ///
    /// Responses use the default eMterm theme colors (black bg, white fg/cursor).
    /// This is sufficient for color-profile detection (the common use case);
    /// exact values are not critical since the TS side still tracks user
    /// customizations for rendering.
    fn maybe_dispatch(&self, master_fd: i32) {
        if self.param_overflow {
            return;
        }
        let response: &[u8] = match self.param {
            10 => b"\x1b]10;rgb:ffff/ffff/ffff\x1b\\",
            11 => b"\x1b]11;rgb:0000/0000/0000\x1b\\",
            12 => b"\x1b]12;rgb:ffff/ffff/ffff\x1b\\",
            _ => return,
        };

        // Write directly to the PTY master fd via libc::write() for true
        // zero-latency delivery — the response reaches the kernel line
        // discipline before we return from this function, and certainly
        // before the querying CLI can exit and restore cooked mode.
        #[cfg(unix)]
        unsafe {
            libc::write(
                master_fd,
                response.as_ptr() as *const libc::c_void,
                response.len(),
            );
        }
        #[cfg(not(unix))]
        let _ = (master_fd, response);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// Create a pipe and return (read_fd, write_fd).
    fn make_pipe() -> (i32, i32) {
        let mut fds = [0i32; 2];
        unsafe {
            assert_eq!(libc::pipe(fds.as_mut_ptr()), 0);
        }
        (fds[0], fds[1])
    }

    /// Read all available data from a pipe read fd (non-blocking).
    fn read_pipe(read_fd: i32) -> Vec<u8> {
        unsafe {
            let flags = libc::fcntl(read_fd, libc::F_GETFL);
            libc::fcntl(read_fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }
        let mut buf = vec![0u8; 4096];
        let mut result = Vec::new();
        loop {
            let n =
                unsafe { libc::read(read_fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
            if n <= 0 {
                break;
            }
            result.extend_from_slice(&buf[..n as usize]);
        }
        result
    }

    fn close_pipe(read_fd: i32, write_fd: i32) {
        unsafe {
            libc::close(read_fd);
            libc::close(write_fd);
        }
    }

    fn scan_and_collect(data: &[u8]) -> Vec<u8> {
        let (read_fd, write_fd) = make_pipe();
        let mut scanner = DeviceQueryScanner::new();
        scanner.process(data, write_fd);
        let result = read_pipe(read_fd);
        close_pipe(read_fd, write_fd);
        result
    }

    #[test]
    fn osc11_query_st_terminated() {
        // OSC 11 query with ESC \ terminator — the form used by charmbracelet/colorprofile.
        let data = b"\x1b]11;?\x1b\\";
        let out = scan_and_collect(data);
        assert_eq!(out, b"\x1b]11;rgb:0000/0000/0000\x1b\\");
    }

    #[test]
    fn osc11_query_bel_terminated() {
        let data = b"\x1b]11;?\x07";
        let out = scan_and_collect(data);
        assert_eq!(out, b"\x1b]11;rgb:0000/0000/0000\x1b\\");
    }

    #[test]
    fn osc10_query() {
        let data = b"\x1b]10;?\x1b\\";
        let out = scan_and_collect(data);
        assert_eq!(out, b"\x1b]10;rgb:ffff/ffff/ffff\x1b\\");
    }

    #[test]
    fn osc12_query() {
        let data = b"\x1b]12;?\x1b\\";
        let out = scan_and_collect(data);
        assert_eq!(out, b"\x1b]12;rgb:ffff/ffff/ffff\x1b\\");
    }

    #[test]
    fn osc11_set_is_ignored() {
        // OSC 11 SET (not query) should NOT produce a response — TS handles it.
        let data = b"\x1b]11;rgb:ff/ff/ff\x1b\\";
        let out = scan_and_collect(data);
        assert!(out.is_empty(), "SET must not produce a response");
    }

    #[test]
    fn osc13_query_is_ignored() {
        // OSC 13 (highlight fg) — not in our supported set.
        let data = b"\x1b]13;?\x1b\\";
        let out = scan_and_collect(data);
        assert!(out.is_empty());
    }

    #[test]
    fn osc4_query_is_ignored() {
        // OSC 4 query (palette) — not handled by this scanner.
        let data = b"\x1b]4;1;?\x1b\\";
        let out = scan_and_collect(data);
        assert!(out.is_empty());
    }

    #[test]
    fn osc11_chained_query_ignored() {
        // Chained query `?;?;?` — not our simple form.
        let data = b"\x1b]10;?;?;?\x1b\\";
        let out = scan_and_collect(data);
        assert!(out.is_empty());
    }

    #[test]
    fn osc11_query_followed_by_text() {
        // Query inside a larger chunk (em-agent's real pattern: query + CSI 6n + usage text).
        let data = b"\x1b]11;?\x1b\\\x1b[6nusage: em-agent\n";
        let out = scan_and_collect(data);
        assert_eq!(out, b"\x1b]11;rgb:0000/0000/0000\x1b\\");
    }

    #[test]
    fn osc11_query_split_across_chunks() {
        let (read_fd, write_fd) = make_pipe();
        let mut scanner = DeviceQueryScanner::new();
        scanner.process(b"\x1b]1", write_fd);
        scanner.process(b"1;?", write_fd);
        scanner.process(b"\x1b\\", write_fd);
        let result = read_pipe(read_fd);
        close_pipe(read_fd, write_fd);
        assert_eq!(result, b"\x1b]11;rgb:0000/0000/0000\x1b\\");
    }

    #[test]
    fn plain_text_produces_no_response() {
        let data = b"hello world\n";
        let out = scan_and_collect(data);
        assert!(out.is_empty());
    }

    #[test]
    fn multiple_queries_in_one_chunk() {
        let data = b"\x1b]10;?\x1b\\\x1b]11;?\x1b\\\x1b]12;?\x1b\\";
        let out = scan_and_collect(data);
        let expected: Vec<u8> = [
            b"\x1b]10;rgb:ffff/ffff/ffff\x1b\\".as_slice(),
            b"\x1b]11;rgb:0000/0000/0000\x1b\\".as_slice(),
            b"\x1b]12;rgb:ffff/ffff/ffff\x1b\\".as_slice(),
        ]
        .concat();
        assert_eq!(out, expected);
    }

    #[test]
    fn osc_with_overflowed_param_ignored() {
        // A param that overflows u16 should be ignored silently.
        let data = b"\x1b]999999999;?\x1b\\";
        let out = scan_and_collect(data);
        assert!(out.is_empty());
    }

    #[test]
    fn osc_kitty_apc_not_confused() {
        // Kitty APC uses ESC _ not ESC ] — should not trigger.
        let data = b"\x1b_Ga=q;AAAA\x1b\\";
        let out = scan_and_collect(data);
        assert!(out.is_empty());
    }
}
