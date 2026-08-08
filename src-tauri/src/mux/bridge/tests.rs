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

// ---- Plaintext mux message tests ----

#[test]
fn test_stdin_parser_plaintext_mux_message() {
    let msg = MuxMessage::pty_input(1, vec![0x41, 0x42]);
    let body = msg.to_frame_body();
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&body);
    let input = format!("EMUX;{}\n", encoded);
    let mut parser = StdinApcParser::new();
    let actions = parser.feed(input.as_bytes());
    assert_eq!(actions.len(), 1);
    match &actions[0] {
        StdinAction::MuxMessage(decoded, transport) => {
            assert_eq!(decoded.msg_type, MessageType::PtyInput);
            assert_eq!(decoded.pane_id, 1);
            assert_eq!(decoded.payload, vec![0x41, 0x42]);
            assert_eq!(*transport, Transport::Plaintext);
        }
        _ => panic!("Expected MuxMessage"),
    }
}

#[test]
fn test_stdin_parser_plaintext_with_passthrough() {
    let msg = MuxMessage::pty_input(1, vec![0x41]);
    let body = msg.to_frame_body();
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&body);
    let input = format!("beforeEMUX;{}\nafter", encoded);
    let mut parser = StdinApcParser::new();
    let actions = parser.feed(input.as_bytes());
    // "before" as passthrough, then MuxMessage, then "after" as passthrough
    let msgs: Vec<_> = actions
        .iter()
        .filter_map(|a| match a {
            StdinAction::MuxMessage(m, _) => Some(m),
            _ => None,
        })
        .collect();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].pane_id, 1);
}

#[test]
fn test_stdin_parser_plaintext_split_across_boundaries() {
    let msg = MuxMessage::pty_input(5, vec![0xFF]);
    let body = msg.to_frame_body();
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&body);
    let input = format!("EMUX;{}\n", encoded);
    let bytes = input.as_bytes();
    let mid = bytes.len() / 2;

    let mut parser = StdinApcParser::new();

    let actions1 = parser.feed(&bytes[..mid]);
    for a in &actions1 {
        assert!(
            !matches!(a, StdinAction::MuxMessage(_, _)),
            "Should not decode incomplete plaintext"
        );
    }

    let actions2 = parser.feed(&bytes[mid..]);
    let has_msg = actions2
        .iter()
        .any(|a| matches!(a, StdinAction::MuxMessage(_, _)));
    assert!(has_msg, "Should decode plaintext after second half");
}

#[test]
fn test_stdin_parser_plaintext_prefix_mismatch() {
    // "EMUY;" is not a valid prefix, should be passthrough
    let input = b"EMUY;data\n";
    let mut parser = StdinApcParser::new();
    let actions = parser.feed(input);
    assert!(!actions.is_empty());
    for a in &actions {
        assert!(matches!(a, StdinAction::Passthrough(_)));
    }
}

#[test]
fn test_stdin_parser_plaintext_cr_terminator() {
    // The Windows host writes the envelope with a CR terminator because
    // portable-pty 0.8 opens ConPTY with `PSEUDOCONSOLE_WIN32_INPUT_MODE`,
    // and raw LF is not delivered as a real key event on that channel —
    // only CR (VK_RETURN) survives. This pins that CR alone closes the
    // message; if it doesn't, the bridge stalls in `InPlaintext` forever
    // with the prefix matched but no terminator ever arriving (the exact
    // regression observed against a hyper-v mux daemon).
    let msg = MuxMessage::pty_input(1, vec![0x41, 0x42]);
    let body = msg.to_frame_body();
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&body);
    let input = format!("EMUX;{}\r", encoded);
    let mut parser = StdinApcParser::new();
    let actions = parser.feed(input.as_bytes());
    assert_eq!(actions.len(), 1, "CR alone must produce exactly one action");
    match &actions[0] {
        StdinAction::MuxMessage(decoded, transport) => {
            assert_eq!(decoded.msg_type, MessageType::PtyInput);
            assert_eq!(decoded.pane_id, 1);
            assert_eq!(decoded.payload, vec![0x41, 0x42]);
            assert_eq!(*transport, Transport::Plaintext);
        }
        other => panic!("Expected MuxMessage, got {other:?}"),
    }
}

