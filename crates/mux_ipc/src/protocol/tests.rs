use super::*;

#[test]
fn test_message_type_round_trip() {
    for i in 0x01..=0x1Cu8 {
        if i == 0x11 || i == 0x16 || i == 0x17 {
            // 0x11 (SplitPane) was removed; 0x16/0x17 (StatusUpdate /
            // RequestStatusUpdate) were removed by
            // mux-status-bar-removal task0001 -- all three must
            // return None (reserved, never reassigned).
            continue;
        }
        let mt = MessageType::from_u8(i).unwrap();
        assert_eq!(mt as u8, i);
    }
    assert!(MessageType::from_u8(0x00).is_none());
    assert!(MessageType::from_u8(0x11).is_none());
    assert!(MessageType::from_u8(0x16).is_none());
    assert!(MessageType::from_u8(0x17).is_none());
    assert_eq!(MessageType::from_u8(0x1B), Some(MessageType::SetVisibility));
    assert_eq!(MessageType::from_u8(0x1C), Some(MessageType::Notify));
    // 0x1D..=0x24 (previously unused) now hold the task0002 agent-status
    // / agent-API additions; see `test_agent_api_message_type_round_trip`
    // for full per-discriminant coverage. 0x25..=0x26 (previously
    // unused) now hold the mux-daemon-hot-upgrade task0001 `Upgrade` /
    // `Upgrading` additions; see
    // `test_upgrade_message_type_round_trip`. The unused-space boundary
    // this assertion pins moves to 0x27.
    assert_eq!(
        MessageType::from_u8(0x1D),
        Some(MessageType::AgentStatusUpdate)
    );
    assert!(MessageType::from_u8(0x27).is_none());
    assert!(MessageType::from_u8(0xff).is_none());
}

#[test]
fn test_notify_message_type() {
    assert_eq!(MessageType::from_u8(0x1C), Some(MessageType::Notify));
    assert_eq!(MessageType::Notify as u8, 0x1C);
}

#[test]
fn test_notify_msg_round_trip() {
    let msg = NotifyMsg {
        message: "build done".to_string(),
    };
    let bytes = bincode::serialize(&msg).unwrap();
    let decoded: NotifyMsg = bincode::deserialize(&bytes).unwrap();
    assert_eq!(decoded.message, "build done");
}

#[test]
fn test_notify_msg_via_mux_message() {
    let notify = NotifyMsg {
        message: "ビルド完了 🎉".to_string(),
    };
    let msg = MuxMessage::control(MessageType::Notify, 7, &notify);
    let body = msg.to_frame_body();
    let parsed = MuxMessage::from_frame_body(&body).unwrap();
    assert_eq!(parsed.msg_type, MessageType::Notify);
    assert_eq!(parsed.pane_id, 7);
    let decoded: NotifyMsg = parsed.decode_payload().unwrap();
    assert_eq!(decoded.message, "ビルド完了 🎉");
}

#[test]
fn test_move_window_message_type() {
    assert_eq!(MessageType::from_u8(0x1A), Some(MessageType::MoveWindow));
    assert_eq!(MessageType::MoveWindow as u8, 0x1A);
}

#[test]
fn test_set_visibility_message_type() {
    assert_eq!(MessageType::from_u8(0x1B), Some(MessageType::SetVisibility));
    assert_eq!(MessageType::SetVisibility as u8, 0x1B);
}

#[test]
fn test_set_visibility_payload_round_trip() {
    for visible in [true, false] {
        let payload = SetVisibilityPayload { visible };
        let bytes = payload.to_payload();
        assert_eq!(bytes.len(), 1);
        let decoded = SetVisibilityPayload::from_payload(&bytes).unwrap();
        assert_eq!(decoded.visible, visible);
    }
}

#[test]
fn test_set_visibility_via_mux_message_apc_round_trip() {
    for visible in [true, false] {
        let payload = SetVisibilityPayload { visible };
        let msg = MuxMessage {
            msg_type: MessageType::SetVisibility,
            pane_id: 0,
            payload: payload.to_payload(),
        };
        let apc = msg.to_apc();
        let body = &apc[2..apc.len() - 2];
        let decoded = MuxMessage::from_apc(body).unwrap();
        assert_eq!(decoded.msg_type, MessageType::SetVisibility);
        assert_eq!(decoded.pane_id, 0);
        assert_eq!(decoded.payload.len(), 1);
        let payload_back = SetVisibilityPayload::from_payload(&decoded.payload).unwrap();
        assert_eq!(payload_back.visible, visible);
    }
}

#[test]
fn test_set_visibility_payload_empty_returns_none() {
    assert!(SetVisibilityPayload::from_payload(&[]).is_none());
}

#[test]
fn test_move_window_msg_round_trip() {
    let msg = MoveWindowMsg { target_index: 42 };
    let bytes = bincode::serialize(&msg).unwrap();
    // bincode u32 should be 4 bytes LE
    assert_eq!(bytes.len(), 4);
    let decoded: MoveWindowMsg = bincode::deserialize(&bytes).unwrap();
    assert_eq!(decoded.target_index, 42);
}

#[test]
fn test_move_window_msg_via_mux_message() {
    let move_msg = MoveWindowMsg { target_index: 3 };
    let msg = MuxMessage::control(MessageType::MoveWindow, 99, &move_msg);
    let body = msg.to_frame_body();
    let parsed = MuxMessage::from_frame_body(&body).unwrap();
    assert_eq!(parsed.msg_type, MessageType::MoveWindow);
    assert_eq!(parsed.pane_id, 99);
    let decoded: MoveWindowMsg = parsed.decode_payload().unwrap();
    assert_eq!(decoded.target_index, 3);
}

#[test]
fn test_move_window_msg_zero_index() {
    let msg = MoveWindowMsg { target_index: 0 };
    let bytes = bincode::serialize(&msg).unwrap();
    let decoded: MoveWindowMsg = bincode::deserialize(&bytes).unwrap();
    assert_eq!(decoded.target_index, 0);
}

#[test]
fn test_request_pane_snapshot_message_type() {
    assert_eq!(
        MessageType::from_u8(0x19),
        Some(MessageType::RequestPaneSnapshot)
    );
    assert_eq!(MessageType::RequestPaneSnapshot as u8, 0x19);
}

#[test]
fn test_pty_output_frame_round_trip() {
    let msg = MuxMessage::pty_output(42, vec![1, 2, 3, 4]);
    let body = msg.to_frame_body();
    let parsed = MuxMessage::from_frame_body(&body).unwrap();
    assert_eq!(parsed.msg_type, MessageType::PtyOutput);
    assert_eq!(parsed.pane_id, 42);
    assert_eq!(parsed.payload, vec![1, 2, 3, 4]);
}

#[test]
fn test_control_message_round_trip() {
    let hello = HelloMsg {
        client_type: ClientType::Gui,
        protocol_version: PROTOCOL_VERSION,
    };
    let msg = MuxMessage::control(MessageType::Hello, 0, &hello);
    let body = msg.to_frame_body();
    let parsed = MuxMessage::from_frame_body(&body).unwrap();
    let decoded: HelloMsg = parsed.decode_payload().unwrap();
    assert_eq!(decoded.client_type, ClientType::Gui);
    assert_eq!(decoded.protocol_version, PROTOCOL_VERSION);
}

#[test]
fn test_welcome_accepted_round_trip() {
    let welcome = WelcomeMsg::Accepted {
        server_version: 1,
        sessions: vec![SessionInfo {
            id: 1,
            name: "main".to_string(),
            window_count: 2,
            pane_count: 3,
            active_window_index: 0,
            windows: vec![],
        }],
    };
    let msg = MuxMessage::control(MessageType::Welcome, 0, &welcome);
    let decoded: WelcomeMsg = MuxMessage::from_frame_body(&msg.to_frame_body())
        .unwrap()
        .decode_payload()
        .unwrap();
    match decoded {
        WelcomeMsg::Accepted {
            server_version,
            sessions,
        } => {
            assert_eq!(server_version, 1);
            assert_eq!(sessions.len(), 1);
            assert_eq!(sessions[0].name, "main");
        }
        _ => panic!("Expected Accepted"),
    }
}

#[test]
fn test_create_window_payload_both_none() {
    let payload = CreateWindowPayload {
        name: None,
        command: None,
    };
    let msg = MuxMessage::control(MessageType::CreateWindow, 0, &payload);
    let parsed = MuxMessage::from_frame_body(&msg.to_frame_body()).unwrap();
    let decoded: CreateWindowPayload = parsed.decode_payload().unwrap();
    assert_eq!(decoded.name, None);
    assert_eq!(decoded.command, None);
}

