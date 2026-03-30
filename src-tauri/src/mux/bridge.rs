//! Bridge process: stdin/stdout APC ↔ daemon socket forwarding.
//!
//! The bridge translates between APC escape sequences on stdin/stdout
//! and MuxMessage frames on the Unix domain socket.

use super::ipc::protocol::*;

/// Transport format for mux messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Transport {
    /// APC format (default, works on Linux).
    Apc = 0,
    /// OSC 9999 format (fallback for Windows ConPTY).
    Osc = 1,
}

/// Sentinel value indicating transport has not been detected yet.
const TRANSPORT_UNDETECTED: u8 = 0xFF;

/// Run the long-running bridge process.
///
/// Connects to the daemon via Unix socket, performs handshake, then
/// translates between APC on stdin/stdout and MuxMessage on the socket.
#[cfg(unix)]
pub(super) fn run_bridge(sock_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    rt.block_on(async { bridge_main_loop(sock_path).await })
}

#[cfg(not(unix))]
pub(super) fn run_bridge(_sock_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("Mux bridge is not supported on this platform");
    std::process::exit(1);
}

/// Set stdin to raw mode (non-canonical, no echo) so APC bytes arrive immediately.
/// Returns the original termios for restoration on exit.
#[cfg(unix)]
fn set_stdin_raw() -> Option<libc::termios> {
    use std::mem::MaybeUninit;
    unsafe {
        let mut orig = MaybeUninit::<libc::termios>::uninit();
        if libc::tcgetattr(libc::STDIN_FILENO, orig.as_mut_ptr()) != 0 {
            log::warn!("tcgetattr failed, stdin may not be a tty");
            return None;
        }
        let orig = orig.assume_init();
        let mut raw = orig;
        libc::cfmakeraw(&mut raw);
        if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) != 0 {
            log::warn!("tcsetattr failed");
            return None;
        }
        log::info!("stdin set to raw mode");
        Some(orig)
    }
}

/// Restore original termios settings.
#[cfg(unix)]
fn restore_stdin(orig: &libc::termios) {
    unsafe {
        libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, orig);
    }
    log::info!("stdin restored to original mode");
}

