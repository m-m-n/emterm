//! Kitty Graphics Protocol APC scanner for the PTY reader thread.
//!
//! Scans raw PTY output for Kitty APC sequences and generates OK responses
//! that are written directly to the PTY master fd via `libc::write()`.
//! This bypasses ALL intermediate layers (writer channel, writer thread,
//! WebView, WASM, Tauri IPC), providing true zero-latency response delivery
//! from within the reader thread's read loop.
//!
//! Without this, the response goes through:
//!   WASM → JS callback → invoke("pty_write") → writer channel → PTY
//! which takes milliseconds due to Tauri IPC, allowing the CLI to exit
//! first and the shell to restore cooked mode (ECHO on), causing the
//! response to be echoed as garbage text on screen.

/// Maximum control data length we'll buffer (before `;` separator).
/// Kitty control data is typically < 100 bytes (e.g., "a=T,i=42,p=7,f=100,m=0").
const MAX_CONTROL_LEN: usize = 256;

/// State machine for scanning PTY output bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanState {
    /// Normal output, scanning for ESC (0x1B).
    Normal,
    /// Saw ESC, waiting for `_` (APC start) or other.
    EscSeen,
    /// Just entered APC (ESC _), checking if first byte is `G` (Kitty).
    ApcFirstByte,
    /// In Kitty APC control section (after `G`, before `;` or ESC\).
    KittyControl,
    /// Saw ESC inside Kitty control section, checking for `\` (ST).
    KittyControlEsc,
    /// Skipping content (APC payload after `;`, or non-Kitty APC) until ESC\.
    SkipToST,
    /// Saw ESC while skipping, checking for `\` (ST).
    SkipEsc,
}

/// Scans raw PTY output for Kitty Graphics Protocol APC sequences.
///
/// When a complete Kitty APC final chunk or query is detected, an OK response
/// is written directly to the PTY master fd via `libc::write()`, providing
/// true zero-latency delivery from the reader thread.
pub struct KittyScanner {
    state: ScanState,
    /// Buffer for Kitty control data (key=value pairs before `;`).
    control_buf: Vec<u8>,
    /// Whether the current APC started with `G` (Kitty).
    is_kitty: bool,
}

impl KittyScanner {
    pub fn new() -> Self {
        Self {
            state: ScanState::Normal,
            control_buf: Vec::with_capacity(64),
            is_kitty: false,
        }
    }