#[test]
fn test_create_window_payload_name_only() {
    let payload = CreateWindowPayload {
        name: Some("editor".to_string()),
        command: None,
    };
    let msg = MuxMessage::control(MessageType::CreateWindow, 0, &payload);
    let parsed = MuxMessage::from_frame_body(&msg.to_frame_body()).unwrap();
    let decoded: CreateWindowPayload = parsed.decode_payload().unwrap();
    assert_eq!(decoded.name, Some("editor".to_string()));
    assert_eq!(decoded.command, None);
}

#[test]
fn test_create_window_payload_command_only() {
    let payload = CreateWindowPayload {
        name: None,
        command: Some("nvim".to_string()),
    };
    let msg = MuxMessage::control(MessageType::CreateWindow, 0, &payload);
    let parsed = MuxMessage::from_frame_body(&msg.to_frame_body()).unwrap();
    let decoded: CreateWindowPayload = parsed.decode_payload().unwrap();
    assert_eq!(decoded.name, None);
    assert_eq!(decoded.command, Some("nvim".to_string()));
}

#[test]
fn test_create_window_payload_both_present() {
    let payload = CreateWindowPayload {
        name: Some("editor".to_string()),
        command: Some("nvim".to_string()),
    };
    let msg = MuxMessage::control(MessageType::CreateWindow, 0, &payload);
    let parsed = MuxMessage::from_frame_body(&msg.to_frame_body()).unwrap();
    let decoded: CreateWindowPayload = parsed.decode_payload().unwrap();
    assert_eq!(decoded.name, Some("editor".to_string()));
    assert_eq!(decoded.command, Some("nvim".to_string()));
}

#[test]
fn test_create_window_payload_empty_payload_backward_compat() {
    // Empty payload (from GUI) should fail to decode as CreateWindowPayload
    // Handler should use defaults in this case
    let msg = MuxMessage {
        msg_type: MessageType::CreateWindow,
        pane_id: 0,
        payload: vec![],
    };
    let decoded: Option<CreateWindowPayload> = msg.decode_payload();
    // Empty payload cannot be deserialized - handler uses defaults
    assert!(decoded.is_none());
}

#[test]
fn test_create_window_payload_default() {
    let payload = CreateWindowPayload::default();
    assert_eq!(payload.name, None);
    assert_eq!(payload.command, None);
}

#[test]
fn test_from_frame_body_too_short() {
    assert!(MuxMessage::from_frame_body(&[]).is_none());
    assert!(MuxMessage::from_frame_body(&[1, 2, 3, 4]).is_none());
}

#[test]
fn test_from_frame_body_invalid_type() {
    assert!(MuxMessage::from_frame_body(&[0xFF, 0, 0, 0, 0]).is_none());
}

#[test]
fn test_empty_payload() {
    let msg = MuxMessage::pty_output(0, vec![]);
    let body = msg.to_frame_body();
    assert_eq!(body.len(), 5); // type + pane_id only
    let parsed = MuxMessage::from_frame_body(&body).unwrap();
    assert!(parsed.payload.is_empty());
}

// ---- APC encode/decode tests ----

#[test]
fn test_apc_round_trip_pty_output() {
    let msg = MuxMessage::pty_output(42, vec![1, 2, 3, 4]);
    let apc = msg.to_apc();
    // Verify APC format
    assert!(apc.starts_with("\x1b_emterm-mux;"));
    assert!(apc.ends_with("\x1b\\"));
    // Extract payload between delimiters
    let payload = &apc[2..apc.len() - 2]; // strip ESC_ and ESC\
    let decoded = MuxMessage::from_apc(payload).unwrap();
    assert_eq!(decoded.msg_type, MessageType::PtyOutput);
    assert_eq!(decoded.pane_id, 42);
    assert_eq!(decoded.payload, vec![1, 2, 3, 4]);
}

#[test]
fn test_apc_round_trip_control_hello() {
    let hello = HelloMsg {
        client_type: ClientType::Gui,
        protocol_version: PROTOCOL_VERSION,
    };
    let msg = MuxMessage::control(MessageType::Hello, 0, &hello);
    let apc = msg.to_apc();
    let payload = &apc[2..apc.len() - 2];
    let decoded = MuxMessage::from_apc(payload).unwrap();
    assert_eq!(decoded.msg_type, MessageType::Hello);
    let hello_decoded: HelloMsg = decoded.decode_payload().unwrap();
    assert_eq!(hello_decoded.client_type, ClientType::Gui);
}

#[test]
fn test_apc_round_trip_all_message_types() {
    for i in 0x01..=0x1Cu8 {
        if i == 0x11 || i == 0x16 || i == 0x17 {
            // 0x11 (SplitPane) was removed; 0x16/0x17 (StatusUpdate /
            // RequestStatusUpdate) were removed by
            // mux-status-bar-removal task0001.
            continue;
        }
        let mt = MessageType::from_u8(i).unwrap();
        let msg = MuxMessage {
            msg_type: mt,
            pane_id: i as u32,
            payload: vec![i; 4],
        };
        let apc = msg.to_apc();
        let payload = &apc[2..apc.len() - 2];
        let decoded = MuxMessage::from_apc(payload).unwrap();
        assert_eq!(decoded.msg_type, mt);
        assert_eq!(decoded.pane_id, i as u32);
        assert_eq!(decoded.payload, vec![i; 4]);
    }
}

#[test]
fn test_apc_round_trip_empty_payload() {
    let msg = MuxMessage::pty_output(0, vec![]);
    let apc = msg.to_apc();
    let payload = &apc[2..apc.len() - 2];
    let decoded = MuxMessage::from_apc(payload).unwrap();
    assert!(decoded.payload.is_empty());
}

#[test]
fn test_apc_from_apc_missing_prefix() {
    let err = MuxMessage::from_apc("wrong-prefix;AAAA").unwrap_err();
    assert_eq!(err, ApcDecodeError::MissingPrefix);
}

#[test]
fn test_apc_from_apc_invalid_base64() {
    let err = MuxMessage::from_apc("emterm-mux;!!!invalid!!!").unwrap_err();
    assert_eq!(err, ApcDecodeError::InvalidBase64);
}

#[test]
fn test_apc_from_apc_invalid_frame_body() {
    use base64::Engine;
    // Valid base64 but too short for a frame body (< 5 bytes)
    let encoded = BASE64.encode(&[0x01]);
    let input = format!("emterm-mux;{}", encoded);
    let err = MuxMessage::from_apc(&input).unwrap_err();
    assert_eq!(err, ApcDecodeError::InvalidFrameBody);
}

#[test]
fn test_apc_from_apc_invalid_message_type() {
    use base64::Engine;
    // Valid base64, 5 bytes, but invalid message type 0xFF
    let encoded = BASE64.encode(&[0xFF, 0, 0, 0, 0]);
    let input = format!("emterm-mux;{}", encoded);
    let err = MuxMessage::from_apc(&input).unwrap_err();
    assert_eq!(err, ApcDecodeError::InvalidFrameBody);
}

#[test]
fn test_apc_from_apc_empty_after_prefix() {
    // emterm-mux; with empty base64 => empty bytes => invalid frame body
    let err = MuxMessage::from_apc("emterm-mux;").unwrap_err();
    assert_eq!(err, ApcDecodeError::InvalidFrameBody);
}

/// AC-3 (mux-status-bar-removal task0001): a raw frame carrying either
/// retired opcode (0x16 / 0x17 -- former `StatusUpdate` /
/// `RequestStatusUpdate`, reserved-not-reused) decodes as a benign
/// `InvalidFrameBody` error, the same non-fatal outcome as any other
/// unrecognized message type -- never a panic, and never naming the
/// retired types, so this test stays valid regardless of whether the
/// types still exist anywhere in the tree.
#[test]
fn test_apc_from_apc_retired_status_bar_opcodes_are_non_fatal() {
    use base64::Engine;
    for retired_opcode in [0x16u8, 0x17u8] {
        let encoded = BASE64.encode([retired_opcode, 0, 0, 0, 0]);
        let input = format!("emterm-mux;{}", encoded);
        let err = MuxMessage::from_apc(&input).unwrap_err();
        assert_eq!(err, ApcDecodeError::InvalidFrameBody);
    }
}

// ---- OSC encode/decode tests ----

#[test]
fn test_osc_round_trip_pty_output() {
    let msg = MuxMessage::pty_output(42, vec![1, 2, 3, 4]);
    let osc = msg.to_osc();
    // Verify OSC format
    assert!(osc.starts_with("\x1b]9999;emterm-mux;"));
    assert!(osc.ends_with("\x1b\\"));
    // Extract the APC-compatible payload (after "9999;")
    // OSC: ESC ] 9999 ; <body> ESC \
    // Strip ESC ] (2 bytes) at start and ESC \ (2 bytes) at end
    let inner = &osc[2..osc.len() - 2]; // "9999;emterm-mux;<base64>"
    let apc_payload = inner.strip_prefix("9999;").unwrap();
    let decoded = MuxMessage::from_apc(apc_payload).unwrap();
    assert_eq!(decoded.msg_type, MessageType::PtyOutput);
    assert_eq!(decoded.pane_id, 42);
    assert_eq!(decoded.payload, vec![1, 2, 3, 4]);
}