#[test]
fn test_stdin_parser_plaintext_crlf_terminator() {
    // Intermediate layers (ConPTY, ssh, the host shell) can append LF
    // after the encoder's CR terminator, or translate the standalone CR
    // to a CRLF pair on the way to the bridge. The parser must complete
    // on the first half and silently consume the partner half so the
    // trailing LF doesn't leak out as passthrough.
    let msg = MuxMessage::pty_input(3, vec![0xAB, 0xCD, 0xEF]);
    let body = msg.to_frame_body();
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&body);
    let input = format!("EMUX;{}\r\n", encoded);
    let mut parser = StdinApcParser::new();
    let actions = parser.feed(input.as_bytes());
    assert_eq!(actions.len(), 1, "CRLF must produce exactly one action");
    match &actions[0] {
        StdinAction::MuxMessage(decoded, transport) => {
            assert_eq!(decoded.msg_type, MessageType::PtyInput);
            assert_eq!(decoded.pane_id, 3);
            assert_eq!(decoded.payload, vec![0xAB, 0xCD, 0xEF]);
            assert_eq!(*transport, Transport::Plaintext);
        }
        other => panic!("Expected MuxMessage, got {other:?}"),
    }
}

#[test]
fn test_stdin_parser_plaintext_lfcr_terminator() {
    // The reverse order LF→CR must also collapse to a single message,
    // mirroring the CRLF tolerance above. base64 STANDARD has neither
    // CR nor LF in its alphabet, so a trailing CR after the LF is
    // unambiguously the partner half of the terminator.
    let msg = MuxMessage::pty_input(2, vec![0x10]);
    let body = msg.to_frame_body();
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&body);
    let input = format!("EMUX;{}\n\r", encoded);
    let mut parser = StdinApcParser::new();
    let actions = parser.feed(input.as_bytes());
    assert_eq!(actions.len(), 1, "LFCR must produce exactly one action");
    match &actions[0] {
        StdinAction::MuxMessage(decoded, _) => {
            assert_eq!(decoded.pane_id, 2);
            assert_eq!(decoded.payload, vec![0x10]);
        }
        other => panic!("Expected MuxMessage, got {other:?}"),
    }
}

#[test]
fn test_stdin_parser_plaintext_terminator_split_across_feeds() {
    // The terminator pair (CRLF) is allowed to land on opposite sides of
    // a feed boundary: the CR completes the message at the end of the
    // first feed, and the LF arriving as the first byte of the next feed
    // must be silently swallowed via `swallow_partner_eol` rather than
    // leaking out as a stray passthrough byte.
    let msg = MuxMessage::pty_input(9, vec![0xFF]);
    let body = msg.to_frame_body();
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&body);
    let first = format!("EMUX;{}\r", encoded);

    let mut parser = StdinApcParser::new();
    let actions1 = parser.feed(first.as_bytes());
    let msgs1: Vec<_> = actions1
        .iter()
        .filter(|a| matches!(a, StdinAction::MuxMessage(_, _)))
        .collect();
    assert_eq!(msgs1.len(), 1, "CR alone closes the message in feed 1");

    // Feed 2 starts with the partner LF: must be swallowed silently.
    let actions2 = parser.feed(b"\nrest");
    let passthroughs: Vec<&Vec<u8>> = actions2
        .iter()
        .filter_map(|a| match a {
            StdinAction::Passthrough(p) => Some(p),
            _ => None,
        })
        .collect();
    assert_eq!(
        passthroughs.len(),
        1,
        "feed 2 produces exactly one passthrough (just 'rest')"
    );
    assert_eq!(passthroughs[0], b"rest");
}

#[test]
fn test_stdin_parser_mixed_plaintext_and_apc() {
    let msg1 = MuxMessage::pty_input(1, vec![0x01]);
    let msg2 = MuxMessage::pty_input(2, vec![0x02]);
    let body2 = msg2.to_frame_body();
    use base64::Engine as _;
    let encoded2 = base64::engine::general_purpose::STANDARD.encode(&body2);
    let mut input = msg1.to_apc().into_bytes();
    input.extend_from_slice(format!("EMUX;{}\n", encoded2).as_bytes());

    let mut parser = StdinApcParser::new();
    let actions = parser.feed(&input);
    let msgs: Vec<_> = actions
        .iter()
        .filter_map(|a| match a {
            StdinAction::MuxMessage(m, t) => Some((m, *t)),
            _ => None,
        })
        .collect();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].0.pane_id, 1);
    assert_eq!(msgs[0].1, Transport::Apc);
    assert_eq!(msgs[1].0.pane_id, 2);
    assert_eq!(msgs[1].1, Transport::Plaintext);
}

// ---- task0006: client reconnect after an upgrade announcement ----

/// AC-1: the announcement decodes to `Announced`, never `Forward` — so
/// it can never reach the terminal output path.
#[test]
fn decide_daemon_frame_effect_recognizes_upgrading_without_forwarding() {
    let announcement = MuxMessage::control(MessageType::Upgrading, 0, &());
    let body = announcement.to_frame_body();

    match decide_daemon_frame_effect(&body) {
        DaemonFrameEffect::Announced => {}
        other => panic!("expected Announced, got {other:?}"),
    }
}