    /// Process a chunk of bytes from PTY output.
    ///
    /// When a Kitty APC final chunk is detected, the OK response is written
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
                    if byte == b'_' {
                        // APC start
                        self.control_buf.clear();
                        self.is_kitty = false;
                        ScanState::ApcFirstByte
                    } else if byte == 0x1B {
                        // Consecutive ESC (e.g. DCS passthrough doubles ESCs).
                        // Stay in EscSeen to detect the following `_`.
                        ScanState::EscSeen
                    } else {
                        // Not APC, return to normal
                        ScanState::Normal
                    }
                }

                ScanState::ApcFirstByte => {
                    if byte == b'G' {
                        self.is_kitty = true;
                        ScanState::KittyControl
                    } else if byte == 0x1B {
                        // Immediate ESC — could be ST
                        ScanState::SkipEsc
                    } else {
                        // Non-Kitty APC, skip to ST
                        ScanState::SkipToST
                    }
                }

                ScanState::KittyControl => {
                    if byte == b';' {
                        // Control data complete, skip payload until ST
                        ScanState::SkipToST
                    } else if byte == 0x1B {
                        // Possible ST start
                        ScanState::KittyControlEsc
                    } else if self.control_buf.len() < MAX_CONTROL_LEN {
                        self.control_buf.push(byte);
                        ScanState::KittyControl
                    } else {
                        // Control data too long, skip to ST
                        self.is_kitty = false;
                        ScanState::SkipToST
                    }
                }

                ScanState::KittyControlEsc => {
                    if byte == b'\\' {
                        // ST — APC complete (control only, no payload)
                        if self.is_kitty {
                            self.handle_complete(master_fd);
                        }
                        ScanState::Normal
                    } else {
                        // Not ST, ESC was part of control data (shouldn't happen, but be lenient)
                        if self.control_buf.len() + 2 <= MAX_CONTROL_LEN {
                            self.control_buf.push(0x1B);
                            self.control_buf.push(byte);
                        }
                        ScanState::KittyControl
                    }
                }

                ScanState::SkipToST => {
                    if byte == 0x1B {
                        ScanState::SkipEsc
                    } else {
                        ScanState::SkipToST
                    }
                }

                ScanState::SkipEsc => {
                    if byte == b'\\' {
                        // ST — APC complete
                        if self.is_kitty {
                            self.handle_complete(master_fd);
                        }
                        ScanState::Normal
                    } else {
                        // Not ST
                        ScanState::SkipToST
                    }
                }
            };
        }
    }

    /// Called when a complete Kitty APC is detected.
    /// Parses control data and writes OK response directly to the PTY master fd.
    fn handle_complete(&self, master_fd: i32) {
        let control = &self.control_buf;

        let mut action: u8 = b't'; // default: transmit-and-display
        let mut image_id: Option<u32> = None;
        let mut placement_id: Option<u32> = None;
        let mut quiet: Option<u8> = None;
        let mut more_chunks = false;

        for pair in control.split(|&b| b == b',') {
            if let Some(eq_pos) = pair.iter().position(|&b| b == b'=') {
                let key = &pair[..eq_pos];
                let val = &pair[eq_pos + 1..];
                match key {
                    b"a" => {
                        if let Some(&first) = val.first() {
                            action = first;
                        }
                    }
                    b"i" => {
                        image_id = parse_u32(val);
                    }
                    b"p" => {
                        placement_id = parse_u32(val);
                    }
                    b"q" => {
                        quiet = parse_u32(val).map(|v| v as u8);
                    }
                    b"m" => {
                        if val == b"1" {
                            more_chunks = true;
                        }
                    }
                    _ => {}
                }
            }
        }

        // Continuation chunk (m=1) — no response needed
        if more_chunks && action != b'q' {
            return;
        }

        // Suppress response for q=1 or q=2
        if quiet == Some(1) || quiet == Some(2) {
            return;
        }

        // For non-query actions, suppress response unless q=0 is explicitly set.
        // Many CLI tools (e.g. kitten icat) send delete/transmit commands without
        // q=2 and don't read the response, causing it to leak into the shell as
        // garbage text. Query actions (a=q) always respond since they are used
        // for capability detection (e.g. DetectSupport) and the caller reads them.
        if action != b'q' && quiet != Some(0) {
            return;
        }

        // Generate OK response and write directly to PTY master fd.
        // Using libc::write() provides true zero-latency delivery — the response
        // reaches the kernel line discipline before we even return from this function,
        // and certainly before the CLI process can exit and restore cooked mode.
        let response = build_ok_response(image_id, placement_id);
        unsafe {
            libc::write(
                master_fd,
                response.as_ptr() as *const libc::c_void,
                response.len(),
            );
        }
    }
}

/// Parse a u32 from ASCII decimal bytes.
fn parse_u32(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() {
        return None;
    }
    let mut result: u32 = 0;
    for &b in bytes {
        if !b.is_ascii_digit() {
            return None;
        }
        result = result.checked_mul(10)?.checked_add((b - b'0') as u32)?;
    }
    Some(result)
}

/// Build a Kitty OK response: `ESC _G [i=<id>][,p=<pid>] ;OK ESC \`
fn build_ok_response(image_id: Option<u32>, placement_id: Option<u32>) -> Vec<u8> {
    let mut buf = Vec::with_capacity(32);
    buf.extend_from_slice(b"\x1b_G");

    if let Some(id) = image_id {
        buf.extend_from_slice(b"i=");
        write_u32_decimal(&mut buf, id);
    }

    if let Some(pid) = placement_id {
        if image_id.is_some() {
            buf.push(b',');
        }
        buf.extend_from_slice(b"p=");
        write_u32_decimal(&mut buf, pid);
    }

    buf.extend_from_slice(b";OK\x1b\\");
    buf
}

/// Append a u32 as decimal digits to a Vec.
fn write_u32_decimal(buf: &mut Vec<u8>, val: u32) {
    if val == 0 {
        buf.push(b'0');
        return;
    }
    let start = buf.len();
    let mut n = val;
    while n > 0 {
        buf.push((n % 10) as u8 + b'0');
        n /= 10;
    }
    buf[start..].reverse();
}

