//! Core agent-status types, wire grammar, parsing, and name sanitization
//! (SPEC.md FR1).
//!
//! Build-agnostic: depends only on `std`, so it compiles in both the GUI
//! and CLI-only (`--no-default-features`) builds. Every consumer (the
//! `emterm agent-status` CLI, the mux daemon, the plain-tab GUI OSC path)
//! goes through [`parse`] / [`build_set`] / [`build_clear`] so there is
//! exactly one implementation of the wire grammar.

use std::collections::HashSet;

/// The four states an agent-status report may set (SPEC FR1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentState {
    Idle,
    Working,
    Blocked,
    Done,
}

impl AgentState {
    /// The wire string for this state (`idle|working|blocked|done`).
    pub fn as_str(self) -> &'static str {
        match self {
            AgentState::Idle => "idle",
            AgentState::Working => "working",
            AgentState::Blocked => "blocked",
            AgentState::Done => "done",
        }
    }

    /// Parse a wire string into a state. Unknown values return `None`.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "idle" => Some(Self::Idle),
            "working" => Some(Self::Working),
            "blocked" => Some(Self::Blocked),
            "done" => Some(Self::Done),
            _ => None,
        }
    }
}

/// A validated agent-status report (FR1): either sets state (with an
/// optional sanitized name) or clears the pane's reported status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentStatusEvent {
    Set {
        state: AgentState,
        name: Option<String>,
    },
    Clear,
}

/// Maximum decoded name length, in characters (not bytes) — FR1.
pub const MAX_NAME_LEN: usize = 80;

/// The OSC 777 namespace + kind prefix this module owns. `parse`'s input
/// contract is the full post-namespace OSC body with this prefix attached
/// (the same slice convention every OSC 777 kind dispatcher uses elsewhere
/// in the codebase, e.g. `callbacks.rs`'s `emterm;<kind>;…` handling).
const PREFIX: &str = "emterm;agent-status;";

/// Percent-decode `value` per RFC 3986 `%XX` escapes. Returns `None` on a
/// malformed escape (truncated at the end of the string, or non-hex
/// digits) or invalid UTF-8 after decoding, so the caller can reject the
/// whole sequence (FR1: "a failed decode invalidates the whole sequence").
fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hi = *bytes.get(i + 1)?;
                let lo = *bytes.get(i + 2)?;
                let hi = (hi as char).to_digit(16)?;
                let lo = (lo as char).to_digit(16)?;
                out.push(((hi << 4) | lo) as u8);
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

/// Percent-encode `s` for the `name=` field: escape every byte outside the
/// unreserved set (`A-Za-z0-9-_.~`) — including `;` and `%` themselves —
/// so an encoded value can never be split by the `;` field separator or
/// misparsed as a nested escape.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char);
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Sanitize a decoded name (FR1 / NFR1): strip control characters, then
/// truncate to [`MAX_NAME_LEN`] characters.
pub fn sanitize_name(decoded: &str) -> String {
    decoded
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME_LEN)
        .collect()
}

/// Parse an OSC 777 payload for the `agent-status` kind (FR1).
///
/// `payload` is the full post-namespace OSC body, e.g.
/// `"emterm;agent-status;v=1;state=working;name=claude"` or
/// `"emterm;agent-status;clear"`.
///
/// Whole-or-nothing: returns `None` on any invalid part — missing/unknown
/// `state`, a duplicate key, or a percent-decode failure — and the caller
/// must not mutate any state when this returns `None`. Unknown keys
/// (including `v`, whose value is not validated) are accepted and ignored.
pub fn parse(payload: &str) -> Option<AgentStatusEvent> {
    let rest = payload.strip_prefix(PREFIX)?;
    if rest == "clear" {
        return Some(AgentStatusEvent::Clear);
    }

    let mut seen_keys: HashSet<&str> = HashSet::new();
    let mut state: Option<AgentState> = None;
    let mut name: Option<String> = None;

    for field in rest.split(';') {
        let (key, value) = field.split_once('=')?;
        if !seen_keys.insert(key) {
            return None; // duplicate key invalidates the whole sequence
        }
        match key {
            "state" => state = Some(AgentState::parse(value)?),
            "name" => {
                let decoded = percent_decode(value)?;
                name = Some(sanitize_name(&decoded));
            }
            _ => {
                // "v" and any future/unknown key: accepted, not validated.
            }
        }
    }

    let state = state?;
    Some(AgentStatusEvent::Set { state, name })
}

/// Build the exact FR1 wire body for a Set report (OSC 777 payload, with
/// the `emterm;` namespace attached) — always includes `v=1`.
pub fn build_set(state: AgentState, name: Option<&str>) -> String {
    let mut out = format!("{PREFIX}v=1;state={}", state.as_str());
    if let Some(n) = name {
        out.push_str(";name=");
        out.push_str(&percent_encode(n));
    }
    out
}