/// Sanity check alongside AC-1: an ordinary message still decodes to
/// `Forward` with its fields intact, so recognizing the announcement
/// didn't regress the existing forwarding path.
#[test]
fn decide_daemon_frame_effect_forwards_ordinary_messages() {
    let msg = MuxMessage::pty_output(3, vec![0x41, 0x42]);
    let body = msg.to_frame_body();

    match decide_daemon_frame_effect(&body) {
        DaemonFrameEffect::Forward(forwarded) => {
            assert_eq!(forwarded.msg_type, MessageType::PtyOutput);
            assert_eq!(forwarded.pane_id, 3);
            assert_eq!(forwarded.payload, vec![0x41, 0x42]);
        }
        other => panic!("expected Forward, got {other:?}"),
    }
}

#[test]
fn decide_daemon_frame_effect_ignores_undecodable_frames() {
    // Too short to contain even the 5-byte header.
    let body = vec![0x01, 0x02];
    match decide_daemon_frame_effect(&body) {
        DaemonFrameEffect::Ignored => {}
        other => panic!("expected Ignored, got {other:?}"),
    }
}

/// Half of AC-4: only an `Attach` request is captured for a later
/// resend; every other message type is not.
#[test]
fn capture_if_attach_captures_attach_and_only_attach() {
    let attach = MuxMessage::control(MessageType::Attach, 0, &AttachMsg { session_id: 5 });
    let attach_body = attach.to_frame_body();
    assert_eq!(
        capture_if_attach(&attach, &attach_body),
        Some(attach_body.clone())
    );

    let other = MuxMessage::pty_input(1, vec![0x41]);
    let other_body = other.to_frame_body();
    assert_eq!(capture_if_attach(&other, &other_body), None);
}

/// AC-3 / AC-7: the connection-ended decision is exactly the observed
/// announcement flag — never sticky across calls (`forward_loop`
/// creates a fresh flag every invocation, so a reconnected connection
/// that never sees a NEW announcement concludes `Normal`).
#[test]
fn conclude_connection_maps_the_announced_flag() {
    assert_eq!(conclude_connection(false), ConnectionEnded::Normal);
    assert_eq!(conclude_connection(true), ConnectionEnded::Announced);
}

// ---- reconnect: stand-in daemon in a plain thread, mirroring the
// pattern in daemon.rs's tests (spawn_fake_legacy_daemon) ----

#[cfg(unix)]
fn read_frame_blocking<S: std::io::Read>(stream: &mut S) -> MuxMessage {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).expect("read frame length");
    let frame_len = u32::from_be_bytes(len_buf) as usize;
    let mut frame_buf = vec![0u8; frame_len];
    stream.read_exact(&mut frame_buf).expect("read frame body");
    MuxMessage::from_frame_body(&frame_buf).expect("valid frame")
}

#[cfg(unix)]
fn write_frame_blocking<S: std::io::Write>(stream: &mut S, msg: &MuxMessage) {
    let body = msg.to_frame_body();
    stream
        .write_all(&(body.len() as u32).to_be_bytes())
        .expect("write frame length");
    stream.write_all(&body).expect("write frame body");
    stream.flush().expect("flush");
}

#[cfg(unix)]
fn accept_and_handshake_blocking(
    listener: &std::os::unix::net::UnixListener,
) -> std::os::unix::net::UnixStream {
    let (mut stream, _) = listener.accept().expect("accept reconnect attempt");
    let hello_frame = read_frame_blocking(&mut stream);
    assert_eq!(hello_frame.msg_type, MessageType::Hello);
    let welcome = WelcomeMsg::Accepted {
        server_version: PROTOCOL_VERSION,
        sessions: Vec::new(),
    };
    write_frame_blocking(
        &mut stream,
        &MuxMessage::control(MessageType::Welcome, 0, &welcome),
    );
    stream
}

/// AC-2 / AC-4: `reconnect_and_reattach` completes a second handshake
/// against the stand-in daemon and then resends the previously
/// captured `Attach` frame verbatim.
#[cfg(unix)]
#[tokio::test]
async fn reconnect_and_reattach_resends_the_last_attach_after_reconnecting() {
    use std::os::unix::net::UnixListener;

    let dir = tempfile::tempdir().expect("tempdir");
    let sock_path = dir.path().join("reconnect.sock");

    let attach = MuxMessage::control(MessageType::Attach, 0, &AttachMsg { session_id: 7 });
    let attach_body = attach.to_frame_body();
    let last_attach: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(Some(attach_body.clone())));

    let bind_path = sock_path.clone();
    let daemon = std::thread::spawn(move || {
        let listener = UnixListener::bind(&bind_path).expect("bind stand-in daemon socket");
        let mut stream = accept_and_handshake_blocking(&listener);
        // The very next frame after the reconnect handshake must be
        // the resent Attach request (AC-4), not anything else.
        read_frame_blocking(&mut stream)
    });

    let result = reconnect_and_reattach(&sock_path, &last_attach).await;
    assert!(
        result.is_some(),
        "reconnect_and_reattach should succeed against a live stand-in daemon"
    );

    let resent = daemon.join().expect("stand-in daemon thread panicked");
    assert_eq!(resent.msg_type, MessageType::Attach);
    let decoded: AttachMsg = resent.decode_payload().expect("Attach payload");
    assert_eq!(decoded.session_id, 7);
}

