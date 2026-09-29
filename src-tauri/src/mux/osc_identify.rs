//! Shared OSC number recovery and viewer-launch identification
//! (mux-suppressed-output-round2-fixes task0001, FR7).
//!
//! Two consumers classify an OSC body by what the client's `term_core` parser
//! would dispatch: the scrollback strip (`super::scrollback_filter`, ring
//! write and snapshot assembly) and the suppressed-chunk delivery scan
//! (`super::ipc::pty_spawn::client_parity_scan`). Both go through this module,
//! so a viewer launch written with a leading-zero number (`0777;…`) or with
//! non-digit bytes before the first `;` (`777emterm;;markdown;…`) is seen as
//! the same launch by both sides and can neither survive in the ring while
//! also being delivered, nor be lost.
//!
//! Two responsibilities:
//!
//! 1. **Recovery** ([`recover_osc`]): the OSC number and data exactly as
//!    term_core's OSC string state reconstructs them.
//! 2. **Identification** ([`identify_osc`]): what the OSC is. The
//!    identification never decides whether to strip or deliver; each consumer
//!    maps the identity onto its own selection.
//!
//! This is a leaf module: it depends on [`crate::viewer_kinds`] only — never
//! on `mux::ipc`, never on an item gated on the `gui` feature (NFR6).

use crate::viewer_kinds::REPLAYABLE_VIEWER_KINDS;

/// The OSC 777 `<kind>` token for agent-status reports (SPEC FR1/FR4). The
/// single source of the literal: the strip selection reaches it only through
/// [`OscIdentity::AgentStatusReport`].
pub(in crate::mux) const AGENT_STATUS_OSC_KIND: &str = "agent-status";

/// The OSC 777 payload prefix that introduces every eMterm extension kind.
const OSC_777_PAYLOAD_PREFIX: &str = "emterm;";

/// The OSC number that carries eMterm extension kinds (`emterm;<kind>;…`).
const OSC_NUMBER_EMTERM: u16 = 777;

/// The OSC number of the Markdown launch (`emterm-md[;…]`).
const OSC_NUMBER_MARKDOWN: u16 = 9999;

/// The data token of the Markdown launch on [`OSC_NUMBER_MARKDOWN`].
const MARKDOWN_LAUNCH_TOKEN: &str = "emterm-md";

/// An OSC body's number and data as term_core reconstructs them.
///
/// `number` is `None` when the accumulation exceeds term_core's `u16`
/// (as-06): term_core's own arithmetic can wrap or panic there, so no route
/// can be reliably established. Recovery itself never panics (all arithmetic
/// is saturating).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::mux) struct RecoveredOsc {
    pub(in crate::mux) number: Option<u16>,
    pub(in crate::mux) data: String,
}

/// What an OSC is, as far as the mux strip and delivery decisions care.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::mux) enum OscIdentity {
    /// OSC 777 `emterm;<kind>[;…]` with `<kind>` one of
    /// [`REPLAYABLE_VIEWER_KINDS`] (image included).
    ViewerLaunch(&'static str),
    /// OSC 9999 `emterm-md[;…]`.
    MarkdownLaunch,
    /// OSC 777 `emterm;agent-status[;…]`.
    AgentStatusReport,
    /// Everything else: fold, emterm-mux, other kinds, other numbers, an
    /// absent number.
    NotIdentified,
}

