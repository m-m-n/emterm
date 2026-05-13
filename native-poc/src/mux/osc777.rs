//! `OSC 777 ; emterm ; mux ; …` parser.
//!
//! `term_core` maps the wire OSC 777 number to `action_type = 100` and
//! delivers the raw payload (everything after the `]777;`) to the registered
//! `TerminalCallbacks::on_osc`. native-poc's `callbacks.rs` already buffers
//! emterm-extension payloads on `NativeCallbackState::osc_queue`; Phase 4-C
//! adds the **mux** sub-protocol here:
//!
//! ```text
//! OSC 777 ; emterm ; mux ; attach ; <socket-path> ; <session-id> ST
//! OSC 777 ; emterm ; mux ; detach                                ST
//! ```
//!
//! The first two semicolon-separated tokens (`emterm`, `mux`) form the
//! prefix that distinguishes this from other OSC 777 dialects. The third
//! token is the action verb (`attach` / `detach`); `attach` requires two
//! more tokens for the socket path and session ID.
//!
//! Validation rules (defense in depth — the daemon also validates):
//!
//! - Socket path must be absolute and live under
//!   `/tmp/emterm-mux/` **or** `$XDG_RUNTIME_DIR/emterm-mux/`. Any other
//!   prefix → `Err(MuxOscError::InvalidSocketPath)`.
//! - Session ID must match `^[A-Za-z0-9_-]{1,64}$`. Any other shape →
//!   `Err(MuxOscError::InvalidSessionId)`.
//! - Unknown action verbs (anything other than `attach` / `detach`) →
//!   `Err(MuxOscError::UnknownAction)`.
//! - Wrong arity (e.g. `attach` without a socket path) →
//!   `Err(MuxOscError::Malformed)`.

use std::path::PathBuf;

/// A parsed mux-related OSC 777 action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MuxOscAction {
    /// `attach ; <socket> ; <session_id>`. The GUI is asked to open a
    /// `UnixStream` to `socket` and identify itself with `session_id`.
    Attach { socket: PathBuf, session_id: String },
    /// `detach`. The GUI is asked to close any active mux connection on
    /// the tab that produced this OSC.
    Detach,
}

/// Why a mux OSC 777 payload was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MuxOscError {
    /// The payload did not start with the `emterm ; mux` prefix. The caller
    /// can ignore this silently; the OSC is targeted at another OSC 777
    /// dialect.
    NotMuxPrefix,
    /// The action verb after the prefix was not `attach` or `detach`.
    UnknownAction,
    /// The action verb was known but the parameter count was wrong (e.g.
    /// `attach` with no socket path).
    Malformed,
    /// The socket path did not match the allowed prefix list.
    InvalidSocketPath,
    /// The session ID did not match `^[A-Za-z0-9_-]{1,64}$`.
    InvalidSessionId,
}

impl std::fmt::Display for MuxOscError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotMuxPrefix => write!(f, "OSC 777 payload is not addressed to emterm mux"),
            Self::UnknownAction => write!(f, "OSC 777 emterm mux: unknown action verb"),
            Self::Malformed => write!(f, "OSC 777 emterm mux: malformed payload"),
            Self::InvalidSocketPath => write!(f, "OSC 777 emterm mux: invalid socket path"),
            Self::InvalidSessionId => write!(f, "OSC 777 emterm mux: invalid session id"),
        }
    }
}

impl std::error::Error for MuxOscError {}

/// Parse the payload of an OSC 777 sequence into a [`MuxOscAction`].
///
/// `payload` is the text **after** `]777;` and **before** the string
/// terminator (`ST` / `BEL`). The `term_core` OSC handler strips those
/// surroundings for us.
pub fn parse(payload: &str) -> Result<MuxOscAction, MuxOscError> {
    parse_with_runtime_dir(payload, std::env::var("XDG_RUNTIME_DIR").ok().as_deref())
}