#[test]
fn test_osc_round_trip_control_hello() {
    let hello = HelloMsg {
        client_type: ClientType::Gui,
        protocol_version: PROTOCOL_VERSION,
    };
    let msg = MuxMessage::control(MessageType::Hello, 0, &hello);
    let osc = msg.to_osc();
    let inner = &osc[2..osc.len() - 2];
    let apc_payload = inner.strip_prefix("9999;").unwrap();
    let decoded = MuxMessage::from_apc(apc_payload).unwrap();
    assert_eq!(decoded.msg_type, MessageType::Hello);
    let hello_decoded: HelloMsg = decoded.decode_payload().unwrap();
    assert_eq!(hello_decoded.client_type, ClientType::Gui);
}

#[test]
fn test_osc_round_trip_empty_payload() {
    let msg = MuxMessage::pty_output(0, vec![]);
    let osc = msg.to_osc();
    let inner = &osc[2..osc.len() - 2];
    let apc_payload = inner.strip_prefix("9999;").unwrap();
    let decoded = MuxMessage::from_apc(apc_payload).unwrap();
    assert!(decoded.payload.is_empty());
}

#[test]
fn test_apc_large_payload() {
    let data = vec![0xAB; 65536];
    let msg = MuxMessage::pty_output(99, data.clone());
    let apc = msg.to_apc();
    let payload = &apc[2..apc.len() - 2];
    let decoded = MuxMessage::from_apc(payload).unwrap();
    assert_eq!(decoded.payload, data);
}

// ---- WindowInfo and extended SessionInfo tests ----

#[test]
fn test_window_info_serde_roundtrip() {
    let info = WindowInfo {
        id: 1,
        name: "editor".to_string(),
        active_pane_id: 42,
    };
    let bytes = bincode::serialize(&info).unwrap();
    let decoded: WindowInfo = bincode::deserialize(&bytes).unwrap();
    assert_eq!(decoded.id, 1);
    assert_eq!(decoded.name, "editor");
    assert_eq!(decoded.active_pane_id, 42);
}

#[test]
fn test_session_info_with_windows_roundtrip() {
    let info = SessionInfo {
        id: 1,
        name: "main".to_string(),
        window_count: 2,
        pane_count: 3,
        active_window_index: 0,
        windows: vec![
            WindowInfo {
                id: 1,
                name: "shell".to_string(),
                active_pane_id: 10,
            },
            WindowInfo {
                id: 2,
                name: "editor".to_string(),
                active_pane_id: 20,
            },
        ],
    };
    let bytes = bincode::serialize(&info).unwrap();
    let decoded: SessionInfo = bincode::deserialize(&bytes).unwrap();
    assert_eq!(decoded.windows.len(), 2);
    assert_eq!(decoded.windows[0].name, "shell");
    assert_eq!(decoded.windows[0].active_pane_id, 10);
    assert_eq!(decoded.windows[1].name, "editor");
    assert_eq!(decoded.windows[1].active_pane_id, 20);
}

#[test]
fn test_session_info_backward_compat_missing_windows() {
    // Simulate old SessionInfo without windows field (bincode)
    // by serializing a struct that has no windows field
    #[derive(Serialize)]
    struct OldSessionInfo {
        id: u32,
        name: String,
        window_count: u32,
        pane_count: u32,
        active_window_index: u32,
    }
    let old = OldSessionInfo {
        id: 1,
        name: "legacy".to_string(),
        window_count: 1,
        pane_count: 1,
        active_window_index: 0,
    };
    // For bincode, missing trailing field won't deserialize correctly,
    // but serde(default) handles JSON. Test via JSON for backward compat.
    let json = serde_json::to_string(&old).unwrap();
    let decoded: SessionInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.id, 1);
    assert_eq!(decoded.name, "legacy");
    assert!(decoded.windows.is_empty());
}

// ---- base64 transport inflation metrics (perf regression guard) ----
//
// The bridge/ConPTY transport encodes each `MuxMessage` frame body as
// base64 inside the APC / OSC envelope (`to_apc` / `to_osc`). base64
// inflates the body by a deterministic ~33% (4 output bytes per 3 input
// bytes, padded up), on top of a fixed-size envelope. These tests pin the
// exact byte counts so a future change to the transport encoding (or a
// regression that double-encodes) is caught, and so the perf work tracking
// "base64 adds 33%" has a stable, real-time-independent baseline.

/// base64(STANDARD) output length for `n` input bytes: 4 bytes per 3-byte
/// group, the final partial group padded to 4. This mirrors what
/// `BASE64.encode` produces and lets the tests assert the encoded size
/// without hard-coding magic numbers.
fn base64_len(n: usize) -> usize {
    n.div_ceil(3) * 4
}

#[test]
fn base64_inflation_to_apc_64kib_payload() {
    // Representative PtyOutput payload: 64 KiB of data.
    let payload_len = 64 * 1024; // 65536
    let msg = MuxMessage::pty_output(7, vec![0xAB; payload_len]);

    let frame_body = msg.to_frame_body();
    // frame body = 1 (type) + 4 (pane_id) + payload
    assert_eq!(frame_body.len(), 5 + payload_len, "frame body layout fixed");

    let apc = msg.to_apc();

    // Fixed envelope: ESC _ (2) + "emterm-mux;" + base64 + ESC \ (2).
    let envelope_overhead = APC_START.len() + APC_PREFIX.len() + APC_ST.len();
    let expected_b64 = base64_len(frame_body.len());
    assert_eq!(
        apc.len(),
        envelope_overhead + expected_b64,
        "to_apc() size = fixed envelope + base64(frame_body)"
    );

    // base64 of a 65541-byte body: ceil(65541/3)*4 = 21847*4 = 87388.
    assert_eq!(
        expected_b64, 87388,
        "base64 length of the 64KiB+5 frame body"
    );

    // The inflation the perf work cares about: encoded-vs-raw-body ratio.
    // 87388 / 65541 ≈ 1.3333 (the canonical base64 +33%). Pin it tight.
    let body = frame_body.len();
    // Express as parts-per-thousand to keep the assertion integer-exact.
    let ratio_permille = expected_b64 * 1000 / body;
    assert_eq!(
        ratio_permille, 1333,
        "base64 inflates the frame body by ~33.3% (1333 permille)"
    );

    // Absolute inflation: encoded body is exactly 21847 bytes larger than
    // the raw body for this payload.
    assert_eq!(expected_b64 - body, 21847, "absolute base64 byte growth");
}

#[test]
fn base64_inflation_to_osc_matches_apc_plus_param() {
    // OSC adds only the "9999;" parameter over the APC envelope; the
    // base64 body is byte-for-byte identical. This pins that the OSC
    // fallback transport does not encode the payload any differently.
    let payload_len = 64 * 1024;
    let msg = MuxMessage::pty_output(3, vec![0xCD; payload_len]);

    let apc = msg.to_apc();
    let osc = msg.to_osc();

    // OSC envelope = ESC ] (2) + "9999" + ";" + "emterm-mux;" + b64 + ESC \.
    // APC envelope = ESC _ (2) + "emterm-mux;" + b64 + ESC \.
    // Both ESC introducers are 2 bytes, so the only delta is "9999;".
    let param_overhead = MUX_OSC_PARAM.to_string().len() + 1; // "9999" + ";"
    assert_eq!(
        osc.len(),
        apc.len() + param_overhead,
        "to_osc() = to_apc() + the OSC \"9999;\" parameter, same base64 body"
    );
    assert_eq!(param_overhead, 5, "OSC param overhead is exactly \"9999;\"");
}

#[test]
fn base64_inflation_ratio_is_payload_size_independent() {
    // The ~33% inflation holds across payload sizes (only the fixed
    // envelope changes the headline ratio for tiny payloads). Verify the
    // base64 body ratio converges to 4/3 as the payload grows, so the
    // perf model "base64 = +33%" is sound for the bulk-output case the
    // regression targets.
    for &payload_len in &[4 * 1024usize, 16 * 1024, 256 * 1024] {
        let msg = MuxMessage::pty_output(1, vec![0u8; payload_len]);
        let body = msg.to_frame_body().len();
        let encoded_body = base64_len(body);
        // 1333 permille = +33.3%. Large payloads stay within 1 permille.
        let ratio_permille = encoded_body * 1000 / body;
        assert!(
            (1333..=1334).contains(&ratio_permille),
            "payload {payload_len}: base64 body ratio {ratio_permille} permille not ~1333"
        );
    }
}

