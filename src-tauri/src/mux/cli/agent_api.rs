//! Agent-API subcommands: `read` / `send` / `wait`, plus the shared
//! request / response plumbing and pane-argument parsing they use.

use super::*;

/// Default `--lines` for `emterm mux read` when omitted.
const READ_DEFAULT_LINES: u32 = 100;
/// Default `--timeout` (seconds) for `emterm mux wait` when omitted.
const WAIT_DEFAULT_TIMEOUT_SECS: u64 = 30;
/// Margin added to the client-side read timeout above the daemon's own
/// `timeout_ms` for `wait`, so the client never gives up before the daemon
/// replies with its own (bounded) timeout error.
const WAIT_CLIENT_TIMEOUT_MARGIN_MS: u64 = 5000;

/// Map an `AgentApiErrorKind` to the CLI exit-code convention
/// (IMPLEMENTATION.md "Conventions"). Pure — testable without a live daemon
/// (AC-7).
pub(in crate::mux::cli) fn agent_api_error_exit_code(kind: AgentApiErrorKind) -> i32 {
    match kind {
        AgentApiErrorKind::InvalidInput => 2,
        AgentApiErrorKind::Timeout => 3,
        AgentApiErrorKind::UnknownPane | AgentApiErrorKind::PaneGone => 4,
        AgentApiErrorKind::NotMuxPane => 5,
    }
}

/// Resolve a `--pane` argument: `"current"` resolves from `EMTERM_PANE_ID`
/// (a missing variable is a usage error per FR13); any other value passes
/// through verbatim as the public pane ID. Pure aside from the env read —
/// testable via `temp_env` (AC-8).
pub(in crate::mux::cli) fn resolve_pane_arg(pane_arg: &str) -> Result<String, String> {
    if pane_arg == "current" {
        std::env::var("EMTERM_PANE_ID")
            .map_err(|_| "EMTERM_PANE_ID is not set (required for --pane current)".to_string())
    } else {
        Ok(pane_arg.to_string())
    }
}

fn parse_agent_state_str(s: &str) -> Option<AgentState> {
    match s {
        "idle" => Some(AgentState::Idle),
        "working" => Some(AgentState::Working),
        "blocked" => Some(AgentState::Blocked),
        "done" => Some(AgentState::Done),
        _ => None,
    }
}

/// Parse a comma-separated `--state` value (e.g. `"done,blocked"`) into the
/// wire `Vec<AgentState>`. Pure — testable without a live daemon.
pub(in crate::mux::cli) fn parse_agent_states(s: &str) -> Result<Vec<AgentState>, String> {
    let mut states = Vec::new();
    for part in s.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err(format!("invalid --state value: {s:?}"));
        }
        match parse_agent_state_str(part) {
            Some(state) => states.push(state),
            None => {
                return Err(format!(
                    "unknown state {part:?} (expected idle|working|blocked|done)"
                ));
            }
        }
    }
    if states.is_empty() {
        return Err("`--state` requires at least one state".to_string());
    }
    Ok(states)
}

/// Send a bincode-encoded control request and read back one framed
/// response. Generic over the platform-specific stream type returned by
/// `cli_handshake()` (`UnixStream` / Windows `File`), both `Read + Write`.
fn send_agent_request<S, T>(
    stream: &mut S,
    msg_type: MessageType,
    payload: &T,
) -> Result<MuxMessage, Box<dyn std::error::Error>>
where
    S: std::io::Read + std::io::Write,
    T: Serialize,
{
    let msg = MuxMessage::control(msg_type, 0, payload);
    let body = msg.to_frame_body();
    let len = (body.len() as u32).to_be_bytes();
    stream.write_all(&len)?;
    stream.write_all(&body)?;
    stream.flush()?;

    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let frame_len = u32::from_be_bytes(len_buf) as usize;
    if frame_len > MAX_FRAME_LENGTH {
        return Err("Frame too large".into());
    }
    let mut frame_buf = vec![0u8; frame_len];
    stream.read_exact(&mut frame_buf)?;
    MuxMessage::from_frame_body(&frame_buf).ok_or_else(|| "Invalid frame".into())
}

/// Decode `resp` as either the expected success payload `T` (`on_success`,
/// exit 0) or a shared `AgentApiError` (mapped via
/// [`agent_api_error_exit_code`]). Any other response shape is an exit-1
/// protocol error.
fn handle_agent_response<T: for<'a> Deserialize<'a>>(
    resp: MuxMessage,
    expected: MessageType,
    on_success: impl FnOnce(T),
) -> i32 {
    if resp.msg_type == expected {
        match resp.decode_payload::<T>() {
            Some(v) => {
                on_success(v);
                0
            }
            None => {
                eprintln!("Error: malformed response payload");
                1
            }
        }
    } else if resp.msg_type == MessageType::AgentApiError {
        match resp.decode_payload::<AgentApiError>() {
            Some(err) => {
                eprintln!("Error: {}", err.message);
                agent_api_error_exit_code(err.kind)
            }
            None => {
                eprintln!("Error: malformed error response");
                1
            }
        }
    } else {
        eprintln!("Error: unexpected response type {:?}", resp.msg_type);
        1
    }
}