/// AC-5: with nothing ever listening on `sock_path`, every attempt
/// fails and the bounded retry loop gives up rather than retrying
/// forever.
#[cfg(unix)]
#[tokio::test]
async fn reconnect_with_backoff_gives_up_after_the_window_is_exhausted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock_path = dir.path().join("nothing-here.sock");

    let started = std::time::Instant::now();
    let result = reconnect_with_backoff(&sock_path).await;
    let elapsed = started.elapsed();

    assert!(
        result.is_none(),
        "reconnect must give up, not retry forever"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "the retry window must be bounded, took {:?}",
        elapsed
    );
}

/// AC-6: the stand-in daemon only starts listening after the first
/// backoff delay would have elapsed, so the first attempt observably
/// fails (nothing bound yet) before a later attempt succeeds — proving
/// a real delay separates attempts rather than a tight spin.
#[cfg(unix)]
#[tokio::test]
async fn reconnect_with_backoff_waits_between_failed_attempts() {
    use std::os::unix::net::UnixListener;

    let dir = tempfile::tempdir().expect("tempdir");
    let sock_path = dir.path().join("delayed.sock");
    let bind_path = sock_path.clone();

    let daemon = std::thread::spawn(move || {
        std::thread::sleep(RECONNECT_INITIAL_BACKOFF + std::time::Duration::from_millis(20));
        let listener = UnixListener::bind(&bind_path).expect("bind delayed stand-in daemon");
        let _stream = accept_and_handshake_blocking(&listener);
    });

    let started = std::time::Instant::now();
    let result = reconnect_with_backoff(&sock_path).await;
    let elapsed = started.elapsed();

    daemon.join().expect("stand-in daemon thread panicked");

    assert!(
        result.is_some(),
        "reconnect should succeed once the daemon starts listening"
    );
    assert!(
        elapsed >= RECONNECT_INITIAL_BACKOFF,
        "attempts must be separated by a backoff delay, took only {:?}",
        elapsed
    );
}

// ---- task0010: the stdin handle (and its parser) persist across
// `forward_loop` calls instead of being recreated per reconnect ----