/// Same as [`parse`] but the caller supplies the value of `XDG_RUNTIME_DIR`.
/// Used by unit tests so we don't have to mutate the process environment.
pub fn parse_with_runtime_dir(
    payload: &str,
    xdg_runtime_dir: Option<&str>,
) -> Result<MuxOscAction, MuxOscError> {
    let mut parts = payload.split(';');
    let p0 = parts.next().ok_or(MuxOscError::NotMuxPrefix)?;
    let p1 = parts.next().ok_or(MuxOscError::NotMuxPrefix)?;
    if p0.trim() != "emterm" || p1.trim() != "mux" {
        return Err(MuxOscError::NotMuxPrefix);
    }

    let action = parts.next().ok_or(MuxOscError::Malformed)?.trim();
    match action {
        "attach" => {
            let socket = parts.next().ok_or(MuxOscError::Malformed)?.trim();
            let session_id = parts.next().ok_or(MuxOscError::Malformed)?.trim();
            if parts.next().is_some() {
                // Extra tokens — refuse rather than silently accept.
                return Err(MuxOscError::Malformed);
            }
            if !is_allowed_socket_path(socket, xdg_runtime_dir) {
                return Err(MuxOscError::InvalidSocketPath);
            }
            if !is_valid_session_id(session_id) {
                return Err(MuxOscError::InvalidSessionId);
            }
            Ok(MuxOscAction::Attach {
                socket: PathBuf::from(socket),
                session_id: session_id.to_string(),
            })
        }
        "detach" => {
            if parts.next().is_some() {
                return Err(MuxOscError::Malformed);
            }
            Ok(MuxOscAction::Detach)
        }
        _ => Err(MuxOscError::UnknownAction),
    }
}

/// A socket path is allowed if it starts with `/tmp/emterm-mux/` or, when
/// `XDG_RUNTIME_DIR` is set, with `<XDG_RUNTIME_DIR>/emterm-mux/`. The path
/// must be absolute and may not contain `..` segments to defend against
/// traversal.
fn is_allowed_socket_path(path: &str, xdg_runtime_dir: Option<&str>) -> bool {
    if path.is_empty() {
        return false;
    }
    if !path.starts_with('/') {
        return false;
    }
    // Reject ".." segments to keep callers from breaking out of the
    // emterm-mux/ subdirectory.
    for seg in path.split('/') {
        if seg == ".." {
            return false;
        }
    }
    if path.starts_with("/tmp/emterm-mux/") {
        return true;
    }
    if let Some(base) = xdg_runtime_dir {
        // `XDG_RUNTIME_DIR` is sometimes set to a path without trailing `/`.
        // We accept both shapes.
        let prefix_with_slash = format!(
            "{}{}emterm-mux/",
            base,
            if base.ends_with('/') { "" } else { "/" }
        );
        if path.starts_with(&prefix_with_slash) {
            return true;
        }
    }
    false
}