/// Async bridge main loop: handshake, then bidirectional APC/socket forwarding.
#[cfg(unix)]
async fn bridge_main_loop(sock_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixStream;

    // Set stdin to raw mode so APC escape sequences arrive byte-by-byte
    let orig_termios = set_stdin_raw();

    // Connect to daemon
    log::info!("Connecting to daemon at {:?}", sock_path);
    let stream = UnixStream::connect(sock_path)
        .await
        .map_err(|e| format!("Failed to connect to daemon at {:?}: {}", sock_path, e))?;
    log::info!("Socket connected");

    let (mut sock_reader, mut sock_writer) = tokio::io::split(stream);

    // Perform handshake
    log::info!("Sending Hello (protocol v{})", PROTOCOL_VERSION);
    let hello = HelloMsg {
        client_type: ClientType::Gui,
        protocol_version: PROTOCOL_VERSION,
    };
    let hello_msg = MuxMessage::control(MessageType::Hello, 0, &hello);
    let body = hello_msg.to_frame_body();
    let len = (body.len() as u32).to_be_bytes();
    sock_writer.write_all(&len).await?;
    sock_writer.write_all(&body).await?;
    sock_writer.flush().await?;

    // Read Welcome with 5-second timeout (handles non-eMterm terminals naturally)
    log::info!("Waiting for Welcome (5s timeout)");
    let welcome_msg = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut len_buf = [0u8; 4];
        sock_reader
            .read_exact(&mut len_buf)
            .await
            .map_err(|e| format!("read Welcome length: {}", e))?;
        let frame_len = u32::from_be_bytes(len_buf) as usize;
        if frame_len > MAX_FRAME_LENGTH {
            return Err("Frame too large during handshake".to_string());
        }
        let mut frame_buf = vec![0u8; frame_len];
        sock_reader
            .read_exact(&mut frame_buf)
            .await
            .map_err(|e| format!("read Welcome body: {}", e))?;
        MuxMessage::from_frame_body(&frame_buf).ok_or_else(|| "Invalid Welcome frame".to_string())
    })
    .await
    .map_err(|_| "Daemon did not respond within 5 seconds")?
    .map_err(|e: String| -> Box<dyn std::error::Error> { e.into() })?;
    if welcome_msg.msg_type != MessageType::Welcome {
        log::error!("Expected Welcome, got {:?}", welcome_msg.msg_type);
        return Err(format!("Expected Welcome, got {:?}", welcome_msg.msg_type).into());
    }
    log::info!("Handshake complete, received Welcome");

    // Write Welcome as both OSC and APC to stdout so GUI receives it
    // regardless of which transport the terminal supports.
    // OSC is sent FIRST because Windows ConPTY may corrupt stream state
    // when encountering APC (ESC _), potentially consuming subsequent data.
    let welcome_osc = welcome_msg.to_osc();
    let welcome_apc = welcome_msg.to_apc();
    {
        use std::io::Write;
        let stdout = std::io::stdout();
        let mut stdout = stdout.lock();
        stdout.write_all(welcome_osc.as_bytes())?;
        stdout.write_all(welcome_apc.as_bytes())?;
        stdout.flush()?;
    }

    // Bidirectional forwarding: stdin -> daemon, daemon -> stdout
    log::info!("Starting bidirectional forwarding");

    use std::sync::atomic::{AtomicU8, Ordering};
    use std::sync::Arc;
    let transport = Arc::new(AtomicU8::new(TRANSPORT_UNDETECTED));
    let transport_for_stdin = Arc::clone(&transport);
    let transport_for_stdout = Arc::clone(&transport);

    let stdin_to_daemon = async {
        let mut stdin = tokio::io::stdin();
        let mut parser = StdinApcParser::new();
        let mut buf = [0u8; 8192];
        loop {
            let n = match stdin.read(&mut buf).await {
                Ok(0) => {
                    log::info!("stdin EOF, stopping stdin→daemon");
                    break;
                }
                Ok(n) => n,
                Err(e) => {
                    log::warn!("stdin read error: {}, stopping stdin→daemon", e);
                    break;
                }
            };

            let actions = parser.feed(&buf[..n]);
            for action in actions {
                match action {
                    StdinAction::MuxMessage(msg, t) => {
                        // Set transport on first detected message
                        let _ = transport_for_stdin.compare_exchange(
                            TRANSPORT_UNDETECTED,
                            t as u8,
                            Ordering::AcqRel,
                            Ordering::Relaxed,
                        );
                        log::info!(
                            "stdin→daemon: forwarding {:?} pane={} transport={:?}",
                            msg.msg_type,
                            msg.pane_id,
                            t
                        );
                        let body = msg.to_frame_body();
                        let len = (body.len() as u32).to_be_bytes();
                        if sock_writer.write_all(&len).await.is_err() {
                            log::warn!("stdin→daemon: socket write failed (len)");
                            return;
                        }
                        if sock_writer.write_all(&body).await.is_err() {
                            log::warn!("stdin→daemon: socket write failed (body)");
                            return;
                        }
                        let _ = sock_writer.flush().await;
                    }
                    StdinAction::Passthrough(_) => {
                        // Non-mux data from stdin is dropped in bridge mode
                    }
                }
            }
        }
    };

    let daemon_to_stdout = async {
        let mut len_buf = [0u8; 4];
        loop {
            if let Err(e) = sock_reader.read_exact(&mut len_buf).await {
                log::info!("Daemon socket read error: {}, stopping daemon→stdout", e);
                break;
            }
            let frame_len = u32::from_be_bytes(len_buf) as usize;
            if frame_len > MAX_FRAME_LENGTH || frame_len == 0 {
                log::warn!("Invalid frame length {}, stopping daemon→stdout", frame_len);
                break;
            }
            let mut frame_buf = vec![0u8; frame_len];
            if let Err(e) = sock_reader.read_exact(&mut frame_buf).await {
                log::warn!(
                    "Daemon socket read error (body): {}, stopping daemon→stdout",
                    e
                );
                break;
            }

            if let Some(msg) = MuxMessage::from_frame_body(&frame_buf) {
                let t = transport_for_stdout.load(Ordering::Relaxed);
                let encoded = if t == Transport::Osc as u8 {
                    msg.to_osc()
                } else {
                    msg.to_apc()
                };
                use std::io::Write;
                let stdout = std::io::stdout();
                let mut stdout = stdout.lock();
                if stdout.write_all(encoded.as_bytes()).is_err() {
                    log::info!("stdout write error, stopping daemon→stdout");
                    break;
                }
                let _ = stdout.flush();
            } else {
                log::warn!("Invalid frame body ({} bytes), skipping", frame_len);
            }
        }
    };

    // Run both directions concurrently; exit when either ends
    tokio::select! {
        _ = stdin_to_daemon => {
            log::info!("stdin→daemon ended, shutting down bridge");
        }
        _ = daemon_to_stdout => {
            log::info!("daemon→stdout ended, shutting down bridge");
        }
    }

    // Ensure all stdout data (including final Detached APC) is flushed
    // before exiting, so the GUI receives it.
    log::info!("Flushing stdout before exit");
    {
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }

    // Restore terminal settings
    if let Some(ref orig) = orig_termios {
        restore_stdin(orig);
    }

    // Brief delay so the GUI's PTY reader can consume the flushed data
    // before the PTY slave fd is closed by process exit.
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Exit immediately so the host shell returns to foreground promptly.
    // Without this, tokio runtime shutdown waits for the blocked stdin
    // reader task, delaying shell prompt redraw by seconds.
    log::info!("Bridge exiting via process::exit");
    std::process::exit(0);
}