// ---- to_plaintext (Windows ConPTY input transport) ----

#[test]
fn to_plaintext_has_emux_prefix_and_cr_terminator() {
    let msg = MuxMessage::pty_output(7, b"hi".to_vec());
    let pt = msg.to_plaintext();
    assert!(pt.starts_with("EMUX;"), "got {pt:?}");
    assert!(pt.ends_with('\r'), "got {pt:?}");
    // No APC / OSC escapes in the body — ConPTY input strips those.
    assert!(
        !pt.contains('\x1b'),
        "plaintext envelope must be escape-free, got {pt:?}"
    );
    // CR is VK_RETURN through ConPTY's WIN32_INPUT_MODE; LF is NOT a
    // standard key and gets dropped on the host→bridge path, so the
    // terminator must be CR. This pins that regression.
    assert!(
        !pt.contains('\n'),
        "plaintext envelope must not carry LF (drops under ConPTY WIN32_INPUT_MODE), got {pt:?}"
    );
}

#[test]
fn to_plaintext_round_trips_with_bridge_parser_shape() {
    // The bridge's StdinApcParser strips the EMUX; prefix and \r
    // terminator, then prepends APC_PREFIX before calling from_apc.
    // Mirror that shape here so a wire-format drift between encoder and
    // parser fails this test. Uses `NotifyMsg` (a string-payload control
    // message, like the former `StatusUpdateMsg` this test used before
    // mux-status-bar-removal task0001) purely as a payload shape --
    // nothing about this test is specific to notifications.
    let payload = NotifyMsg {
        message: "left 🦀 right ✨".to_string(),
    };
    let msg = MuxMessage::control(MessageType::Notify, 11, &payload);

    let pt = msg.to_plaintext();
    let body = pt
        .strip_prefix("EMUX;")
        .and_then(|s| s.strip_suffix('\r'))
        .expect("plaintext envelope");
    let with_apc_prefix = format!("{}{}", APC_PREFIX, body);
    let decoded = MuxMessage::from_apc(&with_apc_prefix).expect("decoded");

    assert_eq!(decoded.msg_type, MessageType::Notify);
    assert_eq!(decoded.pane_id, 11);
    let back: NotifyMsg = decoded.decode_payload().unwrap();
    assert_eq!(back.message, "left 🦀 right ✨");
}

#[test]
fn to_plaintext_body_matches_to_apc_body() {
    // Both transports base64-encode the SAME frame body; only the
    // envelope differs. This pins that to_plaintext does not double-
    // wrap or otherwise change the protocol payload.
    let msg = MuxMessage::pty_output(3, vec![0xAB; 1024]);

    let apc = msg.to_apc();
    let pt = msg.to_plaintext();

    let apc_body = apc
        .strip_prefix("\x1b_emterm-mux;")
        .and_then(|s| s.strip_suffix("\x1b\\"))
        .unwrap();
    let pt_body = pt
        .strip_prefix("EMUX;")
        .and_then(|s| s.strip_suffix('\r'))
        .unwrap();
    assert_eq!(apc_body, pt_body, "base64 frame body must be identical");
}

#[test]
fn test_welcome_with_windows_roundtrip() {
    let welcome = WelcomeMsg::Accepted {
        server_version: 1,
        sessions: vec![SessionInfo {
            id: 1,
            name: "main".to_string(),
            window_count: 1,
            pane_count: 1,
            active_window_index: 0,
            windows: vec![WindowInfo {
                id: 1,
                name: "shell".to_string(),
                active_pane_id: 5,
            }],
        }],
    };
    let msg = MuxMessage::control(MessageType::Welcome, 0, &welcome);
    let decoded: WelcomeMsg = MuxMessage::from_frame_body(&msg.to_frame_body())
        .unwrap()
        .decode_payload()
        .unwrap();
    match decoded {
        WelcomeMsg::Accepted { sessions, .. } => {
            assert_eq!(sessions[0].windows.len(), 1);
            assert_eq!(sessions[0].windows[0].active_pane_id, 5);
        }
        _ => panic!("Expected Accepted"),
    }
}

// ---- agent-status / agent-API message additions (task0002) ----

/// AC-3: `from_u8` maps every new discriminant. The space right after
/// this extended range is occupied by the mux-daemon-hot-upgrade
/// task0001 `Upgrade` / `Upgrading` additions (see
/// `test_upgrade_message_type_round_trip`), so the still-unmapped
/// boundary this test pins moves to 0x27.
#[test]
fn test_agent_api_message_type_round_trip() {
    for i in 0x1Du8..=0x24u8 {
        let mt = MessageType::from_u8(i).unwrap();
        assert_eq!(mt as u8, i);
    }
    assert_eq!(
        MessageType::from_u8(0x1D),
        Some(MessageType::AgentStatusUpdate)
    );
    assert_eq!(MessageType::from_u8(0x1E), Some(MessageType::ReadPane));
    assert_eq!(
        MessageType::from_u8(0x1F),
        Some(MessageType::ReadPaneResult)
    );
    assert_eq!(MessageType::from_u8(0x20), Some(MessageType::SendText));
    assert_eq!(
        MessageType::from_u8(0x21),
        Some(MessageType::SendTextResult)
    );
    assert_eq!(
        MessageType::from_u8(0x22),
        Some(MessageType::WaitAgentState)
    );
    assert_eq!(
        MessageType::from_u8(0x23),
        Some(MessageType::WaitAgentStateResult)
    );
    assert_eq!(MessageType::from_u8(0x24), Some(MessageType::AgentApiError));
    assert!(MessageType::from_u8(0x27).is_none());
}

/// AC-1 / AC-3: APC round trip for every new discriminant, mirroring
/// `test_apc_round_trip_all_message_types` for the pre-existing range.
#[test]
fn test_apc_round_trip_agent_api_message_types() {
    for i in 0x1Du8..=0x24u8 {
        let mt = MessageType::from_u8(i).unwrap();
        let msg = MuxMessage {
            msg_type: mt,
            pane_id: i as u32,
            payload: vec![i; 4],
        };
        let apc = msg.to_apc();
        let payload = &apc[2..apc.len() - 2];
        let decoded = MuxMessage::from_apc(payload).unwrap();
        assert_eq!(decoded.msg_type, mt);
        assert_eq!(decoded.pane_id, i as u32);
        assert_eq!(decoded.payload, vec![i; 4]);
    }
}

/// AC-1: `AgentStatusUpdate` round-trips with a `Set`-like payload
/// (state + name present, not replay-derived).
#[test]
fn test_agent_status_update_msg_round_trip_set() {
    let update = AgentStatusUpdateMsg {
        pane_id: 7,
        public_pane_id: "ab12cd34-7".to_string(),
        state: Some(AgentState::Working),
        name: Some("build".to_string()),
        revision: 3,
        replay_derived: false,
    };
    let msg = MuxMessage::control(MessageType::AgentStatusUpdate, 7, &update);
    let parsed = MuxMessage::from_frame_body(&msg.to_frame_body()).unwrap();
    assert_eq!(parsed.msg_type, MessageType::AgentStatusUpdate);
    assert_eq!(parsed.pane_id, 7);
    let decoded: AgentStatusUpdateMsg = parsed.decode_payload().unwrap();
    assert_eq!(decoded.pane_id, 7);
    assert_eq!(decoded.public_pane_id, "ab12cd34-7");
    assert_eq!(decoded.state, Some(AgentState::Working));
    assert_eq!(decoded.name, Some("build".to_string()));
    assert_eq!(decoded.revision, 3);
    assert!(!decoded.replay_derived);
}

/// AC-1: `AgentStatusUpdate` round-trips with a `Clear`-like payload
/// (state + name absent) and `replay_derived: true`.
#[test]
fn test_agent_status_update_msg_round_trip_clear_replay_derived() {
    let update = AgentStatusUpdateMsg {
        pane_id: 12,
        public_pane_id: "ab12cd34-12".to_string(),
        state: None,
        name: None,
        revision: 9,
        replay_derived: true,
    };
    let msg = MuxMessage::control(MessageType::AgentStatusUpdate, 12, &update);
    let decoded: AgentStatusUpdateMsg = MuxMessage::from_frame_body(&msg.to_frame_body())
        .unwrap()
        .decode_payload()
        .unwrap();
    assert_eq!(decoded.state, None);
    assert_eq!(decoded.name, None);
    assert_eq!(decoded.revision, 9);
    assert!(decoded.replay_derived);
}

