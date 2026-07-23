//! Build-agnostic agent-status core: types, OSC 777 wire grammar, parsing,
//! and name sanitization (SPEC FR1 / NFR1).
//!
//! Compiled WITHOUT the `gui` feature (CLI-shared): every other consumer
//! (the `emterm agent-status` CLI subcommand, the mux daemon, and the GUI)
//! depends on this module; it depends on nothing feature-gated.
//!
//! Wire grammar (the OSC 777 payload — what `term_core` delivers to
//! `on_osc` callbacks once it has stripped the `ESC ] 777 ;` introducer
//! and `ESC \` terminator):
//! - Set:   `emterm;agent-status;v=1;state=<s>[;name=<pct-encoded>]`
//! - Clear: `emterm;agent-status;clear`
//!
//! Parsing is whole-or-nothing: any invalid part rejects the entire
//! sequence ([`parse`] returns `None`), leaving nothing for the caller to
//! apply.

use std::fmt;

/// The states an agent-status report may carry (SPEC FR1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentState {
    Idle,
    Working,
    Blocked,
    Done,
}

impl AgentState {
    /// All four states, for exhaustive test iteration.
    pub const ALL: [AgentState; 4] = [
        AgentState::Idle,
        AgentState::Working,
        AgentState::Blocked,
        AgentState::Done,
    ];

    fn as_wire(self) -> &'static str {
        match self {
            AgentState::Idle => "idle",
            AgentState::Working => "working",
            AgentState::Blocked => "blocked",
            AgentState::Done => "done",
        }
    }

    fn parse_wire(s: &str) -> Option<Self> {
        match s {
            "idle" => Some(AgentState::Idle),
            "working" => Some(AgentState::Working),
            "blocked" => Some(AgentState::Blocked),
            "done" => Some(AgentState::Done),
            _ => None,
        }
    }
}

impl fmt::Display for AgentState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_wire())
    }
}

/// A decoded agent-status report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentStatusEvent {
    Set {
        state: AgentState,
        name: Option<String>,
    },
    Clear,
}

/// Wire protocol version emitted by [`build_set_payload`] / [`build`].
const WIRE_VERSION: &str = "1";

/// Sanitized name length cap, in characters (SPEC NFR1).
const MAX_NAME_LEN: usize = 80;

/// The OSC 777 namespace + kind prefix every agent-status payload starts
/// with (after the `ESC ] 777 ;` introducer has already been stripped).
const PAYLOAD_PREFIX: &str = "emterm;agent-status;";

/// `ESC ] 777 ;` — the OSC 777 introducer.
const OSC_INTRODUCER: &str = "\x1b]777;";

/// `ESC \` — the string terminator (ST).
const ST: &str = "\x1b\\";

/// Parse an OSC 777 agent-status payload — the string starting
/// `emterm;agent-status;…` (no `ESC ] 777 ;` introducer / `ESC \`
/// terminator; those are already stripped by the caller, mirroring how
/// `term_core` delivers OSC 777 payloads to `on_osc`).
///
/// Returns the matching [`AgentStatusEvent`] on success. Rejects
/// (returns `None`, whole sequence, nothing partially applied) on:
/// - a payload that isn't ours (missing the `emterm;agent-status;` prefix)
/// - a `state` value that is missing or not one of
///   `idle`/`working`/`blocked`/`done`
/// - any key (`v`, `state`, `name`) repeated
/// - malformed percent-encoding in `name`
///
/// Keys other than `v`/`state`/`name` are ignored (forward compatible).
pub fn parse(payload: &str) -> Option<AgentStatusEvent> {
    let rest = payload.strip_prefix(PAYLOAD_PREFIX)?;

    if rest == "clear" {
        return Some(AgentStatusEvent::Clear);
    }

    let mut state: Option<AgentState> = None;
    let mut name: Option<String> = None;
    let mut seen_state = false;
    let mut seen_name = false;
    let mut seen_version = false;

    for token in rest.split(';') {
        if token.is_empty() {
            continue;
        }
        let (key, value) = token.split_once('=').unwrap_or((token, ""));
        match key {
            "state" => {
                if seen_state {
                    return None; // duplicate key
                }
                seen_state = true;
                state = Some(AgentState::parse_wire(value)?);
            }
            "name" => {
                if seen_name {
                    return None; // duplicate key
                }
                seen_name = true;
                let decoded = percent_decode(value)?;
                name = Some(sanitize_name(&decoded));
            }
            "v" => {
                if seen_version {
                    return None; // duplicate key
                }
                seen_version = true;
                // The value itself is not validated beyond presence —
                // unrecognized future versions are tolerated here.
            }
            _ => {
                // Unknown key: ignored, sequence still accepted.
            }
        }
    }

    let state = state?; // missing state -> whole-sequence rejection
    Some(AgentStatusEvent::Set { state, name })
}