/// Build the exact FR1 wire body for a Clear report.
pub fn build_clear() -> String {
    format!("{PREFIX}clear")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── AgentState::as_str / parse round trip ───────────────────────────

    #[test]
    fn agent_state_wire_strings_round_trip() {
        for s in [
            AgentState::Idle,
            AgentState::Working,
            AgentState::Blocked,
            AgentState::Done,
        ] {
            assert_eq!(AgentState::parse(s.as_str()), Some(s));
        }
        assert_eq!(AgentState::parse("bogus"), None);
    }

    // ── parse: accepted forms ────────────────────────────────────────────

    #[test]
    fn parse_accepts_all_states_without_name() {
        for (wire, expected) in [
            ("idle", AgentState::Idle),
            ("working", AgentState::Working),
            ("blocked", AgentState::Blocked),
            ("done", AgentState::Done),
        ] {
            let payload = format!("emterm;agent-status;v=1;state={wire}");
            assert_eq!(
                parse(&payload),
                Some(AgentStatusEvent::Set {
                    state: expected,
                    name: None
                })
            );
        }
    }

    #[test]
    fn parse_accepts_state_with_name() {
        let payload = "emterm;agent-status;v=1;state=working;name=claude";
        assert_eq!(
            parse(payload),
            Some(AgentStatusEvent::Set {
                state: AgentState::Working,
                name: Some("claude".to_string()),
            })
        );
    }

    #[test]
    fn parse_accepts_clear() {
        let payload = "emterm;agent-status;clear";
        assert_eq!(parse(payload), Some(AgentStatusEvent::Clear));
    }

    #[test]
    fn parse_ignores_unknown_keys() {
        let payload = "emterm;agent-status;v=1;state=idle;future=xyz";
        assert_eq!(
            parse(payload),
            Some(AgentStatusEvent::Set {
                state: AgentState::Idle,
                name: None
            })
        );
    }

    // ── parse: rejected forms (whole-sequence rejection) ─────────────────

    #[test]
    fn parse_rejects_missing_state() {
        let payload = "emterm;agent-status;v=1;name=claude";
        assert_eq!(parse(payload), None);
    }

    #[test]
    fn parse_rejects_unknown_state_value() {
        let payload = "emterm;agent-status;v=1;state=sleeping";
        assert_eq!(parse(payload), None);
    }

    #[test]
    fn parse_rejects_duplicate_keys() {
        let payload = "emterm;agent-status;v=1;state=idle;state=working";
        assert_eq!(parse(payload), None);
        let payload_name = "emterm;agent-status;v=1;state=idle;name=a;name=b";
        assert_eq!(parse(payload_name), None);
    }

    #[test]
    fn parse_rejects_bad_percent_encoding() {
        // truncated escape
        assert_eq!(parse("emterm;agent-status;v=1;state=idle;name=100%"), None);
        // non-hex digits
        assert_eq!(parse("emterm;agent-status;v=1;state=idle;name=%ZZ"), None);
    }

    #[test]
    fn parse_rejects_wrong_namespace_or_kind() {
        assert_eq!(parse("emterm;markdown;begin"), None);
        assert_eq!(parse("agent-status;v=1;state=idle"), None);
        assert_eq!(parse(""), None);
    }

    // ── name sanitization ────────────────────────────────────────────────

    #[test]
    fn parse_decodes_and_sanitizes_name() {
        let payload = "emterm;agent-status;v=1;state=working;name=Claude%20Code";
        assert_eq!(
            parse(payload),
            Some(AgentStatusEvent::Set {
                state: AgentState::Working,
                name: Some("Claude Code".to_string()),
            })
        );
    }

    #[test]
    fn parse_strips_control_characters_from_name() {
        let payload = "emterm;agent-status;v=1;state=working;name=bad%01name%1b";
        match parse(payload) {
            Some(AgentStatusEvent::Set { name, .. }) => {
                assert_eq!(name.as_deref(), Some("badname"));
            }
            other => panic!("expected Set, got {other:?}"),
        }
    }

    #[test]
    fn parse_truncates_name_to_80_chars() {
        let long_name = "a".repeat(200);
        let payload = format!("emterm;agent-status;v=1;state=working;name={long_name}");
        match parse(&payload) {
            Some(AgentStatusEvent::Set { name, .. }) => {
                let name = name.unwrap();
                assert_eq!(name.chars().count(), MAX_NAME_LEN);
            }
            other => panic!("expected Set, got {other:?}"),
        }
    }

    // ── build: exact wire strings + round trip ───────────────────────────

    #[test]
    fn build_set_includes_v1() {
        let out = build_set(AgentState::Blocked, None);
        assert_eq!(out, "emterm;agent-status;v=1;state=blocked");
    }

    #[test]
    fn build_set_with_name_percent_encodes() {
        let out = build_set(AgentState::Working, Some("Claude Code"));
        assert_eq!(
            out,
            "emterm;agent-status;v=1;state=working;name=Claude%20Code"
        );
    }

    #[test]
    fn build_clear_wire_string() {
        assert_eq!(build_clear(), "emterm;agent-status;clear");
    }

    #[test]
    fn build_output_round_trips_through_parse_for_every_state() {
        for state in [
            AgentState::Idle,
            AgentState::Working,
            AgentState::Blocked,
            AgentState::Done,
        ] {
            let no_name = build_set(state, None);
            assert_eq!(
                parse(&no_name),
                Some(AgentStatusEvent::Set { state, name: None })
            );

            let with_name = build_set(state, Some("agent; name=with/special%chars"));
            assert_eq!(
                parse(&with_name),
                Some(AgentStatusEvent::Set {
                    state,
                    name: Some("agent; name=with/special%chars".to_string()),
                })
            );
        }
    }

    #[test]
    fn build_clear_round_trips_through_parse() {
        assert_eq!(parse(&build_clear()), Some(AgentStatusEvent::Clear));
    }
}