/// AC-1: `ReadPane` request / `ReadPaneResult` response round-trip.
#[test]
fn test_read_pane_request_and_result_round_trip() {
    let req = ReadPaneMsg {
        public_pane_id: "ab12cd34-3".to_string(),
        lines: 200,
    };
    let req_msg = MuxMessage::control(MessageType::ReadPane, 3, &req);
    let decoded_req: ReadPaneMsg = MuxMessage::from_frame_body(&req_msg.to_frame_body())
        .unwrap()
        .decode_payload()
        .unwrap();
    assert_eq!(decoded_req.public_pane_id, "ab12cd34-3");
    assert_eq!(decoded_req.lines, 200);

    let result = ReadPaneResultMsg {
        text: "line1\nline2\n🎉".to_string(),
    };
    let result_msg = MuxMessage::control(MessageType::ReadPaneResult, 3, &result);
    let decoded_result: ReadPaneResultMsg =
        MuxMessage::from_frame_body(&result_msg.to_frame_body())
            .unwrap()
            .decode_payload()
            .unwrap();
    assert_eq!(decoded_result.text, "line1\nline2\n🎉");
}

/// AC-1: `SendText` request / `SendTextResult` response round-trip.
#[test]
fn test_send_text_request_and_result_round_trip() {
    let req = SendTextMsg {
        public_pane_id: "ab12cd34-5".to_string(),
        bytes: b"echo hi\n".to_vec(),
    };
    let req_msg = MuxMessage::control(MessageType::SendText, 5, &req);
    let decoded_req: SendTextMsg = MuxMessage::from_frame_body(&req_msg.to_frame_body())
        .unwrap()
        .decode_payload()
        .unwrap();
    assert_eq!(decoded_req.public_pane_id, "ab12cd34-5");
    assert_eq!(decoded_req.bytes, b"echo hi\n".to_vec());

    let result = SendTextResultMsg {
        revision_watermark: 42,
    };
    let result_msg = MuxMessage::control(MessageType::SendTextResult, 5, &result);
    let decoded_result: SendTextResultMsg =
        MuxMessage::from_frame_body(&result_msg.to_frame_body())
            .unwrap()
            .decode_payload()
            .unwrap();
    assert_eq!(decoded_result.revision_watermark, 42);
}

/// AC-1: `WaitAgentState` request / `WaitAgentStateResult` response
/// round-trip, with `after_revision` present.
#[test]
fn test_wait_agent_state_request_and_result_round_trip() {
    let req = WaitAgentStateMsg {
        public_pane_id: "ab12cd34-9".to_string(),
        states: vec![AgentState::Blocked, AgentState::Done],
        timeout_ms: 5000,
        after_revision: Some(10),
    };
    let req_msg = MuxMessage::control(MessageType::WaitAgentState, 9, &req);
    let decoded_req: WaitAgentStateMsg = MuxMessage::from_frame_body(&req_msg.to_frame_body())
        .unwrap()
        .decode_payload()
        .unwrap();
    assert_eq!(decoded_req.public_pane_id, "ab12cd34-9");
    assert_eq!(
        decoded_req.states,
        vec![AgentState::Blocked, AgentState::Done]
    );
    assert_eq!(decoded_req.timeout_ms, 5000);
    assert_eq!(decoded_req.after_revision, Some(10));

    let result = WaitAgentStateResultMsg {
        state: AgentState::Done,
        revision: 11,
    };
    let result_msg = MuxMessage::control(MessageType::WaitAgentStateResult, 9, &result);
    let decoded_result: WaitAgentStateResultMsg =
        MuxMessage::from_frame_body(&result_msg.to_frame_body())
            .unwrap()
            .decode_payload()
            .unwrap();
    assert_eq!(decoded_result.state, AgentState::Done);
    assert_eq!(decoded_result.revision, 11);
}

/// AC-1: `WaitAgentState` request round-trips with `after_revision: None`.
#[test]
fn test_wait_agent_state_request_round_trip_no_after_revision() {
    let req = WaitAgentStateMsg {
        public_pane_id: "ab12cd34-1".to_string(),
        states: vec![AgentState::Idle],
        timeout_ms: 0,
        after_revision: None,
    };
    let msg = MuxMessage::control(MessageType::WaitAgentState, 1, &req);
    let decoded: WaitAgentStateMsg = MuxMessage::from_frame_body(&msg.to_frame_body())
        .unwrap()
        .decode_payload()
        .unwrap();
    assert_eq!(decoded.after_revision, None);
    assert_eq!(decoded.timeout_ms, 0);
}

/// AC-1: `AgentApiError` round-trips for every error kind.
#[test]
fn test_agent_api_error_round_trip_all_kinds() {
    let kinds = [
        AgentApiErrorKind::UnknownPane,
        AgentApiErrorKind::NotMuxPane,
        AgentApiErrorKind::Timeout,
        AgentApiErrorKind::PaneGone,
        AgentApiErrorKind::InvalidInput,
    ];
    for kind in kinds {
        let err = AgentApiError {
            kind,
            message: format!("error: {kind:?}"),
        };
        let msg = MuxMessage::control(MessageType::AgentApiError, 0, &err);
        let decoded: AgentApiError = MuxMessage::from_frame_body(&msg.to_frame_body())
            .unwrap()
            .decode_payload()
            .unwrap();
        assert_eq!(decoded.kind, kind);
        assert_eq!(decoded.message, format!("error: {kind:?}"));
    }
}

/// AC-1: `AgentApiErrorKind` serializes to the exact lowercase-snake
/// wire strings the CLI exit-code mapping depends on.
#[test]
fn test_agent_api_error_kind_wire_strings() {
    let cases = [
        (AgentApiErrorKind::UnknownPane, "\"unknown_pane\""),
        (AgentApiErrorKind::NotMuxPane, "\"not_mux_pane\""),
        (AgentApiErrorKind::Timeout, "\"timeout\""),
        (AgentApiErrorKind::PaneGone, "\"pane_gone\""),
        (AgentApiErrorKind::InvalidInput, "\"invalid_input\""),
    ];
    for (kind, expected_json) in cases {
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, expected_json);
    }
}

/// AC-1: `AgentState` serializes to the exact lowercase wire strings
/// the core `agent_status` module's mirror contract depends on.
#[test]
fn test_agent_state_wire_strings() {
    let cases = [
        (AgentState::Idle, "\"idle\""),
        (AgentState::Working, "\"working\""),
        (AgentState::Blocked, "\"blocked\""),
        (AgentState::Done, "\"done\""),
    ];
    for (state, expected_json) in cases {
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, expected_json);
    }
}

// ---- public pane ID helpers (task0002) ----

/// AC-4: compose → parse round-trips.
#[test]
fn test_public_pane_id_compose_parse_round_trip() {
    let composed = PublicPaneId::compose("ab12cd34", 7);
    assert_eq!(composed, "ab12cd34-7");
    let parsed = PublicPaneId::parse(&composed).unwrap();
    assert_eq!(
        parsed,
        PublicPaneId {
            incarnation: "ab12cd34".to_string(),
            pane_id: 7,
        }
    );
}

#[test]
fn test_public_pane_id_compose_parse_round_trip_pane_zero() {
    let composed = PublicPaneId::compose("0f", 0);
    let parsed = PublicPaneId::parse(&composed).unwrap();
    assert_eq!(parsed.incarnation, "0f");
    assert_eq!(parsed.pane_id, 0);
}

/// AC-4: parsing an empty string returns an error, never a panic.
#[test]
fn test_public_pane_id_parse_rejects_empty() {
    assert!(PublicPaneId::parse("").is_err());
}

/// AC-4: parsing a string with no `-` separator returns an error.
#[test]
fn test_public_pane_id_parse_rejects_missing_separator() {
    let err = PublicPaneId::parse("ab12cd347").unwrap_err();
    assert_eq!(err, PublicPaneIdError::MissingSeparator);
}

/// AC-4: parsing a string whose incarnation segment is not lowercase
/// hex returns an error.
#[test]
fn test_public_pane_id_parse_rejects_non_hex_incarnation() {
    let err = PublicPaneId::parse("AB12CD34-7").unwrap_err();
    assert_eq!(err, PublicPaneIdError::InvalidIncarnation);

    let err = PublicPaneId::parse("not-hex-zone-7").unwrap_err();
    assert_eq!(err, PublicPaneIdError::InvalidIncarnation);

    let err = PublicPaneId::parse("-7").unwrap_err();
    assert_eq!(err, PublicPaneIdError::InvalidIncarnation);
}

/// AC-4: parsing a pane-number segment that overflows `u32` returns an
/// error.
#[test]
fn test_public_pane_id_parse_rejects_pane_number_overflow() {
    let err = PublicPaneId::parse("ab12cd34-4294967296").unwrap_err();
    assert_eq!(err, PublicPaneIdError::InvalidPaneNumber);
}