/// Sanitize a decoded agent name (SPEC NFR1 postcondition): strip control
/// characters, then truncate to [`MAX_NAME_LEN`] characters.
fn sanitize_name(decoded: &str) -> String {
    decoded
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME_LEN)
        .collect()
}

/// Percent-decode a `%XX`-escaped string (RFC 3986 style — unlike
/// form-urlencoding, `+` is left as a literal `+`, never decoded to a
/// space). Returns `None` on a truncated/invalid `%` escape or on
/// invalid UTF-8 in the decoded byte sequence.
fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hi = (*bytes.get(i + 1)? as char).to_digit(16)?;
                let lo = (*bytes.get(i + 2)? as char).to_digit(16)?;
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// Percent-encode a raw name for embedding as a `name=` field value.
/// Unreserved ASCII (letters, digits, `-`, `_`, `.`, `~`) passes through
/// unchanged; everything else — including `;` (the field delimiter) and
/// multi-byte UTF-8 — is escaped as `%XX` per byte.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &byte in s.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Build the bare OSC 777 payload (no `ESC ] 777 ;` / `ESC \` framing)
/// for a `Set` report: `emterm;agent-status;v=1;state=<s>[;name=<pct>]`.
/// This is exactly what [`parse`] consumes.
pub fn build_set_payload(state: AgentState, name: Option<&str>) -> String {
    let mut payload = format!("{PAYLOAD_PREFIX}v={WIRE_VERSION};state={state}");
    if let Some(n) = name {
        payload.push_str(";name=");
        payload.push_str(&percent_encode(n));
    }
    payload
}

/// Build the bare OSC 777 payload for a `Clear` report:
/// `emterm;agent-status;clear`.
pub fn build_clear_payload() -> String {
    format!("{PAYLOAD_PREFIX}clear")
}

/// Build the bare OSC 777 payload for any [`AgentStatusEvent`].
pub fn build_payload(event: &AgentStatusEvent) -> String {
    match event {
        AgentStatusEvent::Set { state, name } => build_set_payload(*state, name.as_deref()),
        AgentStatusEvent::Clear => build_clear_payload(),
    }
}