/// Actions produced by the stdin mux parser.
#[derive(Debug)]
#[allow(dead_code)]
pub(super) enum StdinAction {
    /// A decoded mux message to forward to the daemon, with transport info.
    MuxMessage(MuxMessage, Transport),
    /// Passthrough data (non-mux bytes).
    Passthrough(Vec<u8>),
}

/// Maximum APC payload size (matches MAX_FRAME_LENGTH after Base64 expansion).
/// Base64 expands by ~4/3, so 22MB covers the 16MB frame limit.
const MAX_APC_PAYLOAD: usize = 22 * 1024 * 1024;

/// State machine that separates APC/OSC mux sequences from passthrough data on stdin.
///
/// Handles partial reads across buffer boundaries.
/// Recognizes both APC (ESC _) and OSC 9999 (ESC ]) mux sequences.
pub(super) struct StdinApcParser {
    state: ParserState,
    apc_buf: Vec<u8>,
    passthrough_buf: Vec<u8>,
    /// Accumulator for OSC numeric parameter.
    osc_param_accum: u16,
    /// Number of digits accumulated for the OSC parameter (to reconstruct on rejection).
    osc_param_digits: Vec<u8>,
    /// Whether the current sequence entered via the OSC path (ESC ] 9999 ;).
    is_osc: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParserState {
    /// Normal text / passthrough mode.
    Ground,
    /// Seen ESC, waiting for _ (APC start), ] (OSC start), or \ (APC end inside accumulation).
    EscSeen,
    /// Inside OSC parameter accumulation (digits after ESC ]).
    InOscParam,
    /// Inside APC/OSC body accumulation.
    InApc,
    /// Inside APC/OSC body, seen ESC (could be ST = ESC \).
    InApcEsc,
}

impl StdinApcParser {
    pub fn new() -> Self {
        Self {
            state: ParserState::Ground,
            apc_buf: Vec::new(),
            passthrough_buf: Vec::new(),
            osc_param_accum: 0,
            osc_param_digits: Vec::new(),
            is_osc: false,
        }
    }

    /// Complete a mux sequence: decode the accumulated APC buffer and produce an action.
    fn complete_mux_sequence(
        apc_buf: &mut Vec<u8>,
        is_osc: bool,
        actions: &mut Vec<StdinAction>,
    ) {
        let payload = String::from_utf8_lossy(apc_buf).to_string();
        apc_buf.clear();

        let transport = if is_osc {
            Transport::Osc
        } else {
            Transport::Apc
        };

        if payload.starts_with(APC_PREFIX) {
            match MuxMessage::from_apc(&payload) {
                Ok(msg) => actions.push(StdinAction::MuxMessage(msg, transport)),
                Err(e) => {
                    eprintln!("Bridge: mux decode error: {}", e);
                }
            }
        } else if !is_osc {
            // Non-mux APC: forward as passthrough (ESC_ + body + ESC\)
            let mut pdata = Vec::with_capacity(2 + payload.len() + 2);
            pdata.extend_from_slice(b"\x1b_");
            pdata.extend_from_slice(payload.as_bytes());
            pdata.extend_from_slice(b"\x1b\\");
            actions.push(StdinAction::Passthrough(pdata));
        }
        // Non-mux OSC with param 9999 shouldn't happen (we only enter InApc for 9999),
        // but if the body doesn't have APC_PREFIX, just discard it.
    }