/// AC-4: parsing a pane-number segment that is not numeric at all
/// returns an error.
#[test]
fn test_public_pane_id_parse_rejects_non_numeric_pane_number() {
    let err = PublicPaneId::parse("ab12cd34-abc").unwrap_err();
    assert_eq!(err, PublicPaneIdError::InvalidPaneNumber);
}

// ---- mux daemon hot-upgrade: Upgrade / Upgrading message types (task0001) ----

/// AC-1 / AC-3: `from_u8` maps both new discriminants to their own
/// distinct variants, and the byte immediately after them is still
/// unmapped.
#[test]
fn test_upgrade_message_type_round_trip() {
    for i in 0x25u8..=0x26u8 {
        let mt = MessageType::from_u8(i).unwrap();
        assert_eq!(mt as u8, i);
    }
    assert_eq!(MessageType::from_u8(0x25), Some(MessageType::Upgrade));
    assert_eq!(MessageType::from_u8(0x26), Some(MessageType::Upgrading));
    assert!(MessageType::from_u8(0x27).is_none());
}

/// AC-3: neither new discriminant collides with any existing value the
/// enumeration already maps, nor with the retired `0x11` (removed
/// `SplitPane`) or `0x16`/`0x17` (removed `StatusUpdate` /
/// `RequestStatusUpdate`, mux-status-bar-removal task0001).
#[test]
fn test_upgrade_message_type_bytes_do_not_collide_with_existing_or_retired() {
    for i in 0x01u8..=0x24u8 {
        if i == 0x11 || i == 0x16 || i == 0x17 {
            assert!(MessageType::from_u8(i).is_none());
            continue;
        }
        assert!(MessageType::from_u8(i).is_some());
        assert_ne!(i, MessageType::Upgrade as u8);
        assert_ne!(i, MessageType::Upgrading as u8);
    }
    assert_ne!(MessageType::Upgrade as u8, MessageType::Upgrading as u8);
}

/// AC-1: `Upgrade` and `Upgrading` round-trip through the frame body
/// encode/decode helpers, preserving type, pane id, and the empty
/// payload mandated by their wire shape (mirrors `Shutdown`: type byte,
/// pane id zero, empty payload).
#[test]
fn test_upgrade_and_upgrading_round_trip_through_frame_body() {
    for mt in [MessageType::Upgrade, MessageType::Upgrading] {
        let msg = MuxMessage {
            msg_type: mt,
            pane_id: 0,
            payload: Vec::new(),
        };
        let body = msg.to_frame_body();
        let decoded = MuxMessage::from_frame_body(&body).unwrap();
        assert_eq!(decoded.msg_type, mt);
        assert_eq!(decoded.pane_id, 0);
        assert!(decoded.payload.is_empty());
    }
}

/// AC-1: same round trip through the APC envelope, mirroring
/// `test_apc_round_trip_all_message_types` for the pre-existing range.
#[test]
fn test_apc_round_trip_upgrade_message_types() {
    for mt in [MessageType::Upgrade, MessageType::Upgrading] {
        let msg = MuxMessage {
            msg_type: mt,
            pane_id: 0,
            payload: Vec::new(),
        };
        let apc = msg.to_apc();
        let payload = &apc[2..apc.len() - 2];
        let decoded = MuxMessage::from_apc(payload).unwrap();
        assert_eq!(decoded.msg_type, mt);
        assert_eq!(decoded.pane_id, 0);
        assert!(decoded.payload.is_empty());
    }
}

/// AC-2: a frame carrying a type byte the decoder does not recognise is
/// reported as "not a known message" (`from_frame_body` returns `None`),
/// not as an error that would tear the connection down — checked for
/// the byte immediately adjacent to the new `Upgrading` discriminant.
#[test]
fn test_from_frame_body_returns_none_for_byte_adjacent_to_new_upgrade_types() {
    assert!(MessageType::from_u8(0x27).is_none());
    let mut body = vec![0x27u8];
    body.extend_from_slice(&0u32.to_le_bytes());
    assert!(MuxMessage::from_frame_body(&body).is_none());
}

/// AC-6: adding `Upgrade` / `Upgrading` does not bump `PROTOCOL_VERSION`
/// — no existing bincode structure changed (NFR6 / IMPLEMENTATION.md D7).
#[test]
fn test_protocol_version_unchanged_by_upgrade_message_types() {
    assert_eq!(PROTOCOL_VERSION, 3);
}

// ---- PROTOCOL_VERSION bump (task0002) ----

/// AC-5: `PROTOCOL_VERSION` was bumped to 2 for the agent-API additions.
#[test]
fn test_protocol_version_bumped_for_agent_api_additions() {
    assert!(PROTOCOL_VERSION >= 2);
}

// ---- task0005 rework D4'': PROTOCOL_VERSION bump for the D1'
// structural Snapshot/SnapshotRestore payload change (review round-4
// finding `fdfd391ba97167de`) ------------------------------------------

/// AC-6: `PROTOCOL_VERSION` reflects the incompatible `Snapshot` /
/// `SnapshotRestore` payload change — bumped to 3 so a new daemon and an
/// old (v2) GUI no longer handshake successfully and misinterpret the
/// `EMSNAP2` envelope as terminal content.
#[test]
fn test_protocol_version_bumped_for_structural_snapshot_payload_change() {
    assert_eq!(PROTOCOL_VERSION, 3);
}

// ---- task0010 rework: safe PROTOCOL_VERSION upgrade path (strategy B) ----

/// AC-1: the adjacent-version constant tracks `PROTOCOL_VERSION - 1`
/// exactly, so a future bump keeps the recovery retry one version back.
#[test]
fn test_previous_protocol_version_is_adjacent() {
    assert_eq!(PREVIOUS_PROTOCOL_VERSION, PROTOCOL_VERSION - 1);
    assert_eq!(PREVIOUS_PROTOCOL_VERSION, 2);
}

/// AC-1: parses the exact reason text the daemon's version-mismatch
/// path produces.
#[test]
fn test_parse_rejected_server_version_matches_daemon_format() {
    let reason = format!(
        "Protocol version mismatch: client={}, server={}",
        PROTOCOL_VERSION, PREVIOUS_PROTOCOL_VERSION
    );
    assert_eq!(
        parse_rejected_server_version(&reason),
        Some(PREVIOUS_PROTOCOL_VERSION)
    );
}

/// AC-3: a rejection for any other reason never gets misread as a
/// version number — no panic, just `None`.
#[test]
fn test_parse_rejected_server_version_returns_none_for_unrelated_reason() {
    assert_eq!(parse_rejected_server_version("Connection refused"), None);
    assert_eq!(parse_rejected_server_version(""), None);
    assert_eq!(parse_rejected_server_version("server=not-a-number"), None);
    assert_eq!(parse_rejected_server_version("server="), None);
}

/// AC-1: only the digits immediately after `server=` are consumed, so
/// trailing text in a future reason format doesn't corrupt the parse.
#[test]
fn test_parse_rejected_server_version_stops_at_non_digit() {
    assert_eq!(
        parse_rejected_server_version("client=2, server=1 (extra info)"),
        Some(1)
    );
}

// ── task0004 round-4 rework (D1'): structural snapshot segments ──────

#[test]
fn encode_decode_snapshot_payload_round_trips_multiple_segments() {
    // Offsets must be non-decreasing, start at 0, and stay within
    // `bytes`' length (D2''' round-6 rework) — this test's `bytes` is
    // 42 bytes, so the third segment's offset is chosen accordingly
    // rather than the earlier (unrealistic, content-exceeding) 4096.
    let segments = vec![
        DimSegment {
            offset: 0,
            cols: 80,
            rows: 24,
        },
        DimSegment {
            offset: 20,
            cols: 120,
            rows: 40,
        },
        DimSegment {
            offset: 40,
            cols: 200,
            rows: 50,
        },
    ];
    let bytes = b"hello resize world, this is plain content".to_vec();
    assert!(bytes.len() >= 40, "test fixture offsets assume len >= 40");
    let encoded = encode_snapshot_payload(&segments, &bytes);
    let (decoded_segments, decoded_bytes) = decode_snapshot_payload(&encoded);
    assert_eq!(decoded_segments, segments);
    assert_eq!(decoded_bytes, bytes.as_slice());
}

#[test]
fn encode_decode_snapshot_payload_round_trips_empty_segments() {
    let bytes = b"no dimension changes recorded".to_vec();
    let encoded = encode_snapshot_payload(&[], &bytes);
    let (decoded_segments, decoded_bytes) = decode_snapshot_payload(&encoded);
    assert!(decoded_segments.is_empty());
    assert_eq!(decoded_bytes, bytes.as_slice());
}