/// Session IDs match `^[A-Za-z0-9_-]{1,64}$`.
fn is_valid_session_id(id: &str) -> bool {
    let len = id.len();
    if !(1..=64).contains(&len) {
        return false;
    }
    id.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── TS-osc777-1: happy-path attach / detach ──────────────────────────

    #[test]
    fn parses_attach_with_tmp_socket() {
        let res = parse_with_runtime_dir(
            "emterm;mux;attach;/tmp/emterm-mux/default.sock;session-1",
            None,
        );
        assert_eq!(
            res,
            Ok(MuxOscAction::Attach {
                socket: PathBuf::from("/tmp/emterm-mux/default.sock"),
                session_id: "session-1".to_string(),
            })
        );
    }

    #[test]
    fn parses_attach_with_xdg_runtime_dir() {
        let res = parse_with_runtime_dir(
            "emterm;mux;attach;/run/user/1000/emterm-mux/default.sock;abc_DEF-123",
            Some("/run/user/1000"),
        );
        assert!(matches!(res, Ok(MuxOscAction::Attach { .. })));
    }

    #[test]
    fn parses_attach_with_xdg_runtime_dir_trailing_slash() {
        let res = parse_with_runtime_dir(
            "emterm;mux;attach;/run/user/1000/emterm-mux/default.sock;abc",
            Some("/run/user/1000/"),
        );
        assert!(matches!(res, Ok(MuxOscAction::Attach { .. })));
    }

    #[test]
    fn parses_detach() {
        assert_eq!(
            parse_with_runtime_dir("emterm;mux;detach", None),
            Ok(MuxOscAction::Detach)
        );
    }

    #[test]
    fn ignores_whitespace_around_tokens() {
        let res =
            parse_with_runtime_dir("emterm ; mux ; attach ; /tmp/emterm-mux/x.sock ; sid", None);
        assert!(matches!(res, Ok(MuxOscAction::Attach { .. })));
    }

    // ── TS-osc777-2: validation rejections ───────────────────────────────

    #[test]
    fn rejects_non_mux_prefix() {
        // Some other OSC 777 dialect — caller should silently fall through.
        let res = parse_with_runtime_dir("emterm;notify;attach;x;y", None);
        assert_eq!(res, Err(MuxOscError::NotMuxPrefix));
        let res = parse_with_runtime_dir("vendor-x;mux;attach;x;y", None);
        assert_eq!(res, Err(MuxOscError::NotMuxPrefix));
        // Empty payload.
        let res = parse_with_runtime_dir("", None);
        assert_eq!(res, Err(MuxOscError::NotMuxPrefix));
    }

    #[test]
    fn rejects_unknown_action() {
        let res = parse_with_runtime_dir("emterm;mux;reload;/tmp/emterm-mux/x;sid", None);
        assert_eq!(res, Err(MuxOscError::UnknownAction));
    }

    #[test]
    fn rejects_malformed_arity() {
        // attach without session_id.
        let res = parse_with_runtime_dir("emterm;mux;attach;/tmp/emterm-mux/x.sock", None);
        assert_eq!(res, Err(MuxOscError::Malformed));
        // attach with trailing junk.
        let res =
            parse_with_runtime_dir("emterm;mux;attach;/tmp/emterm-mux/x.sock;sid;extra", None);
        assert_eq!(res, Err(MuxOscError::Malformed));
        // detach with trailing junk.
        let res = parse_with_runtime_dir("emterm;mux;detach;extra", None);
        assert_eq!(res, Err(MuxOscError::Malformed));
        // Missing action verb.
        let res = parse_with_runtime_dir("emterm;mux", None);
        assert_eq!(res, Err(MuxOscError::Malformed));
    }

    #[test]
    fn rejects_invalid_socket_path() {
        // Not under /tmp/emterm-mux nor XDG_RUNTIME_DIR/emterm-mux.
        let res =
            parse_with_runtime_dir("emterm;mux;attach;/etc/passwd;sid", Some("/run/user/1000"));
        assert_eq!(res, Err(MuxOscError::InvalidSocketPath));
        // Relative path.
        let res = parse_with_runtime_dir(
            "emterm;mux;attach;relative/path;sid",
            Some("/run/user/1000"),
        );
        assert_eq!(res, Err(MuxOscError::InvalidSocketPath));
        // `..` traversal under the allowed prefix.
        let res = parse_with_runtime_dir("emterm;mux;attach;/tmp/emterm-mux/../passwd;sid", None);
        assert_eq!(res, Err(MuxOscError::InvalidSocketPath));
        // Empty socket.
        let res = parse_with_runtime_dir("emterm;mux;attach;;sid", None);
        assert_eq!(res, Err(MuxOscError::InvalidSocketPath));
    }

    #[test]
    fn rejects_invalid_session_id() {
        // Empty session id.
        let res = parse_with_runtime_dir("emterm;mux;attach;/tmp/emterm-mux/x.sock;", None);
        assert_eq!(res, Err(MuxOscError::InvalidSessionId));
        // Contains a forbidden character (slash).
        let res = parse_with_runtime_dir("emterm;mux;attach;/tmp/emterm-mux/x.sock;a/b", None);
        assert_eq!(res, Err(MuxOscError::InvalidSessionId));
        // Contains a space.
        let res = parse_with_runtime_dir("emterm;mux;attach;/tmp/emterm-mux/x.sock;a b", None);
        assert_eq!(res, Err(MuxOscError::InvalidSessionId));
        // Length > 64.
        let long = "a".repeat(65);
        let payload = format!("emterm;mux;attach;/tmp/emterm-mux/x.sock;{long}");
        let res = parse_with_runtime_dir(&payload, None);
        assert_eq!(res, Err(MuxOscError::InvalidSessionId));
    }

    // ── TS-osc777-3: edge cases ──────────────────────────────────────────

    #[test]
    fn accepts_max_length_session_id() {
        let sixtyfour = "a".repeat(64);
        let payload = format!("emterm;mux;attach;/tmp/emterm-mux/x.sock;{sixtyfour}");
        let res = parse_with_runtime_dir(&payload, None);
        assert!(matches!(res, Ok(MuxOscAction::Attach { .. })));
    }

    #[test]
    fn accepts_session_id_with_underscore_and_dash() {
        let res =
            parse_with_runtime_dir("emterm;mux;attach;/tmp/emterm-mux/x.sock;a_b-c-D_E", None);
        assert!(matches!(res, Ok(MuxOscAction::Attach { .. })));
    }

    #[test]
    fn xdg_path_without_xdg_env_is_rejected() {
        // If XDG_RUNTIME_DIR is unset, a path that looks like one must still
        // be rejected.
        let res = parse_with_runtime_dir(
            "emterm;mux;attach;/run/user/1000/emterm-mux/x.sock;sid",
            None,
        );
        assert_eq!(res, Err(MuxOscError::InvalidSocketPath));
    }
}