/// AC-1 / AC-2 / AC-3: `forward_loop` takes the stdin handle and its
/// parser as caller-owned `&mut` parameters instead of constructing its
/// own, so the SAME instances can be threaded through every connection
/// of a bridge run (exactly how `bridge_main_loop` uses them across a
/// reconnect). This test drives two `forward_loop` calls with the same
/// handle/parser and proves the direct consequence: a byte that
/// arrives on stdin only AFTER the first connection has already ended
/// is still delivered to the daemon once the second connection's call
/// starts reading (AC-2). Before this fix, each call constructed its
/// own `tokio::io::stdin()`; that handle would already have been
/// dropped by the time this byte arrived, discarding it silently while
/// leaving the abandoned blocking read (AC-3) occupying a pool thread
/// until the user's next keystroke. Because only one handle is ever
/// constructed for the whole run (AC-1), no such second handle exists
/// to strand a read against.
#[cfg(unix)]
#[tokio::test]
async fn forward_loop_persistent_stdin_handle_delivers_bytes_queued_across_reconnect() {
    use tokio::io::AsyncReadExt as _;
    use tokio::io::AsyncWriteExt as _;

    // One stdin handle + one parser for the whole simulated bridge
    // run — created ONCE, exactly as `bridge_main_loop` does, and
    // passed into BOTH `forward_loop` calls below.
    let (mut stdin, mut stdin_probe) = tokio::io::duplex(4096);
    let mut parser = StdinApcParser::new();
    let transport = Arc::new(AtomicU8::new(TRANSPORT_UNDETECTED));
    let last_attach: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));

    // Connection #1's "daemon" side is closed immediately, before
    // stdin has produced anything, so `daemon_to_stdout` ends the call
    // right away — mirroring the daemon dropping the connection while
    // a keystroke is still in flight on stdin.
    let (conn1, conn1_daemon_side) = tokio::io::duplex(64);
    drop(conn1_daemon_side);
    let (mut r1, mut w1) = tokio::io::split(conn1);

    let ended1 = forward_loop(
        &mut r1,
        &mut w1,
        &transport,
        &last_attach,
        &mut stdin,
        &mut parser,
    )
    .await;
    assert_eq!(ended1, ConnectionEnded::Normal);

    // The keystroke arrives only now, strictly after connection #1 has
    // already ended — the window in which a freshly-recreated handle
    // would already have been dropped.
    let msg = MuxMessage::pty_input(1, vec![0x41]);
    let apc = msg.to_apc();
    stdin_probe
        .write_all(apc.as_bytes())
        .await
        .expect("write pending keystroke");
    stdin_probe.flush().await.expect("flush pending keystroke");

    // Connection #2 stands in for the reconnect: a fresh "daemon"
    // duplex, but `stdin` and `parser` are the SAME instances again —
    // never recreated.
    let (conn2, mut conn2_daemon_side) = tokio::io::duplex(4096);
    let (mut r2, mut w2) = tokio::io::split(conn2);

    let daemon_task = tokio::spawn(async move {
        let mut len_buf = [0u8; 4];
        conn2_daemon_side
            .read_exact(&mut len_buf)
            .await
            .expect("read forwarded frame length");
        let frame_len = u32::from_be_bytes(len_buf) as usize;
        let mut frame_buf = vec![0u8; frame_len];
        conn2_daemon_side
            .read_exact(&mut frame_buf)
            .await
            .expect("read forwarded frame body");
        MuxMessage::from_frame_body(&frame_buf).expect("valid forwarded frame")
        // `conn2_daemon_side` drops here, ending the duplex so `r2`
        // observes EOF and the `forward_loop` call below returns.
    });

    let ended2 = forward_loop(
        &mut r2,
        &mut w2,
        &transport,
        &last_attach,
        &mut stdin,
        &mut parser,
    )
    .await;
    assert_eq!(ended2, ConnectionEnded::Normal);

    let forwarded = daemon_task.await.expect("daemon task panicked");
    assert_eq!(forwarded.msg_type, MessageType::PtyInput);
    assert_eq!(forwarded.pane_id, 1);
    assert_eq!(forwarded.payload, vec![0x41]);
}

// ---- task0002: bridge stdout writer decoupled from the runtime
// thread (Test Notes: testability seam) ----

/// Bound for waits expected to complete (Convention 5): matches the
/// project's existing named-timeout convention (`connection.rs`'s
/// `HANDSHAKE_TIMEOUT`).
const STDOUT_WRITER_TEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Short bound used only to assert a wait did NOT complete quickly
/// (i.e. it suspended rather than ran to completion) — long enough to
/// rule out scheduling jitter as a false negative, short enough to
/// keep the test fast.
const STDOUT_WRITER_TEST_SUSPEND_CHECK: std::time::Duration = std::time::Duration::from_millis(200);

/// Settle delay after bulk-admitting frames in the `forward_loop`
/// level test: gives the scheduler a bounded window to drive
/// `daemon_to_stdout` through reading and admitting everything already
/// sitting in the socket buffer before the test proceeds to check the
/// sibling direction. Not a correctness wait (nothing here is racing
/// against it for correctness) — only a determinism aid.
const STDOUT_WRITER_TEST_SETTLE: std::time::Duration = std::time::Duration::from_millis(200);

/// Release-all gate used to simulate a stalled real stdout: every
/// `wait()` call blocks until `release()` is called once, after which
/// every past and future `wait()` returns immediately.
struct Gate {
    released: Mutex<bool>,
    cv: std::sync::Condvar,
}

impl Gate {
    fn new() -> Self {
        Self {
            released: Mutex::new(false),
            cv: std::sync::Condvar::new(),
        }
    }

    fn wait(&self) {
        let mut released = self.released.lock().expect("gate mutex poisoned");
        while !*released {
            released = self.cv.wait(released).expect("gate mutex poisoned");
        }
    }

    fn release(&self) {
        *self.released.lock().expect("gate mutex poisoned") = true;
        self.cv.notify_all();
    }
}

/// Test double for `StdoutSink` (Test Notes: testability seam). Each
/// `write_all` call signals `started` (so a test can confirm the pump
/// picked up a frame even while parked), then optionally blocks on
/// `gate`, then either fails (if `fail_at_call` matches this call's
/// index) or records the bytes into `written` in call order.
struct TestSink {
    started: tokio::sync::mpsc::Sender<Vec<u8>>,
    gate: Option<Arc<Gate>>,
    written: Arc<Mutex<Vec<Vec<u8>>>>,
    fail_at_call: Option<usize>,
    call_count: usize,
}