/// AC-11: a legacy payload (no magic prefix — an older daemon that never
/// adopted the D1' wire format) decodes with NO segments and the WHOLE
/// input preserved as content bytes, never misinterpreted or truncated.
#[test]
fn decode_snapshot_payload_falls_back_for_legacy_payload_without_magic() {
    let legacy = b"\x1b[3J\x1b[H\x1b[2Jsome legacy ansi snapshot bytes".to_vec();
    let (segments, bytes) = decode_snapshot_payload(&legacy);
    assert!(segments.is_empty());
    assert_eq!(bytes, legacy.as_slice());
}

#[test]
fn decode_snapshot_payload_falls_back_for_empty_payload() {
    let (segments, bytes) = decode_snapshot_payload(&[]);
    assert!(segments.is_empty());
    assert!(bytes.is_empty());
}

/// A truncated segment table (transfer cut mid-header) must not panic
/// or read out of bounds. task0005 rework D2'' (review round-4 finding
/// `5299d50f586b8cb8`): the magic prefix proves this was MEANT to be a
/// structured payload, so it is reported as `Malformed` — the tuple
/// compat wrapper maps that to EMPTY content, not the raw
/// magic-plus-garbage bytes. Confirmed to fail pre-fix: the old
/// implementation returned `(Vec::new(), malformed.as_slice())` here,
/// i.e. the WHOLE magic + truncated-table bytes handed back as if they
/// were plain terminal content — exactly the bug this test now guards
/// against.
#[test]
fn decode_snapshot_payload_falls_back_for_truncated_segment_table() {
    let mut malformed = Vec::new();
    malformed.extend_from_slice(&SNAPSHOT_PAYLOAD_MAGIC);
    malformed.extend_from_slice(&2u32.to_le_bytes()); // claims 2 segments
    malformed.extend_from_slice(&[1, 2, 3]); // but far too few bytes follow
    assert_eq!(
        decode_snapshot_payload_typed(&malformed),
        DecodedSnapshotPayload::Malformed
    );
    let (segments, bytes) = decode_snapshot_payload(&malformed);
    assert!(segments.is_empty());
    assert!(
        bytes.is_empty(),
        "a malformed structured frame must never surface its magic/table \
         bytes as replayable content, got {bytes:?}"
    );
}

/// D2'' (review round-4 finding `1cd7b5e593f3b901`): a segment count
/// above `MAX_SEGMENTS` is rejected as `Malformed` BEFORE any per-entry
/// parsing — even though the actual table bytes following it happen to
/// be well-formed for the (smaller) number of entries genuinely
/// present. Confirmed to fail pre-fix: the old decoder had no count
/// ceiling at all (only `count.min(4096)` for the initial `Vec`
/// capacity), so an oversized `count` here would have looped far past
/// `MAX_SEGMENTS`, parsing whatever real entries followed instead of
/// rejecting the frame outright.
#[test]
fn decode_snapshot_payload_rejects_a_count_above_max_segments() {
    let mut malformed = Vec::new();
    malformed.extend_from_slice(&SNAPSHOT_PAYLOAD_MAGIC);
    malformed.extend_from_slice(&((MAX_SEGMENTS as u32) + 1).to_le_bytes());
    // No table bytes at all — irrelevant, the count ceiling must reject
    // this before the table length is even checked.
    assert_eq!(
        decode_snapshot_payload_typed(&malformed),
        DecodedSnapshotPayload::Malformed
    );
}

/// A `count` exactly at `MAX_SEGMENTS` with a genuinely complete table
/// still decodes successfully — the bound rejects EXCESS, not the
/// legitimate ceiling itself.
#[test]
fn decode_snapshot_payload_accepts_a_count_exactly_at_max_segments() {
    // Every entry at offset 0 (all describing the SAME content byte,
    // legitimate per the coalescing rule the daemon-side recorder
    // applies) — this test is about the COUNT ceiling, not offsets, so
    // the fixture must satisfy D2''' round-6's offset validation
    // (leading zero / non-decreasing / within content) trivially.
    let segments: Vec<DimSegment> = (0..MAX_SEGMENTS)
        .map(|_| DimSegment {
            offset: 0,
            cols: 80,
            rows: 24,
        })
        .collect();
    let encoded = encode_snapshot_payload(&segments, b"content");
    match decode_snapshot_payload_typed(&encoded) {
        DecodedSnapshotPayload::Structured {
            segments: decoded, ..
        } => assert_eq!(decoded.len(), MAX_SEGMENTS),
        other => panic!("expected Structured, got {other:?}"),
    }
}

// ── D2''' (round-6 rework, review round-5 finding
// `58db33c799bedf87`): segment OFFSETS are validated, not just
// dimensions and count. Each test below builds a well-formed table by
// `count`/length rules but violates exactly one offset rule, and
// confirms `decode_snapshot_payload_typed` rejects it as `Malformed`
// rather than silently producing a segment list `TerminalCore::
// replay_segments` would drop content against.

/// AC-3 (round-8 rework, review round-7 finding `01f91fe698ceb287`): a
/// non-zero LEADING offset is now ACCEPTED, not rejected — it is the
/// shape `ScrollbackRingBuffer::read_segments` legitimately produces
/// when 2+ `dim_markers` entries have been evicted by the cap
/// (D1'''''), leaving the leading span deliberately unattributed.
/// `TerminalCore::replay_segments` was fixed in lockstep to replay that
/// leading span at the caller's target dims rather than drop it, so
/// this is no longer a malformed-envelope condition.
///
/// Confirmed to fail pre-fix: before this fix, the decoder rejected any
/// non-zero leading offset as `Malformed` — the assertion below
/// (expecting `Structured`) would instead see `Malformed`.
#[test]
fn decode_snapshot_payload_typed_accepts_a_non_zero_leading_offset() {
    let segments = vec![DimSegment {
        offset: 5,
        cols: 80,
        rows: 24,
    }];
    let encoded = encode_snapshot_payload(&segments, b"0123456789");
    match decode_snapshot_payload_typed(&encoded) {
        DecodedSnapshotPayload::Structured {
            segments: decoded, ..
        } => assert_eq!(decoded, segments),
        other => panic!("expected Structured, got {other:?}"),
    }
}

/// A non-monotonic (decreasing) offset would make `replay_segments`
/// compute an `end < start` range for the offending segment, silently
/// dropping the bytes in between — rejected as `Malformed`.
#[test]
fn decode_snapshot_payload_typed_rejects_non_monotonic_offset() {
    let segments = vec![
        DimSegment {
            offset: 0,
            cols: 80,
            rows: 24,
        },
        DimSegment {
            offset: 8,
            cols: 100,
            rows: 30,
        },
        DimSegment {
            offset: 4, // goes BACKWARD relative to the previous entry
            cols: 120,
            rows: 40,
        },
    ];
    let encoded = encode_snapshot_payload(&segments, b"0123456789");
    assert_eq!(
        decode_snapshot_payload_typed(&encoded),
        DecodedSnapshotPayload::Malformed
    );
}

/// An offset past the end of `content` can never be reached by any
/// segment's byte range — rejected as `Malformed` rather than silently
/// producing a segment `replay_segments` would clamp to `bytes.len()`
/// (making the segment's declared start meaningless).
#[test]
fn decode_snapshot_payload_typed_rejects_offset_past_content_length() {
    let segments = vec![DimSegment {
        offset: 0,
        cols: 80,
        rows: 24,
    }];
    let content = b"short";
    let mut encoded = encode_snapshot_payload(&segments, content);
    // Hand-corrupt the SECOND segment's offset (appended below,
    // bypassing `encode_snapshot_payload`'s single-segment table) so
    // the table declares an offset beyond `content`'s true length
    // without extending `content` to match.
    encoded[SNAPSHOT_PAYLOAD_MAGIC.len()..SNAPSHOT_PAYLOAD_MAGIC.len() + 4]
        .copy_from_slice(&2u32.to_le_bytes());
    let mut with_second = encoded[..SNAPSHOT_PAYLOAD_MAGIC.len() + 4 + 8].to_vec();
    with_second.extend_from_slice(&(content.len() as u32 + 100).to_le_bytes()); // offset
    with_second.extend_from_slice(&100u16.to_le_bytes()); // cols
    with_second.extend_from_slice(&30u16.to_le_bytes()); // rows
    with_second.extend_from_slice(content);
    assert_eq!(
        decode_snapshot_payload_typed(&with_second),
        DecodedSnapshotPayload::Malformed
    );
}