    /// Feed bytes into the parser and return resulting actions.
    pub fn feed(&mut self, data: &[u8]) -> Vec<StdinAction> {
        let mut actions = Vec::new();

        for &byte in data {
            match self.state {
                ParserState::Ground => {
                    if byte == 0x1B {
                        self.state = ParserState::EscSeen;
                    } else {
                        self.passthrough_buf.push(byte);
                    }
                }
                ParserState::EscSeen => {
                    if byte == b'_' {
                        // APC start: flush passthrough first
                        if !self.passthrough_buf.is_empty() {
                            actions.push(StdinAction::Passthrough(std::mem::take(
                                &mut self.passthrough_buf,
                            )));
                        }
                        self.state = ParserState::InApc;
                        self.apc_buf.clear();
                        self.is_osc = false;
                    } else if byte == b']' {
                        // OSC start: flush passthrough and begin parameter accumulation
                        if !self.passthrough_buf.is_empty() {
                            actions.push(StdinAction::Passthrough(std::mem::take(
                                &mut self.passthrough_buf,
                            )));
                        }
                        self.osc_param_accum = 0;
                        self.osc_param_digits.clear();
                        self.state = ParserState::InOscParam;
                    } else {
                        // Not APC/OSC start: treat ESC + byte as passthrough
                        self.passthrough_buf.push(0x1B);
                        self.passthrough_buf.push(byte);
                        self.state = ParserState::Ground;
                    }
                }
                ParserState::InOscParam => {
                    if byte.is_ascii_digit() {
                        self.osc_param_digits.push(byte);
                        self.osc_param_accum =
                            self.osc_param_accum.saturating_mul(10) + (byte - b'0') as u16;
                    } else if byte == b';' {
                        if self.osc_param_accum == MUX_OSC_PARAM {
                            // Matched OSC 9999; -> reuse APC accumulation for body
                            self.state = ParserState::InApc;
                            self.apc_buf.clear();
                            self.is_osc = true;
                        } else {
                            // Wrong OSC param: push ESC ] <digits> ; as passthrough
                            self.passthrough_buf.push(0x1B);
                            self.passthrough_buf.push(b']');
                            self.passthrough_buf
                                .extend_from_slice(&self.osc_param_digits);
                            self.passthrough_buf.push(b';');
                            self.osc_param_digits.clear();
                            self.state = ParserState::Ground;
                        }
                    } else {
                        // Non-digit, non-semicolon: not a valid OSC param, passthrough
                        self.passthrough_buf.push(0x1B);
                        self.passthrough_buf.push(b']');
                        self.passthrough_buf
                            .extend_from_slice(&self.osc_param_digits);
                        self.passthrough_buf.push(byte);
                        self.osc_param_digits.clear();
                        self.state = ParserState::Ground;
                    }
                }
                ParserState::InApc => {
                    if byte == 0x1B {
                        self.state = ParserState::InApcEsc;
                    } else if self.is_osc && byte == 0x07 {
                        // BEL as alternative OSC terminator
                        Self::complete_mux_sequence(
                            &mut self.apc_buf,
                            self.is_osc,
                            &mut actions,
                        );
                        self.state = ParserState::Ground;
                    } else if self.apc_buf.len() < MAX_APC_PAYLOAD {
                        self.apc_buf.push(byte);
                    } else {
                        // APC/OSC payload too large: discard and reset to ground
                        eprintln!(
                            "Bridge: APC/OSC payload exceeds {} bytes, discarding",
                            MAX_APC_PAYLOAD
                        );
                        self.apc_buf.clear();
                        self.state = ParserState::Ground;
                    }
                }
                ParserState::InApcEsc => {
                    if byte == b'\\' {
                        // ST (ESC \) found: decode the payload
                        Self::complete_mux_sequence(
                            &mut self.apc_buf,
                            self.is_osc,
                            &mut actions,
                        );
                        self.state = ParserState::Ground;
                    } else {
                        // ESC inside APC but not followed by \: keep accumulating
                        self.apc_buf.push(0x1B);
                        self.apc_buf.push(byte);
                        self.state = ParserState::InApc;
                    }
                }
            }
        }

        // Flush remaining passthrough
        if !self.passthrough_buf.is_empty() && self.state == ParserState::Ground {
            actions.push(StdinAction::Passthrough(std::mem::take(
                &mut self.passthrough_buf,
            )));
        }

        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stdin_parser_passthrough_only() {
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(b"hello world");
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            StdinAction::Passthrough(data) => assert_eq!(data, b"hello world"),
            _ => panic!("Expected Passthrough"),
        }
    }