/// Recover an OSC body's number and data.
///
/// `body` is the bytes between the `ESC ]` introducer and the terminator,
/// terminator excluded. Digits before the first `;` accumulate in base 10
/// (leading zeros do not change the value); the first `;` is dropped; every
/// other byte is data — including non-digit bytes before the first `;` and
/// digits after it. The data is decoded lossily, as term_core's dispatch does.
/// A body without leading digits yields number zero.
///
/// A single pass over `body`, no panicking arithmetic or indexing (TM-2).
pub(in crate::mux) fn recover_osc(body: &[u8]) -> RecoveredOsc {
    let mut acc: u32 = 0;
    let mut overflowed = false;
    let mut done = false;
    let mut data = Vec::with_capacity(body.len());
    for &b in body {
        if !done {
            if b.is_ascii_digit() {
                acc = acc.saturating_mul(10).saturating_add(u32::from(b - b'0'));
                if acc > u32::from(u16::MAX) {
                    overflowed = true;
                }
                continue;
            }
            if b == b';' {
                done = true;
                continue;
            }
        }
        data.push(b);
    }
    let number = if overflowed {
        None
    } else {
        u16::try_from(acc).ok()
    };
    let data = String::from_utf8(data)
        .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
    RecoveredOsc { number, data }
}

/// Classify a recovered OSC as exactly one [`OscIdentity`].
pub(in crate::mux) fn identify_osc(osc: &RecoveredOsc) -> OscIdentity {
    match osc.number {
        Some(OSC_NUMBER_EMTERM) => {
            let Some(rest) = osc.data.strip_prefix(OSC_777_PAYLOAD_PREFIX) else {
                return OscIdentity::NotIdentified;
            };
            let kind = rest.split(';').next().unwrap_or(rest);
            if kind == AGENT_STATUS_OSC_KIND {
                return OscIdentity::AgentStatusReport;
            }
            match REPLAYABLE_VIEWER_KINDS.iter().find(|&&k| k == kind) {
                Some(&viewer_kind) => OscIdentity::ViewerLaunch(viewer_kind),
                None => OscIdentity::NotIdentified,
            }
        }
        Some(OSC_NUMBER_MARKDOWN) => {
            let is_markdown = osc.data == MARKDOWN_LAUNCH_TOKEN
                || osc
                    .data
                    .strip_prefix(MARKDOWN_LAUNCH_TOKEN)
                    .is_some_and(|rest| rest.starts_with(';'));
            if is_markdown {
                OscIdentity::MarkdownLaunch
            } else {
                OscIdentity::NotIdentified
            }
        }
        _ => OscIdentity::NotIdentified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identify_body(body: &[u8]) -> OscIdentity {
        identify_osc(&recover_osc(body))
    }

    /// The three body forms the identification must treat alike: canonical,
    /// leading-zero number, and non-digit bytes before the first `;`.
    fn viewer_forms(kind: &str) -> Vec<Vec<u8>> {
        vec![
            format!("777;emterm;{kind};x").into_bytes(),
            format!("777;emterm;{kind}").into_bytes(),
            format!("0777;emterm;{kind};x").into_bytes(),
            format!("0777;emterm;{kind}").into_bytes(),
            format!("777emterm;;{kind};x").into_bytes(),
        ]
    }

    // ---- AC-1 (TM-2): recovery ----

    #[test]
    fn ac1_recovery_returns_number_and_data_as_term_core_reconstructs_them() {
        let cases: &[(&[u8], Option<u16>, &str)] = &[
            (b"10;?;?", Some(10), "?;?"),
            (b"010;?", Some(10), "?"),
            (b"0777;emterm;markdown;x", Some(777), "emterm;markdown;x"),
            (b"777emterm;;markdown;x", Some(777), "emterm;markdown;x"),
            // A digit run after the first `;` is data, not number.
            (b"10;5;6", Some(10), "5;6"),
            // Only the first `;` is dropped.
            (b";x;y", Some(0), "x;y"),
            // A body without leading digits yields number zero.
            (b"hello", Some(0), "hello"),
            // The first `;` is dropped even when non-digit bytes precede it.
            (b"abc;def", Some(0), "abcdef"),
            // u16 boundary.
            (b"65535;x", Some(65535), "x"),
            (b"65536;x", None, "x"),
            (b"999999999;?", None, "?"),
        ];
        for &(body, number, data) in cases {
            let osc = recover_osc(body);
            assert_eq!(
                osc.number,
                number,
                "number of {:?}",
                String::from_utf8_lossy(body)
            );
            assert_eq!(
                osc.data,
                data,
                "data of {:?}",
                String::from_utf8_lossy(body)
            );
        }
    }

    #[test]
    fn ac1_recovery_of_an_empty_body_is_number_zero_and_empty_data() {
        let osc = recover_osc(b"");
        assert_eq!(osc.number, Some(0));
        assert_eq!(osc.data, "");
    }

    #[test]
    fn ac1_recovery_decodes_invalid_utf8_lossily_without_panicking() {
        let osc = recover_osc(b"777;\xff\xfe;x");
        assert_eq!(osc.number, Some(777));
        assert_eq!(osc.data, "\u{fffd}\u{fffd};x");
    }

    #[test]
    fn ac1_recovery_of_one_million_digits_overflows_without_panicking() {
        let osc = recover_osc(&vec![b'1'; 1_000_000]);
        assert_eq!(osc.number, None);
        assert_eq!(osc.data, "");
    }

    #[test]
    fn ac1_recovery_of_one_million_leading_zeros_keeps_the_value_zero() {
        let mut body = vec![b'0'; 1_000_000];
        body.extend_from_slice(b";x");
        let osc = recover_osc(&body);
        assert_eq!(osc.number, Some(0));
        assert_eq!(osc.data, "x");
    }

    // ---- AC-2: identification ----

    #[test]
    fn ac2_every_replayable_viewer_kind_is_a_viewer_launch_in_all_forms() {
        for expected in ["markdown", "image", "json", "yaml", "html"] {
            assert!(
                REPLAYABLE_VIEWER_KINDS.contains(&expected),
                "the viewer-kind SSOT lost {expected}"
            );
        }
        for &kind in REPLAYABLE_VIEWER_KINDS {
            for body in viewer_forms(kind) {
                assert_eq!(
                    identify_body(&body),
                    OscIdentity::ViewerLaunch(kind),
                    "body {:?}",
                    String::from_utf8_lossy(&body)
                );
            }
        }
    }

    #[test]
    fn ac2_markdown_launch_is_identified_in_all_forms() {
        for body in [
            b"9999;emterm-md".as_slice(),
            b"9999;emterm-md;x",
            b"09999;emterm-md;x",
            b"09999;emterm-md",
            b"9999emterm-md;;x",
        ] {
            assert_eq!(
                identify_body(body),
                OscIdentity::MarkdownLaunch,
                "body {:?}",
                String::from_utf8_lossy(body)
            );
        }
    }

    #[test]
    fn ac2_agent_status_report_is_identified() {
        for body in [
            b"777;emterm;agent-status;x".as_slice(),
            b"777;emterm;agent-status",
            b"0777;emterm;agent-status;v=1;state=idle",
            b"777emterm;;agent-status;x",
        ] {
            assert_eq!(
                identify_body(body),
                OscIdentity::AgentStatusReport,
                "body {:?}",
                String::from_utf8_lossy(body)
            );
        }
    }

    #[test]
    fn ac2_agent_status_kind_name_has_a_single_source() {
        assert_eq!(AGENT_STATUS_OSC_KIND, "agent-status");
    }

    #[test]
    fn ac2_everything_else_is_not_identified() {
        for body in [
            b"777;emterm;fold;x".as_slice(),
            b"0777;emterm;fold;x",
            b"9999;emterm-mux;x",
            b"9999;emterm-mdx",
            b"9999;emterm-mdx;x",
            b"777;other;x",
            b"777;emterm;",
            b"777;emterm;markdownx",
            b"777;emterm;status-bar;x",
            b"778;emterm;markdown;x",
            b"9998;emterm-md;x",
            // Overflowed numbers (as-06) never route.
            b"70000;emterm;markdown;x",
            b"65536;emterm;markdown;x",
            b"99999;emterm-md;x",
            // No leading digits: number zero.
            b"emterm;markdown;x",
            b"",
        ] {
            assert_eq!(
                identify_body(body),
                OscIdentity::NotIdentified,
                "body {:?}",
                String::from_utf8_lossy(body)
            );
        }
    }
}