/// A segment's offset EQUAL to `content.len()` (a zero-length trailing
/// segment — the shape a real head-marker-only snapshot with no
/// further content yet would produce) is the valid boundary, not an
/// off-by-one Malformed rejection.
#[test]
fn decode_snapshot_payload_typed_accepts_offset_equal_to_content_length() {
    let segments = vec![DimSegment {
        offset: 0,
        cols: 80,
        rows: 24,
    }];
    let content = b"exact";
    let mut encoded = encode_snapshot_payload(&segments, content);
    encoded[SNAPSHOT_PAYLOAD_MAGIC.len()..SNAPSHOT_PAYLOAD_MAGIC.len() + 4]
        .copy_from_slice(&2u32.to_le_bytes());
    let mut with_second = encoded[..SNAPSHOT_PAYLOAD_MAGIC.len() + 4 + 8].to_vec();
    with_second.extend_from_slice(&(content.len() as u32).to_le_bytes());
    with_second.extend_from_slice(&100u16.to_le_bytes());
    with_second.extend_from_slice(&30u16.to_le_bytes());
    with_second.extend_from_slice(content);
    match decode_snapshot_payload_typed(&with_second) {
        DecodedSnapshotPayload::Structured { segments, .. } => assert_eq!(segments.len(), 2),
        other => panic!("expected Structured, got {other:?}"),
    }
}

// ── D5''' (round-6 rework, review round-5 finding
// `1227fc04fb9368d0`): a segment's dimension PRODUCT is bounded, not
// just each dimension individually. ───────────────────────────────────

/// A segment at `RESIZE_MARKER_MAX_COLS` x `RESIZE_MARKER_MAX_ROWS`
/// (4096 x 4096, each individually within term_core's per-dimension
/// clamp) still exceeds `MAX_SEGMENT_CELLS` and must be rejected before
/// any allocation happens downstream.
#[test]
fn decode_snapshot_payload_typed_rejects_segment_exceeding_dimension_budget() {
    let segments = vec![DimSegment {
        offset: 0,
        cols: 4096,
        rows: 4096,
    }];
    assert!(
        (segments[0].cols as u32) * (segments[0].rows as u32) > MAX_SEGMENT_CELLS,
        "test prerequisite: fixture must actually exceed the budget"
    );
    let encoded = encode_snapshot_payload(&segments, b"content");
    assert_eq!(
        decode_snapshot_payload_typed(&encoded),
        DecodedSnapshotPayload::Malformed
    );
}

/// A segment whose product is exactly at `MAX_SEGMENT_CELLS` decodes
/// successfully — the budget rejects EXCESS, not the boundary itself.
#[test]
fn decode_snapshot_payload_typed_accepts_segment_at_dimension_budget() {
    // 1000 x 1000 = 1_000_000 == MAX_SEGMENT_CELLS.
    let segments = vec![DimSegment {
        offset: 0,
        cols: 1000,
        rows: 1000,
    }];
    assert_eq!(
        (segments[0].cols as u32) * (segments[0].rows as u32),
        MAX_SEGMENT_CELLS
    );
    let encoded = encode_snapshot_payload(&segments, b"content");
    match decode_snapshot_payload_typed(&encoded) {
        DecodedSnapshotPayload::Structured {
            segments: decoded, ..
        } => {
            assert_eq!(decoded, segments)
        }
        other => panic!("expected Structured, got {other:?}"),
    }
}

// ── D2'''' (round-7 rework, review round-6 finding
// `02bb52aaff9638e5`): a CUMULATIVE budget across all segments, not
// just a per-segment one ────────────────────────────────────────────

/// The per-segment budget bounds ONE segment but not their SUM — a
/// payload with `MAX_CUMULATIVE_SEGMENT_CELLS / MAX_SEGMENT_CELLS + 1`
/// segments, each individually AT the per-segment ceiling (valid on
/// its own), sums to MORE than `MAX_CUMULATIVE_SEGMENT_CELLS` and must
/// be rejected as Malformed before any per-segment allocation happens.
///
/// Confirmed to fail pre-fix: without the cumulative running-total
/// check, every segment individually passes the `MAX_SEGMENT_CELLS`
/// check (exactly at the ceiling, never over it), so decode would have
/// returned `Structured` with all of them.
#[test]
fn decode_snapshot_payload_typed_rejects_cumulative_cost_over_budget() {
    let segment_count = (MAX_CUMULATIVE_SEGMENT_CELLS / MAX_SEGMENT_CELLS as u64) as usize + 1;
    let segments: Vec<DimSegment> = (0..segment_count)
        .map(|_| DimSegment {
            offset: 0,
            cols: 1000,
            rows: 1000,
        })
        .collect();
    assert_eq!(
        (segments[0].cols as u32) * (segments[0].rows as u32),
        MAX_SEGMENT_CELLS,
        "test prerequisite: each segment individually at the \
         per-segment ceiling"
    );
    let total: u64 = segments
        .iter()
        .map(|s| (s.cols as u64) * (s.rows as u64))
        .sum();
    assert!(
        total > MAX_CUMULATIVE_SEGMENT_CELLS,
        "test prerequisite: fixture must actually exceed the \
         cumulative budget"
    );
    let encoded = encode_snapshot_payload(&segments, b"");
    assert_eq!(
        decode_snapshot_payload_typed(&encoded),
        DecodedSnapshotPayload::Malformed
    );
}

/// The cumulative budget rejects EXCESS, not the boundary itself: a
/// payload whose segments sum to EXACTLY `MAX_CUMULATIVE_SEGMENT_CELLS`
/// still decodes.
#[test]
fn decode_snapshot_payload_typed_accepts_cumulative_cost_at_budget() {
    let segment_count = (MAX_CUMULATIVE_SEGMENT_CELLS / MAX_SEGMENT_CELLS as u64) as usize;
    let segments: Vec<DimSegment> = (0..segment_count)
        .map(|_| DimSegment {
            offset: 0,
            cols: 1000,
            rows: 1000,
        })
        .collect();
    let total: u64 = segments
        .iter()
        .map(|s| (s.cols as u64) * (s.rows as u64))
        .sum();
    assert_eq!(total, MAX_CUMULATIVE_SEGMENT_CELLS);
    let encoded = encode_snapshot_payload(&segments, b"");
    match decode_snapshot_payload_typed(&encoded) {
        DecodedSnapshotPayload::Structured {
            segments: decoded, ..
        } => {
            assert_eq!(decoded.len(), segment_count)
        }
        other => panic!("expected Structured, got {other:?}"),
    }
}

/// D2'' checked-arithmetic path: a `count` near `u32::MAX` must be
/// rejected via the `MAX_SEGMENTS` ceiling (and, defensively, the
/// `checked_mul` overflow guard) rather than attempting
/// `count * 8`, which would overflow `usize` on a 32-bit target and — if
/// wrapping were used instead of checked arithmetic — could wrap back
/// into a small, spuriously "valid" table length.
#[test]
fn decode_snapshot_payload_rejects_count_near_u32_max_without_overflow_panic() {
    let mut malformed = Vec::new();
    malformed.extend_from_slice(&SNAPSHOT_PAYLOAD_MAGIC);
    malformed.extend_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        decode_snapshot_payload_typed(&malformed),
        DecodedSnapshotPayload::Malformed
    );
}

/// D2'': `Legacy` (no magic prefix) and `Malformed` (magic prefix but
/// corrupt table) are OBSERVABLY different outcomes, not merely two
/// names for the same tuple shape — the whole point of the typed API.
#[test]
fn decode_snapshot_payload_typed_distinguishes_legacy_from_malformed() {
    let legacy = b"\x1b[3J\x1b[H\x1b[2Jplain ansi, no magic".to_vec();
    assert_eq!(
        decode_snapshot_payload_typed(&legacy),
        DecodedSnapshotPayload::Legacy(&legacy)
    );

    let mut malformed = Vec::new();
    malformed.extend_from_slice(&SNAPSHOT_PAYLOAD_MAGIC);
    malformed.extend_from_slice(&1u32.to_le_bytes());
    // Declares 1 segment (needs 8 bytes) but supplies none.
    assert_eq!(
        decode_snapshot_payload_typed(&malformed),
        DecodedSnapshotPayload::Malformed
    );
}

#[test]
fn max_snapshot_frame_payload_matches_frame_length_minus_header() {
    assert_eq!(
        MAX_SNAPSHOT_FRAME_PAYLOAD,
        MAX_FRAME_LENGTH - FRAME_HEADER_LEN
    );
    assert_eq!(FRAME_HEADER_LEN, 5);
}

/// D6'' (task0005 rework, review round-4 finding `1d4a0c96821da0ef`):
/// the shared size-policy check accepts exactly up to the limit and
/// rejects one byte past it.
#[test]
fn fits_single_snapshot_frame_boundary() {
    assert!(fits_single_snapshot_frame(0));
    assert!(fits_single_snapshot_frame(MAX_SNAPSHOT_FRAME_PAYLOAD));
    assert!(!fits_single_snapshot_frame(MAX_SNAPSHOT_FRAME_PAYLOAD + 1));
}