    #[test]
    fn test_stdin_parser_apc_mux_message() {
        let msg = MuxMessage::pty_input(1, vec![0x41, 0x42]);
        let apc = msg.to_apc();
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(apc.as_bytes());
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            StdinAction::MuxMessage(decoded, transport) => {
                assert_eq!(decoded.msg_type, MessageType::PtyInput);
                assert_eq!(decoded.pane_id, 1);
                assert_eq!(decoded.payload, vec![0x41, 0x42]);
                assert_eq!(*transport, Transport::Apc);
            }
            _ => panic!("Expected MuxMessage"),
        }
    }

    #[test]
    fn test_stdin_parser_passthrough_then_apc() {
        let msg = MuxMessage::pty_input(1, vec![0x41]);
        let apc = msg.to_apc();
        let mut input = b"before".to_vec();
        input.extend_from_slice(apc.as_bytes());
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(&input);
        assert_eq!(actions.len(), 2);
        match &actions[0] {
            StdinAction::Passthrough(data) => assert_eq!(data, b"before"),
            _ => panic!("Expected Passthrough"),
        }
        match &actions[1] {
            StdinAction::MuxMessage(decoded, _) => {
                assert_eq!(decoded.msg_type, MessageType::PtyInput);
            }
            _ => panic!("Expected MuxMessage"),
        }
    }

    #[test]
    fn test_stdin_parser_split_across_boundaries() {
        let msg = MuxMessage::pty_input(5, vec![0xFF]);
        let apc = msg.to_apc();
        let bytes = apc.as_bytes();
        let mid = bytes.len() / 2;

        let mut parser = StdinApcParser::new();

        // Feed first half
        let actions1 = parser.feed(&bytes[..mid]);
        // Should not produce MuxMessage yet (APC not complete)
        for a in &actions1 {
            assert!(
                !matches!(a, StdinAction::MuxMessage(_, _)),
                "Should not decode incomplete APC"
            );
        }

        // Feed second half
        let actions2 = parser.feed(&bytes[mid..]);
        let has_msg = actions2
            .iter()
            .any(|a| matches!(a, StdinAction::MuxMessage(_, _)));
        assert!(has_msg, "Should decode APC after second half");
    }