/// Execute `emterm mux read --pane <id|current> [--lines N]` (FR10).
pub fn execute_mux_read(pane_arg: &str, lines_arg: Option<&str>) -> i32 {
    let public_pane_id = match resolve_pane_arg(pane_arg) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("Error: {e}");
            return 2;
        }
    };
    let lines: u32 = match lines_arg {
        Some(s) => match s.parse() {
            Ok(n) => n,
            Err(_) => {
                eprintln!("Error: --lines must be a non-negative integer (got {s:?})");
                return 2;
            }
        },
        None => READ_DEFAULT_LINES,
    };

    let (mut stream, _sessions) = match cli_handshake() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };

    let req = ReadPaneMsg {
        public_pane_id,
        lines,
    };
    let resp = match send_agent_request(&mut stream, MessageType::ReadPane, &req) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };
    handle_agent_response::<ReadPaneResultMsg>(resp, MessageType::ReadPaneResult, |r| {
        println!("{}", r.text);
    })
}

/// Execute `emterm mux send --pane <id|current> (--text <s> | --stdin)`
/// (FR11). Argument mutual-exclusivity is validated by the caller (`run`).
pub fn execute_mux_send(pane_arg: &str, text_arg: Option<&str>, use_stdin: bool) -> i32 {
    let public_pane_id = match resolve_pane_arg(pane_arg) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("Error: {e}");
            return 2;
        }
    };

    let bytes: Vec<u8> = if use_stdin {
        use std::io::Read;
        let mut buf = Vec::new();
        if let Err(e) = std::io::stdin().read_to_end(&mut buf) {
            eprintln!("Error: failed to read stdin: {e}");
            return 1;
        }
        buf
    } else {
        // `run` guarantees `text_arg.is_some()` when `!use_stdin`.
        text_arg.unwrap_or_default().as_bytes().to_vec()
    };

    let (mut stream, _sessions) = match cli_handshake() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };

    let req = SendTextMsg {
        public_pane_id,
        bytes,
    };
    let resp = match send_agent_request(&mut stream, MessageType::SendText, &req) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };
    handle_agent_response::<SendTextResultMsg>(resp, MessageType::SendTextResult, |r| {
        println!("revision_watermark={}", r.revision_watermark);
    })
}

/// Execute `emterm mux wait --pane <id|current> --state <set> [--timeout
/// <sec>] [--after <revision>]` (FR12).
pub fn execute_mux_wait(
    pane_arg: &str,
    states_arg: &str,
    timeout_arg: Option<&str>,
    after_arg: Option<&str>,
) -> i32 {
    let public_pane_id = match resolve_pane_arg(pane_arg) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("Error: {e}");
            return 2;
        }
    };
    let states = match parse_agent_states(states_arg) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {e}");
            return 2;
        }
    };
    let timeout_secs: u64 = match timeout_arg {
        Some(s) => match s.parse() {
            Ok(n) => n,
            Err(_) => {
                eprintln!(
                    "Error: --timeout must be a non-negative integer number of seconds (got {s:?})"
                );
                return 2;
            }
        },
        None => WAIT_DEFAULT_TIMEOUT_SECS,
    };
    let after_revision: Option<u64> = match after_arg {
        Some(s) => match s.parse() {
            Ok(n) => Some(n),
            Err(_) => {
                eprintln!("Error: --after must be a non-negative integer revision (got {s:?})");
                return 2;
            }
        },
        None => None,
    };
    let timeout_ms = timeout_secs.saturating_mul(1000);

    let (mut stream, _sessions) = match cli_handshake() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };

    // Extend the client-side read timeout beyond the daemon's own wait
    // timeout so we don't give up before it replies (Unix only: the
    // Windows Named Pipe `File` handle has no read-timeout API, and blocks
    // indefinitely by default — which is already bounded by the daemon's
    // own `timeout_ms`).
    #[cfg(unix)]
    {
        if let Err(e) = stream.set_read_timeout(Some(std::time::Duration::from_millis(
            timeout_ms + WAIT_CLIENT_TIMEOUT_MARGIN_MS,
        ))) {
            eprintln!("Error: {e}");
            return 1;
        }
    }

    let req = WaitAgentStateMsg {
        public_pane_id,
        states,
        timeout_ms,
        after_revision,
    };
    let resp = match send_agent_request(&mut stream, MessageType::WaitAgentState, &req) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };
    handle_agent_response::<WaitAgentStateResultMsg>(resp, MessageType::WaitAgentStateResult, |r| {
        println!("state={:?} revision={}", r.state, r.revision);
    })
}
