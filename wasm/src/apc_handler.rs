/// APC handler: classifies Kitty graphics protocol APC payloads.
///
/// Response generation is handled by the PTY reader thread's KittyScanner
/// (in `src-tauri/src/pty/kitty_scanner.rs`) which writes OK responses
/// directly to the PTY master fd via libc::write(), bypassing all
/// intermediate layers (writer channel, WebView, Tauri IPC).
///
/// This module only classifies the APC payload for the terminal_core's
/// APC dispatch logic (deciding whether to forward to the frontend).
use crate::terminal_core::TerminalCore;

/// Result of handling a Kitty APC payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KittyApcResult {
    /// Not a Kitty Graphics Protocol APC — forward via APC callback.
    NotKitty,
    /// Query (`a=q`) — do NOT forward (no image processing needed).
    QueryHandled,
    /// Final chunk of a non-query action (m=0 or m absent) —
    /// forward via APC callback for image processing.
    FinalChunk,
    /// Continuation chunk (m=1) — forward via APC callback.
    MoreChunks,
}

impl TerminalCore {
    /// Classify an APC payload as Kitty Graphics Protocol.
    ///
    /// Returns the classification result used by the APC dispatch logic
    /// to decide whether to forward the APC to the frontend.
    ///
    /// NOTE: Response generation is NOT done here. The PTY reader thread's
    /// KittyScanner handles OK responses at the Rust level for minimal latency.
    pub(crate) fn handle_kitty_apc(&mut self, payload: &[u8]) -> KittyApcResult {
        // Must start with 'G' for Kitty Graphics Protocol
        if payload.first() != Some(&b'G') {
            return KittyApcResult::NotKitty;
        }

        let data = &payload[1..];

        // Find the control data portion (before ';' separator)
        let control_data = match data.iter().position(|&b| b == b';') {
            Some(pos) => &data[..pos],
            None => data,
        };

        // Parse key=value pairs
        let mut action: u8 = b't'; // default: transmit-and-display
        let mut more_chunks = false; // m=1

        for pair in control_data.split(|&b| b == b',') {
            if let Some(eq_pos) = pair.iter().position(|&b| b == b'=') {
                let key = &pair[..eq_pos];
                let val = &pair[eq_pos + 1..];
                match key {
                    b"a" => {
                        if let Some(&first) = val.first() {
                            action = first;
                        }
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

        let is_query = action == b'q';

        // Continuation chunk — just forward
        if more_chunks && !is_query {
            return KittyApcResult::MoreChunks;
        }

        if is_query {
            KittyApcResult::QueryHandled
        } else {
            KittyApcResult::FinalChunk
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::terminal_core::TerminalCore;

    use super::*;

    // ── Query (a=q) tests ────────────────────────────────

    #[test]
    fn test_kitty_query_with_id() {
        let mut core = TerminalCore::new(80, 24, 0);
        let payload = b"Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA";
        assert_eq!(core.handle_kitty_apc(payload), KittyApcResult::QueryHandled);
        // No response generated (handled by PTY reader thread)
        assert_eq!(core.response_len, 0);
    }

    #[test]
    fn test_kitty_query_without_id() {
        let mut core = TerminalCore::new(80, 24, 0);
        let payload = b"Ga=q;AAAA";
        assert_eq!(core.handle_kitty_apc(payload), KittyApcResult::QueryHandled);
        assert_eq!(core.response_len, 0);
    }

    #[test]
    fn test_kitty_query_no_payload() {
        let mut core = TerminalCore::new(80, 24, 0);
        let payload = b"Ga=q";
        assert_eq!(core.handle_kitty_apc(payload), KittyApcResult::QueryHandled);
        assert_eq!(core.response_len, 0);
    }

    // ── Non-query final chunk tests ──────────────────────

    #[test]
    fn test_kitty_transmit_final_chunk() {
        let mut core = TerminalCore::new(80, 24, 0);
        let payload = b"Ga=T,i=42,f=100;iVBORw0KGgo=";
        assert_eq!(core.handle_kitty_apc(payload), KittyApcResult::FinalChunk);
        // No response generated (handled by PTY reader thread)
        assert_eq!(core.response_len, 0);
    }

    #[test]
    fn test_kitty_default_action_final_chunk() {
        let mut core = TerminalCore::new(80, 24, 0);
        let payload = b"Gi=99,f=100;iVBORw0KGgo=";
        assert_eq!(core.handle_kitty_apc(payload), KittyApcResult::FinalChunk);
        assert_eq!(core.response_len, 0);
    }

    #[test]
    fn test_kitty_final_chunk_with_placement_id() {
        let mut core = TerminalCore::new(80, 24, 0);
        let payload = b"Ga=T,i=42,p=7,f=100;iVBORw0KGgo=";
        assert_eq!(core.handle_kitty_apc(payload), KittyApcResult::FinalChunk);
        assert_eq!(core.response_len, 0);
    }

    #[test]
    fn test_kitty_final_chunk_quiet() {
        let mut core = TerminalCore::new(80, 24, 0);
        // q=1 doesn't affect classification (only response generation)
        let payload = b"Ga=T,i=42,q=1;iVBORw0KGgo=";
        assert_eq!(core.handle_kitty_apc(payload), KittyApcResult::FinalChunk);
        assert_eq!(core.response_len, 0);
    }

    // ── Continuation chunk tests ─────────────────────────

    #[test]
    fn test_kitty_more_chunks() {
        let mut core = TerminalCore::new(80, 24, 0);
        let payload = b"Ga=T,i=42,m=1;iVBORw0KGgo=";
        assert_eq!(core.handle_kitty_apc(payload), KittyApcResult::MoreChunks);
        assert_eq!(core.response_len, 0);
    }

    #[test]
    fn test_kitty_explicit_m0_is_final() {
        let mut core = TerminalCore::new(80, 24, 0);
        let payload = b"Ga=T,i=42,m=0;iVBORw0KGgo=";
        assert_eq!(core.handle_kitty_apc(payload), KittyApcResult::FinalChunk);
        assert_eq!(core.response_len, 0);
    }

    // ── Non-Kitty APC tests ─────────────────────────────

    #[test]
    fn test_non_kitty_apc() {
        let mut core = TerminalCore::new(80, 24, 0);
        let payload = b"Hello";
        assert_eq!(core.handle_kitty_apc(payload), KittyApcResult::NotKitty);
    }

    #[test]
    fn test_empty_payload() {
        let mut core = TerminalCore::new(80, 24, 0);
        let payload = b"";
        assert_eq!(core.handle_kitty_apc(payload), KittyApcResult::NotKitty);
    }
}