impl StdoutSink for TestSink {
    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        let idx = self.call_count;
        self.call_count += 1;
        // Signal only on the FIRST call: `started` has a small fixed
        // capacity and most tests drain it only once (to confirm the
        // writer picked up its first frame), so signalling on every
        // call would eventually block this thread forever once the
        // channel fills — on the very sink-blocking behaviour this
        // test double exists to simulate, but as an artifact of the
        // test double itself rather than of the code under test.
        // `blocking_send` (not `.send().await`) because this runs on
        // the pump's blocking thread (Test Notes / invariant 1).
        if idx == 0 {
            let _ = self.started.blocking_send(buf.to_vec());
        }
        if let Some(gate) = &self.gate {
            gate.wait();
        }
        if self.fail_at_call == Some(idx) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "simulated sink failure",
            ));
        }
        self.written
            .lock()
            .expect("written mutex poisoned")
            .push(buf.to_vec());
        Ok(())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Sends `msg` on `tx`, asserting the admission completes within the
/// bound rather than suspending — the AC-1/AC-4 "admissions up to the
/// bound complete without waiting on the sink" assertion, factored out
/// since several tests repeat it.
async fn send_bounded(tx: &tokio::sync::mpsc::Sender<MuxMessage>, msg: MuxMessage) {
    tokio::time::timeout(STDOUT_WRITER_TEST_TIMEOUT, tx.send(msg))
        .await
        .expect("admission within the bound must not suspend")
        .expect("channel must still be open");
}

/// AC-1 / AC-4: the writer's admission channel has a named, finite
/// bound — admissions up to that bound complete without waiting on
/// the (blocked) sink; the next admission suspends rather than
/// blocking a thread, and completes once the sink is released.
#[tokio::test]
async fn stdout_writer_admits_up_to_capacity_then_suspends_while_sink_blocked() {
    let gate = Arc::new(Gate::new());
    let (started_tx, mut started_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(8);
    let written: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(Vec::new()));
    let transport = Arc::new(AtomicU8::new(Transport::Apc as u8));

    let gate_for_sink = Arc::clone(&gate);
    let written_for_sink = Arc::clone(&written);
    let (tx, handle) = spawn_stdout_writer(Arc::clone(&transport), move || TestSink {
        started: started_tx,
        gate: Some(gate_for_sink),
        written: written_for_sink,
        fail_at_call: None,
        call_count: 0,
    });

    // Picked up by the writer immediately, which then blocks trying
    // to write it (the gate is held closed).
    send_bounded(&tx, MuxMessage::pty_output(1, vec![0])).await;
    tokio::time::timeout(STDOUT_WRITER_TEST_TIMEOUT, started_rx.recv())
        .await
        .expect("writer must start within the timeout")
        .expect("started channel open");

    // STDOUT_WRITER_CAPACITY more admissions must all complete without
    // waiting on the sink.
    for i in 0..STDOUT_WRITER_CAPACITY {
        send_bounded(&tx, MuxMessage::pty_output(1, vec![i as u8])).await;
    }

    // The next admission must suspend rather than complete immediately.
    // Driven from a spawned task (own clone of `tx`, dropped when the
    // task completes) so the original `tx` stays free to drop below
    // without any lingering borrow from this send.
    let tx_overflow = tx.clone();
    let overflow_task = tokio::spawn(async move {
        tx_overflow
            .send(MuxMessage::pty_output(1, vec![0xFF]))
            .await
    });
    tokio::time::sleep(STDOUT_WRITER_TEST_SUSPEND_CHECK).await;
    assert!(
        !overflow_task.is_finished(),
        "admission beyond the bound must suspend rather than complete immediately"
    );

    // Releasing the sink drains everything, including the overflow send.
    gate.release();
    tokio::time::timeout(STDOUT_WRITER_TEST_TIMEOUT, overflow_task)
        .await
        .expect("overflow task must finish once the sink drains")
        .expect("overflow task must not panic")
        .expect("overflow admission completes once the sink drains");

    drop(tx);
    tokio::time::timeout(STDOUT_WRITER_TEST_TIMEOUT, handle)
        .await
        .expect("writer quiesces after the channel closes")
        .expect("writer task must not panic");

    let got = written.lock().expect("written mutex poisoned");
    assert_eq!(
        got.len(),
        STDOUT_WRITER_CAPACITY + 2,
        "every admitted frame reaches the sink"
    );
}