/// Build the full OSC 777 escape sequence (`ESC ] 777 ; <payload> ESC \`)
/// for any [`AgentStatusEvent`] — what CLI / GUI writers emit to the
/// terminal (SPEC FR1).
pub fn build(event: &AgentStatusEvent) -> String {
    format!("{OSC_INTRODUCER}{}{ST}", build_payload(event))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── AC-1: accepts all four states, with/without name; clear ─────

    #[test]
    fn parses_all_states_without_name() {
        for state in AgentState::ALL {
            let payload = format!("emterm;agent-status;v=1;state={state}");
            assert_eq!(
                parse(&payload),
                Some(AgentStatusEvent::Set { state, name: None }),
                "state={state}"
            );
        }
    }

    #[test]
    fn parses_all_states_with_name() {
        for state in AgentState::ALL {
            let payload = format!("emterm;agent-status;v=1;state={state};name=claude");
            assert_eq!(
                parse(&payload),
                Some(AgentStatusEvent::Set {
                    state,
                    name: Some("claude".to_string())
                }),
                "state={state}"
            );
        }
    }

    #[test]
    fn parses_clear() {
        assert_eq!(
            parse("emterm;agent-status;clear"),
            Some(AgentStatusEvent::Clear)
        );
    }

    // ── AC-2: whole-sequence rejection ───────────────────────────────

    #[test]
    fn rejects_missing_state() {
        assert_eq!(parse("emterm;agent-status;v=1"), None);
    }

    #[test]
    fn rejects_unknown_state_value() {
        assert_eq!(parse("emterm;agent-status;v=1;state=sleeping"), None);
    }

    #[test]
    fn rejects_duplicate_state_key() {
        assert_eq!(
            parse("emterm;agent-status;v=1;state=idle;state=working"),
            None
        );
    }

    #[test]
    fn rejects_duplicate_name_key() {
        assert_eq!(
            parse("emterm;agent-status;v=1;state=idle;name=a;name=b"),
            None
        );
    }

    #[test]
    fn rejects_duplicate_version_key() {
        assert_eq!(parse("emterm;agent-status;v=1;v=1;state=idle"), None);
    }

    #[test]
    fn rejects_truncated_percent_escape() {
        assert_eq!(parse("emterm;agent-status;v=1;state=idle;name=abc%2"), None);
    }

    #[test]
    fn rejects_invalid_percent_hex_digits() {
        assert_eq!(
            parse("emterm;agent-status;v=1;state=idle;name=abc%zz"),
            None
        );
    }

    #[test]
    fn rejects_non_agent_status_payload() {
        assert_eq!(parse("emterm;markdown;begin;id=x"), None);
        assert_eq!(parse(""), None);
    }

    // ── AC-3: unknown keys ignored ───────────────────────────────────

    #[test]
    fn ignores_unknown_keys() {
        assert_eq!(
            parse("emterm;agent-status;v=1;future=xyz;state=working;another=1"),
            Some(AgentStatusEvent::Set {
                state: AgentState::Working,
                name: None
            })
        );
    }

    #[test]
    fn ignores_bare_token_without_equals() {
        assert_eq!(
            parse("emterm;agent-status;v=1;state=idle;bogus"),
            Some(AgentStatusEvent::Set {
                state: AgentState::Idle,
                name: None
            })
        );
    }

    // ── AC-4: name postcondition ─────────────────────────────────────

    #[test]
    fn name_is_percent_decoded() {
        // "hello world" percent-encoded.
        let payload = "emterm;agent-status;v=1;state=idle;name=hello%20world";
        assert_eq!(
            parse(payload),
            Some(AgentStatusEvent::Set {
                state: AgentState::Idle,
                name: Some("hello world".to_string())
            })
        );
    }

    #[test]
    fn name_control_characters_are_stripped() {
        // %1b = ESC.
        let payload = "emterm;agent-status;v=1;state=idle;name=a%1bb";
        assert_eq!(
            parse(payload),
            Some(AgentStatusEvent::Set {
                state: AgentState::Idle,
                name: Some("ab".to_string())
            })
        );
    }

    #[test]
    fn name_is_truncated_to_80_chars() {
        let long_name: String = "a".repeat(200);
        let payload = format!("emterm;agent-status;v=1;state=idle;name={long_name}");
        let Some(AgentStatusEvent::Set { name: Some(n), .. }) = parse(&payload) else {
            panic!("expected Set event with a name");
        };
        assert_eq!(n.chars().count(), MAX_NAME_LEN);
        assert_eq!(n, "a".repeat(MAX_NAME_LEN));
    }

    #[test]
    fn name_exactly_80_chars_is_not_truncated() {
        let name: String = "b".repeat(80);
        let payload = format!("emterm;agent-status;v=1;state=idle;name={name}");
        let Some(AgentStatusEvent::Set { name: Some(n), .. }) = parse(&payload) else {
            panic!("expected Set event with a name");
        };
        assert_eq!(n, name);
    }

    // ── AC-5: builder output round-trips ─────────────────────────────

    #[test]
    fn build_set_payload_round_trips_for_every_state() {
        for state in AgentState::ALL {
            let event = AgentStatusEvent::Set { state, name: None };
            assert_eq!(parse(&build_payload(&event)), Some(event));

            let named = AgentStatusEvent::Set {
                state,
                name: Some("claude-code".to_string()),
            };
            assert_eq!(parse(&build_payload(&named)), Some(named));
        }
    }

    #[test]
    fn build_clear_payload_round_trips() {
        let event = AgentStatusEvent::Clear;
        assert_eq!(parse(&build_payload(&event)), Some(event));
    }

    #[test]
    fn build_payload_round_trips_name_with_reserved_and_multibyte_chars() {
        let event = AgentStatusEvent::Set {
            state: AgentState::Blocked,
            name: Some("claude;日本語".to_string()),
        };
        assert_eq!(parse(&build_payload(&event)), Some(event));
    }

    // ── build() wire-format exactness ────────────────────────────────

    #[test]
    fn build_set_wire_format_includes_v1_and_state() {
        let event = AgentStatusEvent::Set {
            state: AgentState::Working,
            name: Some("claude".to_string()),
        };
        assert_eq!(
            build(&event),
            "\x1b]777;emterm;agent-status;v=1;state=working;name=claude\x1b\\"
        );
    }

    #[test]
    fn build_set_wire_format_without_name_omits_name_field() {
        let event = AgentStatusEvent::Set {
            state: AgentState::Idle,
            name: None,
        };
        assert_eq!(
            build(&event),
            "\x1b]777;emterm;agent-status;v=1;state=idle\x1b\\"
        );
    }

    #[test]
    fn build_clear_wire_format() {
        assert_eq!(
            build(&AgentStatusEvent::Clear),
            "\x1b]777;emterm;agent-status;clear\x1b\\"
        );
    }

    #[test]
    fn percent_encode_escapes_delimiter_and_control_chars() {
        let payload = build_set_payload(AgentState::Idle, Some("a;b\x1bc"));
        assert!(payload.contains("name=a%3Bb%1Bc"));
    }
}