    #[test]
    fn test_stdin_parser_non_mux_apc() {
        // A non-mux APC sequence (e.g., Kitty graphics) should be passed through
        let input = b"\x1b_Gf=32;data\x1b\\";
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(input);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            StdinAction::Passthrough(data) => {
                assert_eq!(data, input);
            }
            _ => panic!("Expected Passthrough for non-mux APC"),
        }
    }

    #[test]
    fn test_stdin_parser_esc_not_apc() {
        // ESC followed by something other than _ or ] should be passthrough
        let input = b"\x1b[31m";
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(input);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            StdinAction::Passthrough(data) => {
                assert_eq!(data, &b"\x1b[31m".to_vec());
            }
            _ => panic!("Expected Passthrough"),
        }
    }

    #[test]
    fn test_stdin_parser_multiple_apc_in_one_feed() {
        let msg1 = MuxMessage::pty_input(1, vec![0x01]);
        let msg2 = MuxMessage::pty_input(2, vec![0x02]);
        let mut input = msg1.to_apc().into_bytes();
        input.extend_from_slice(msg2.to_apc().as_bytes());

        let mut parser = StdinApcParser::new();
        let actions = parser.feed(&input);
        let msgs: Vec<_> = actions
            .iter()
            .filter_map(|a| match a {
                StdinAction::MuxMessage(m, _) => Some(m),
                _ => None,
            })
            .collect();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].pane_id, 1);
        assert_eq!(msgs[1].pane_id, 2);
    }

    #[test]
    fn test_stdin_parser_empty_input() {
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(b"");
        assert!(actions.is_empty());
    }

    #[test]
    fn test_stdin_parser_esc_inside_apc_not_st() {
        // ESC inside APC body but not followed by \ should continue accumulation
        let mut input = Vec::new();
        input.extend_from_slice(b"\x1b_emterm-mux;");
        input.push(0x1B); // ESC
        input.push(b'X'); // Not \, so should be added to APC buf
        input.extend_from_slice(b"\x1b\\");
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(&input);
        // The APC body is "emterm-mux;\x1bX" which is invalid base64
        assert_eq!(actions.len(), 0); // decode error is printed, no action produced
    }

    // ---- OSC mux message tests ----

    #[test]
    fn test_stdin_parser_osc_mux_message() {
        let msg = MuxMessage::pty_input(1, vec![0x41, 0x42]);
        let osc = msg.to_osc();
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(osc.as_bytes());
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            StdinAction::MuxMessage(decoded, transport) => {
                assert_eq!(decoded.msg_type, MessageType::PtyInput);
                assert_eq!(decoded.pane_id, 1);
                assert_eq!(decoded.payload, vec![0x41, 0x42]);
                assert_eq!(*transport, Transport::Osc);
            }
            _ => panic!("Expected MuxMessage"),
        }
    }

    #[test]
    fn test_stdin_parser_osc_with_bel_terminator() {
        // OSC 9999 terminated with BEL (0x07) instead of ESC \
        let msg = MuxMessage::pty_input(3, vec![0xAA]);
        let body = msg.to_frame_body();
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode(&body);
        let osc = format!("\x1b]9999;emterm-mux;{}\x07", encoded);
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(osc.as_bytes());
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            StdinAction::MuxMessage(decoded, transport) => {
                assert_eq!(decoded.msg_type, MessageType::PtyInput);
                assert_eq!(decoded.pane_id, 3);
                assert_eq!(decoded.payload, vec![0xAA]);
                assert_eq!(*transport, Transport::Osc);
            }
            _ => panic!("Expected MuxMessage"),
        }
    }

    #[test]
    fn test_stdin_parser_osc_wrong_param_passthrough() {
        // OSC with param != 9999 should be passed through
        let input = b"\x1b]1234;some-data\x1b\\";
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(input);
        // The ESC ] 1 2 3 4 ; is pushed as passthrough, then "some-data" and ESC \ are ground
        assert!(!actions.is_empty());
        // All actions should be Passthrough
        for a in &actions {
            assert!(
                matches!(a, StdinAction::Passthrough(_)),
                "Expected Passthrough for non-9999 OSC, got {:?}",
                a
            );
        }
    }

    #[test]
    fn test_stdin_parser_mixed_apc_and_osc() {
        let msg1 = MuxMessage::pty_input(1, vec![0x01]);
        let msg2 = MuxMessage::pty_input(2, vec![0x02]);
        let msg3 = MuxMessage::pty_input(3, vec![0x03]);
        let mut input = msg1.to_apc().into_bytes();
        input.extend_from_slice(msg2.to_osc().as_bytes());
        input.extend_from_slice(msg3.to_apc().as_bytes());

        let mut parser = StdinApcParser::new();
        let actions = parser.feed(&input);
        let msgs: Vec<_> = actions
            .iter()
            .filter_map(|a| match a {
                StdinAction::MuxMessage(m, t) => Some((m, *t)),
                _ => None,
            })
            .collect();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].0.pane_id, 1);
        assert_eq!(msgs[0].1, Transport::Apc);
        assert_eq!(msgs[1].0.pane_id, 2);
        assert_eq!(msgs[1].1, Transport::Osc);
        assert_eq!(msgs[2].0.pane_id, 3);
        assert_eq!(msgs[2].1, Transport::Apc);
    }

    #[test]
    fn test_stdin_parser_osc_split_across_boundaries() {
        let msg = MuxMessage::pty_input(5, vec![0xFF]);
        let osc = msg.to_osc();
        let bytes = osc.as_bytes();
        let mid = bytes.len() / 2;

        let mut parser = StdinApcParser::new();

        // Feed first half
        let actions1 = parser.feed(&bytes[..mid]);
        for a in &actions1 {
            assert!(
                !matches!(a, StdinAction::MuxMessage(_, _)),
                "Should not decode incomplete OSC"
            );
        }

        // Feed second half
        let actions2 = parser.feed(&bytes[mid..]);
        let has_msg = actions2
            .iter()
            .any(|a| matches!(a, StdinAction::MuxMessage(_, _)));
        assert!(has_msg, "Should decode OSC after second half");
    }

    #[test]
    fn test_stdin_parser_osc_non_digit_in_param() {
        // ESC ] followed by non-digit should be passthrough
        let input = b"\x1b]abc;data\x1b\\";
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(input);
        // 'a' is non-digit, so ESC ] a is pushed as passthrough, rest continues as ground
        assert!(!actions.is_empty());
        for a in &actions {
            assert!(matches!(a, StdinAction::Passthrough(_)));
        }
    }
}