#[cfg(test)]
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
        // Set non-blocking
        unsafe {
            let flags = libc::fcntl(read_fd, libc::F_GETFL);
            libc::fcntl(read_fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }
        let mut buf = vec![0u8; 4096];
        let mut result = Vec::new();
        loop {
            let n = unsafe {
                libc::read(read_fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len())
            };
            if n <= 0 {
                break;
            }
            result.extend_from_slice(&buf[..n as usize]);
        }
        result
    }

    /// Close both ends of a pipe.
    fn close_pipe(read_fd: i32, write_fd: i32) {
        unsafe {
            libc::close(read_fd);
            libc::close(write_fd);
        }
    }

    /// Helper: process a full APC sequence and collect responses from pipe.
    fn scan_and_collect(data: &[u8]) -> Vec<Vec<u8>> {
        let (read_fd, write_fd) = make_pipe();
        let mut scanner = KittyScanner::new();
        scanner.process(data, write_fd);

        let all_data = read_pipe(read_fd);
        close_pipe(read_fd, write_fd);

        // Split by APC responses (each starts with ESC _ G and ends with ESC \)
        let mut responses = Vec::new();
        let mut i = 0;
        while i < all_data.len() {
            if all_data[i] == 0x1B && i + 1 < all_data.len() && all_data[i + 1] == b'_' {
                // Find the end (ESC \)
                let mut j = i + 2;
                while j + 1 < all_data.len() {
                    if all_data[j] == 0x1B && all_data[j + 1] == b'\\' {
                        responses.push(all_data[i..j + 2].to_vec());
                        i = j + 2;
                        break;
                    }
                    j += 1;
                }
                if j + 1 >= all_data.len() {
                    break;
                }
            } else {
                i += 1;
            }
        }
        responses
    }

    // ── Query (a=q) tests ────────────────────────────────

    #[test]
    fn query_with_id() {
        let data = b"\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\";
        let responses = scan_and_collect(data);
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0], b"\x1b_Gi=31;OK\x1b\\");
    }

    #[test]
    fn query_without_id() {
        let data = b"\x1b_Ga=q;AAAA\x1b\\";
        let responses = scan_and_collect(data);
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0], b"\x1b_G;OK\x1b\\");
    }

    #[test]
    fn query_quiet_suppressed() {
        let data = b"\x1b_Gi=31,a=q,q=1;AAAA\x1b\\";
        let responses = scan_and_collect(data);
        assert_eq!(responses.len(), 0);
    }

    #[test]
    fn query_no_payload() {
        let data = b"\x1b_Ga=q\x1b\\";
        let responses = scan_and_collect(data);
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0], b"\x1b_G;OK\x1b\\");
    }

    #[test]
    fn query_large_id() {
        let data = b"\x1b_Gi=4294967295,a=q;\x1b\\";
        let responses = scan_and_collect(data);
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0], b"\x1b_Gi=4294967295;OK\x1b\\");
    }

    // ── Non-query final chunk tests ──────────────────────
    // Non-query actions suppress responses by default (q not set).
    // Only respond when q=0 is explicitly set.

    #[test]
    fn transmit_final_chunk_suppressed_by_default() {
        // No q= specified → suppressed (prevents leak from CLI tools)
        let data = b"\x1b_Ga=T,i=42,f=100;iVBORw0KGgo=\x1b\\";
        let responses = scan_and_collect(data);
        assert_eq!(responses.len(), 0);
    }

    #[test]
    fn transmit_final_chunk_explicit_q0_responds() {
        // q=0 explicitly set → respond
        let data = b"\x1b_Ga=T,i=42,q=0,f=100;iVBORw0KGgo=\x1b\\";
        let responses = scan_and_collect(data);
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0], b"\x1b_Gi=42;OK\x1b\\");
    }

    #[test]
    fn default_action_final_chunk_suppressed() {
        // Default action (a=t) without q → suppressed
        let data = b"\x1b_Gi=99,f=100;iVBORw0KGgo=\x1b\\";
        let responses = scan_and_collect(data);
        assert_eq!(responses.len(), 0);
    }

    #[test]
    fn default_action_explicit_q0_responds() {
        let data = b"\x1b_Gi=99,q=0,f=100;iVBORw0KGgo=\x1b\\";
        let responses = scan_and_collect(data);
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0], b"\x1b_Gi=99;OK\x1b\\");
    }

    #[test]
    fn final_chunk_with_placement_id_suppressed() {
        let data = b"\x1b_Ga=T,i=42,p=7,f=100;iVBORw0KGgo=\x1b\\";
        let responses = scan_and_collect(data);
        assert_eq!(responses.len(), 0);
    }

    #[test]
    fn final_chunk_with_placement_id_explicit_q0() {
        let data = b"\x1b_Ga=T,i=42,p=7,q=0,f=100;iVBORw0KGgo=\x1b\\";
        let responses = scan_and_collect(data);
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0], b"\x1b_Gi=42,p=7;OK\x1b\\");
    }

    #[test]
    fn final_chunk_quiet_suppressed() {
        let data = b"\x1b_Ga=T,i=42,q=1;iVBORw0KGgo=\x1b\\";
        let responses = scan_and_collect(data);
        assert_eq!(responses.len(), 0);
    }

    #[test]
    fn final_chunk_quiet2_suppressed() {
        let data = b"\x1b_Ga=T,i=42,q=2;iVBORw0KGgo=\x1b\\";
        let responses = scan_and_collect(data);
        assert_eq!(responses.len(), 0);
    }

    // ── Delete action tests ─────────────────────────────
    // Delete commands (a=d) are suppressed by default — this is the
    // root cause of kitten icat response leak.

    #[test]
    fn delete_suppressed_by_default() {
        // kitten icat --clear sends this without q
        let data = b"\x1b_Ga=d,d=a;\x1b\\";
        let responses = scan_and_collect(data);
        assert_eq!(responses.len(), 0);
    }

    #[test]
    fn delete_by_range_suppressed() {
        let data = b"\x1b_Ga=d,d=R,x=0,y=4294967295;\x1b\\";
        let responses = scan_and_collect(data);
        assert_eq!(responses.len(), 0);
    }

    #[test]
    fn delete_explicit_q0_responds() {
        let data = b"\x1b_Ga=d,d=a,q=0;\x1b\\";
        let responses = scan_and_collect(data);
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0], b"\x1b_G;OK\x1b\\");
    }

    // ── Continuation chunk tests ─────────────────────────

    #[test]
    fn more_chunks_no_response() {
        let data = b"\x1b_Ga=T,i=42,m=1;iVBORw0KGgo=\x1b\\";
        let responses = scan_and_collect(data);
        assert_eq!(responses.len(), 0);
    }

    #[test]
    fn explicit_m0_is_final_suppressed_without_q0() {
        let data = b"\x1b_Ga=T,i=42,m=0;iVBORw0KGgo=\x1b\\";
        let responses = scan_and_collect(data);
        assert_eq!(responses.len(), 0);
    }

    #[test]
    fn explicit_m0_is_final_with_q0() {
        let data = b"\x1b_Ga=T,i=42,m=0,q=0;iVBORw0KGgo=\x1b\\";
        let responses = scan_and_collect(data);
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0], b"\x1b_Gi=42;OK\x1b\\");
    }

    // ── Non-Kitty APC tests ─────────────────────────────

    #[test]
    fn non_kitty_apc() {
        let data = b"\x1b_Hello\x1b\\";
        let responses = scan_and_collect(data);
        assert_eq!(responses.len(), 0);
    }

    #[test]
    fn empty_apc() {
        let data = b"\x1b_\x1b\\";
        let responses = scan_and_collect(data);
        assert_eq!(responses.len(), 0);
    }

    // ── Partial read / multi-chunk tests ─────────────────

    #[test]
    fn partial_reads_across_buffers_suppressed() {
        // Non-query without q=0 → suppressed even across partial reads
        let (read_fd, write_fd) = make_pipe();
        let mut scanner = KittyScanner::new();

        scanner.process(b"\x1b", write_fd);
        scanner.process(b"_Gi=42", write_fd);
        scanner.process(b",a=T;payload", write_fd);
        scanner.process(b"\x1b\\", write_fd);

        let all_data = read_pipe(read_fd);
        close_pipe(read_fd, write_fd);
        assert_eq!(all_data, b"");
    }

    #[test]
    fn partial_reads_across_buffers_with_q0() {
        let (read_fd, write_fd) = make_pipe();
        let mut scanner = KittyScanner::new();

        scanner.process(b"\x1b", write_fd);
        scanner.process(b"_Gi=42", write_fd);
        scanner.process(b",a=T,q=0;payload", write_fd);
        scanner.process(b"\x1b\\", write_fd);

        let all_data = read_pipe(read_fd);
        close_pipe(read_fd, write_fd);
        assert_eq!(all_data, b"\x1b_Gi=42;OK\x1b\\");
    }

    #[test]
    fn multiple_apcs_in_one_read_suppressed() {
        // Non-query without q=0 → all suppressed
        let mut data = Vec::new();
        data.extend_from_slice(b"\x1b_Gi=1,a=T;data\x1b\\");
        data.extend_from_slice(b"normal text");
        data.extend_from_slice(b"\x1b_Gi=2,a=T;data\x1b\\");

        let responses = scan_and_collect(&data);
        assert_eq!(responses.len(), 0);
    }

    #[test]
    fn multiple_queries_in_one_read() {
        // Query actions always respond
        let mut data = Vec::new();
        data.extend_from_slice(b"\x1b_Gi=1,a=q;data\x1b\\");
        data.extend_from_slice(b"normal text");
        data.extend_from_slice(b"\x1b_Gi=2,a=q;data\x1b\\");

        let responses = scan_and_collect(&data);
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0], b"\x1b_Gi=1;OK\x1b\\");
        assert_eq!(responses[1], b"\x1b_Gi=2;OK\x1b\\");
    }

    #[test]
    fn multi_chunk_sequence_suppressed() {
        // Multi-chunk transmit without q=0 → all suppressed
        let mut data = Vec::new();
        data.extend_from_slice(b"\x1b_Ga=T,i=42,m=1;chunk1data\x1b\\");
        data.extend_from_slice(b"\x1b_Gm=1;chunk2data\x1b\\");
        data.extend_from_slice(b"\x1b_Gi=42,m=0;chunk3data\x1b\\");

        let responses = scan_and_collect(&data);
        assert_eq!(responses.len(), 0);
    }

    #[test]
    fn multi_chunk_sequence_with_q0() {
        let mut data = Vec::new();
        data.extend_from_slice(b"\x1b_Ga=T,i=42,q=0,m=1;chunk1data\x1b\\");
        data.extend_from_slice(b"\x1b_Gm=1,q=0;chunk2data\x1b\\");
        data.extend_from_slice(b"\x1b_Gi=42,m=0,q=0;chunk3data\x1b\\");

        let responses = scan_and_collect(&data);
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0], b"\x1b_Gi=42;OK\x1b\\");
    }

    // ── Helper function tests ───────────────────────────

    #[test]
    fn test_parse_u32() {
        assert_eq!(parse_u32(b"0"), Some(0));
        assert_eq!(parse_u32(b"31"), Some(31));
        assert_eq!(parse_u32(b"4294967295"), Some(4294967295));
        assert_eq!(parse_u32(b""), None);
        assert_eq!(parse_u32(b"abc"), None);
        assert_eq!(parse_u32(b"12x"), None);
    }

    #[test]
    fn test_build_ok_response() {
        assert_eq!(
            build_ok_response(Some(42), None),
            b"\x1b_Gi=42;OK\x1b\\"
        );
        assert_eq!(
            build_ok_response(Some(42), Some(7)),
            b"\x1b_Gi=42,p=7;OK\x1b\\"
        );
        assert_eq!(build_ok_response(None, None), b"\x1b_G;OK\x1b\\");
        assert_eq!(
            build_ok_response(None, Some(3)),
            b"\x1b_Gp=3;OK\x1b\\"
        );
    }

    #[test]
    fn test_write_u32_decimal() {
        let mut buf = Vec::new();
        write_u32_decimal(&mut buf, 0);
        assert_eq!(&buf, b"0");

        let mut buf = Vec::new();
        write_u32_decimal(&mut buf, 12345);
        assert_eq!(&buf, b"12345");

        let mut buf = Vec::new();
        write_u32_decimal(&mut buf, 4294967295);
        assert_eq!(&buf, b"4294967295");
    }
}