/// AC-3: after a blocked sink is released, the sink has received
/// exactly the admitted frames in admission order (no loss, no
/// reorder) — a single admission channel feeding a single writer.
#[tokio::test]
async fn stdout_writer_delivers_frames_in_admission_order_after_release() {
    let gate = Arc::new(Gate::new());
    let (started_tx, mut started_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(8);
    let written: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(Vec::new()));
    let transport = Arc::new(AtomicU8::new(Transport::Apc as u8));

    let gate_for_sink = Arc::clone(&gate);
    let written_for_sink = Arc::clone(&written);
    let (tx, handle) = spawn_stdout_writer(Arc::clone(&transport), move || TestSink {
        started: started_tx,
        gate: Some(gate_for_sink),
        written: written_for_sink,
        fail_at_call: None,
        call_count: 0,
    });

    let panes: Vec<u32> = vec![1, 2, 3, 4, 5];
    let expected: Vec<Vec<u8>> = panes
        .iter()
        .map(|&pane| {
            MuxMessage::pty_output(pane, vec![pane as u8])
                .to_apc()
                .into_bytes()
        })
        .collect();
    for &pane in &panes {
        send_bounded(&tx, MuxMessage::pty_output(pane, vec![pane as u8])).await;
    }

    tokio::time::timeout(STDOUT_WRITER_TEST_TIMEOUT, started_rx.recv())
        .await
        .expect("writer must start within the timeout")
        .expect("started channel open");

    gate.release();
    drop(tx);
    tokio::time::timeout(STDOUT_WRITER_TEST_TIMEOUT, handle)
        .await
        .expect("writer quiesces")
        .expect("writer task must not panic");

    let got = written.lock().expect("written mutex poisoned");
    assert_eq!(
        *got, expected,
        "frames must reach the sink in admission order"
    );
}

/// Invariant 4 (transport parity, Test Notes): an undetected transport
/// resolves to BOTH encodings, OSC first, at write time — the same
/// both-encodings behaviour the inline write performed before this
/// task's rework.
#[tokio::test]
async fn stdout_writer_sends_both_encodings_when_transport_is_undetected() {
    let (started_tx, mut started_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(8);
    let written: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(Vec::new()));
    let transport = Arc::new(AtomicU8::new(TRANSPORT_UNDETECTED));

    let written_for_sink = Arc::clone(&written);
    let (tx, handle) = spawn_stdout_writer(Arc::clone(&transport), move || TestSink {
        started: started_tx,
        gate: None,
        written: written_for_sink,
        fail_at_call: None,
        call_count: 0,
    });

    let msg = MuxMessage::pty_output(1, vec![0xAB]);
    send_bounded(&tx, msg.clone()).await;
    tokio::time::timeout(STDOUT_WRITER_TEST_TIMEOUT, started_rx.recv())
        .await
        .expect("writer must start within the timeout")
        .expect("started channel open");

    drop(tx);
    tokio::time::timeout(STDOUT_WRITER_TEST_TIMEOUT, handle)
        .await
        .expect("writer quiesces")
        .expect("writer task must not panic");

    let got = written.lock().expect("written mutex poisoned");
    assert_eq!(
        *got,
        vec![msg.to_osc().into_bytes(), msg.to_apc().into_bytes()],
        "an undetected transport must send both OSC and APC, OSC first"
    );
}

/// AC-2: while the pump's sink is blocked — and even once its channel
/// is full — the sibling stdin→daemon direction keeps making
/// progress: a stdin-fed mux message still reaches the daemon-side
/// transport within the timeout.
///
/// Honest-TDD note (task plan): this criterion cannot be observed red
/// against the pre-task0002 code, because that code wrote directly to
/// the real process stdout, which a test cannot stall. It is verified
/// here against the reworked structure (this test plus the
/// per-invariant pump tests above), not as red-then-green.
#[tokio::test]
async fn forward_loop_keeps_stdin_to_daemon_progressing_while_stdout_sink_is_blocked() {
    use tokio::io::AsyncReadExt as _;
    use tokio::io::AsyncWriteExt as _;

    let (sock, mut daemon_side) = tokio::io::duplex(1 << 20);
    let (mut sock_reader, mut sock_writer) = tokio::io::split(sock);
    let (mut stdin, mut stdin_writer) = tokio::io::duplex(4096);
    let mut parser = StdinApcParser::new();
    let transport = Arc::new(AtomicU8::new(Transport::Apc as u8));
    let last_attach: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));

    let gate = Arc::new(Gate::new());
    let (started_tx, mut started_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(8);
    let written: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(Vec::new()));

    let gate_for_sink = Arc::clone(&gate);
    let written_for_sink = Arc::clone(&written);

    let forward = forward_loop_inner(
        &mut sock_reader,
        &mut sock_writer,
        &transport,
        &last_attach,
        &mut stdin,
        &mut parser,
        move || TestSink {
            started: started_tx,
            gate: Some(gate_for_sink),
            written: written_for_sink,
            fail_at_call: None,
            call_count: 0,
        },
    );

    let driver = async move {
        // Comfortably beyond the bound, so the writer's channel is
        // guaranteed full (sink blocked, nothing draining) once these
        // are all admitted.
        for i in 0..(STDOUT_WRITER_CAPACITY as u32 + 2) {
            let out = MuxMessage::pty_output(1, vec![i as u8]);
            let body = out.to_frame_body();
            daemon_side
                .write_all(&(body.len() as u32).to_be_bytes())
                .await
                .expect("write frame length");
            daemon_side
                .write_all(&body)
                .await
                .expect("write frame body");
            daemon_side.flush().await.expect("flush");
        }

        // Confirm the writer actually started (picked up the first
        // frame and is now parked on the gate) before proceeding.
        tokio::time::timeout(STDOUT_WRITER_TEST_TIMEOUT, started_rx.recv())
            .await
            .expect("writer must start within the timeout")
            .expect("started channel open");

        // Let the scheduler finish draining the socket into the
        // (now full) admission channel before checking the sibling
        // direction.
        tokio::time::sleep(STDOUT_WRITER_TEST_SETTLE).await;

        // Even with the stdout writer fully stalled and its channel
        // full, a stdin-fed keystroke must still reach the daemon
        // side within the timeout (AC-2).
        let keystroke = MuxMessage::pty_input(9, vec![0x41]);
        let apc = keystroke.to_apc();
        stdin_writer
            .write_all(apc.as_bytes())
            .await
            .expect("write keystroke");
        stdin_writer.flush().await.expect("flush keystroke");

        let mut len_buf = [0u8; 4];
        tokio::time::timeout(
            STDOUT_WRITER_TEST_TIMEOUT,
            daemon_side.read_exact(&mut len_buf),
        )
        .await
        .expect("keystroke must reach the daemon side within the timeout")
        .expect("read forwarded frame length");
        let frame_len = u32::from_be_bytes(len_buf) as usize;
        let mut frame_buf = vec![0u8; frame_len];
        daemon_side
            .read_exact(&mut frame_buf)
            .await
            .expect("read forwarded frame body");
        let forwarded = MuxMessage::from_frame_body(&frame_buf).expect("valid forwarded frame");
        assert_eq!(forwarded.msg_type, MessageType::PtyInput);
        assert_eq!(forwarded.pane_id, 9);
        assert_eq!(forwarded.payload, vec![0x41]);

        // Let forward_loop_inner end cleanly instead of running
        // forever: release the sink and close stdin.
        gate.release();
        drop(stdin_writer);
    };

    let (ended, ()) = tokio::join!(forward, driver);
    assert_eq!(ended, ConnectionEnded::Normal);
}

/// AC-5: a sink write error ends the forwarding — the async side
/// observes termination and `forward_loop`/`daemon_to_stdout`
/// conclude with the same `ConnectionEnded` semantics as today's
/// break-on-write-error (no `Upgrading` frame arrived here, so this
/// concludes `Normal`; `Announced` classification is a separate path,
/// unaffected by this change).
#[tokio::test]
async fn forward_loop_ends_normal_on_stdout_sink_write_error() {
    use tokio::io::AsyncWriteExt as _;

    let (sock, mut daemon_side) = tokio::io::duplex(65536);
    let (mut sock_reader, mut sock_writer) = tokio::io::split(sock);
    let transport = Arc::new(AtomicU8::new(Transport::Apc as u8));
    let last_attach: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
    let (mut stdin, _stdin_writer) = tokio::io::duplex(64);
    let mut parser = StdinApcParser::new();

    let (started_tx, _started_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(8);
    let written: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(Vec::new()));
    let written_for_sink = Arc::clone(&written);

    let msg = MuxMessage::pty_output(1, vec![0xAA]);
    let body = msg.to_frame_body();
    daemon_side
        .write_all(&(body.len() as u32).to_be_bytes())
        .await
        .expect("write frame length");
    daemon_side
        .write_all(&body)
        .await
        .expect("write frame body");
    daemon_side.flush().await.expect("flush");

    let ended = tokio::time::timeout(
        STDOUT_WRITER_TEST_TIMEOUT,
        forward_loop_inner(
            &mut sock_reader,
            &mut sock_writer,
            &transport,
            &last_attach,
            &mut stdin,
            &mut parser,
            move || TestSink {
                started: started_tx,
                gate: None,
                written: written_for_sink,
                fail_at_call: Some(0),
                call_count: 0,
            },
        ),
    )
    .await
    .expect("forward_loop must end within the timeout once the sink fails");

    assert_eq!(
        ended,
        ConnectionEnded::Normal,
        "no Upgrading frame arrived, so the connection ends Normal"
    );
    assert!(
        written.lock().expect("written mutex poisoned").is_empty(),
        "the failed write must not be recorded as delivered"
    );
}
