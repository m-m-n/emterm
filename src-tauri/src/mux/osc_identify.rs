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
//! also being delivered, nor be lost. The same holds for OSC 7501 (Program
//! Status Protocol): [`OscIdentity::ProgramStatus`] is decided by the number
//! alone, so a report or query in any spelling of 7501 is the same sequence
//! for the strip and for the delivery scan (osc7501-program-status task0006).
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

/// The OSC number of the Program Status Protocol (reports and the `?` query).
pub(in crate::mux) const OSC_NUMBER_PROGRAM_STATUS: u16 = 7501;

/// The OSC number of the semantic prompt marks (`A` prompt start, `B`, `C`, `D`).
pub(in crate::mux) const OSC_NUMBER_SEMANTIC_PROMPT: u16 = 133;

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
    /// OSC 7501 (Program Status Protocol): a report or the `?` query,
    /// whatever its data. The number alone decides; the data is not read.
    ProgramStatus,
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
        Some(OSC_NUMBER_PROGRAM_STATUS) => OscIdentity::ProgramStatus,
        _ => OscIdentity::NotIdentified,
    }
}

/// The number part of an OSC body under the shared recognition rule (SC-1).
struct NumberScan {
    /// The accumulated number; `None` above the `u16` range.
    number: Option<u16>,
    /// Where the number part ends: the index of the first `;`, or
    /// `body.len()` when the body has none. Meaningful only with a number.
    head_len: usize,
}

/// The one scan of the number part both [`osc_body_number`] and
/// [`osc_body_identity`] take their number from.
///
/// Digits before the first `;` accumulate in base 10 with saturation, so
/// leading zeros do not change the value; every other byte before the first
/// `;` is data and is skipped here. Above the `u16` range `recover_osc`
/// yields no number, and the accumulation only grows, so the scan stops at
/// once. One forward pass, no allocation, no panicking arithmetic or
/// indexing.
fn scan_number(body: &[u8]) -> NumberScan {
    let mut acc: u32 = 0;
    let mut head_len = body.len();
    for (i, &b) in body.iter().enumerate() {
        if b.is_ascii_digit() {
            acc = acc.saturating_mul(10).saturating_add(u32::from(b - b'0'));
            if acc > u32::from(u16::MAX) {
                return NumberScan {
                    number: None,
                    head_len: i,
                };
            }
        } else if b == b';' {
            head_len = i;
            break;
        }
    }
    NumberScan {
        number: u16::try_from(acc).ok(),
        head_len,
    }
}

/// The OSC number of `body` under the shared recognition rule (SC-1): the
/// bytes between `ESC ]` and the terminator, terminator excluded. Returns
/// exactly `recover_osc(body).number` for every input (leading zeros do not
/// change the value, non-digit bytes before the first `;` are skipped, a body
/// without leading digits yields `Some(0)`, a value above the `u16` range
/// yields `None`) without building the data and without any heap allocation.
/// A single forward pass bounded by `body`; it cannot panic.
pub(in crate::mux) fn osc_body_number(body: &[u8]) -> Option<u16> {
    scan_number(body).number
}

/// Identify an OSC body without copying it and without validating it as
/// UTF-8 (round3 FR8).
///
/// Returns exactly what `identify_osc(&recover_osc(body))` returns for the
/// same `body`, for every input — the pair stays as the reference for the
/// delivery side and as the test oracle. This entry is for the write path,
/// where a body is identified for every OSC the shell emits: it makes no heap
/// allocation, takes one forward pass bounded by `body`, and cannot panic.
///
/// The data view that `recover_osc` would build is never materialized. It is
/// the non-digit bytes before the first `;` (in order, so it can be
/// non-contiguous when digits are interleaved) followed by the bytes after
/// the first `;`, and it is walked lazily as an iterator over the borrowed
/// body.
///
/// Why comparing raw bytes agrees with the reference's comparison of the
/// lossily decoded string: every token matched below (`emterm;`, the kinds,
/// `emterm-md`) is ASCII. Lossy decoding replaces invalid bytes by U+FFFD,
/// which is never ASCII, and never absorbs a valid ASCII byte, so an
/// invalid byte inside a token matches nothing on either side.
pub(in crate::mux) fn osc_body_identity(body: &[u8]) -> OscIdentity {
    // The OSC number: base-10 digits before the first `;`, from the one scan
    // [`osc_body_number`] shares. Non-digit bytes there belong to the data
    // view. Above the u16 range `recover_osc` yields no number.
    let NumberScan { number, head_len } = scan_number(body);
    let Some(number) = number else {
        return OscIdentity::NotIdentified;
    };
    // Program status is decided by the number alone: no data view is needed.
    if number == OSC_NUMBER_PROGRAM_STATUS {
        return OscIdentity::ProgramStatus;
    }
    if number != OSC_NUMBER_EMTERM && number != OSC_NUMBER_MARKDOWN {
        return OscIdentity::NotIdentified;
    }

    // The borrowed data view. `head_len <= body.len()`, so the split cannot
    // fail; the first `;` (at `tail[0]`, when there is one) is dropped.
    let (head, tail) = body.split_at(head_len);
    let tail = tail.get(1..).unwrap_or_default();
    let mut view = head
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_digit())
        .chain(tail.iter().copied());

    if number == OSC_NUMBER_EMTERM {
        if !consume_literal(&mut view, OSC_777_PAYLOAD_PREFIX) {
            return OscIdentity::NotIdentified;
        }
        // The kind runs up to the next `;`, or to the end of the view.
        let kind = view.take_while(|&b| b != b';');
        if kind.clone().eq(AGENT_STATUS_OSC_KIND.bytes()) {
            return OscIdentity::AgentStatusReport;
        }
        match REPLAYABLE_VIEWER_KINDS
            .iter()
            .find(|&&k| kind.clone().eq(k.bytes()))
        {
            Some(&viewer_kind) => OscIdentity::ViewerLaunch(viewer_kind),
            None => OscIdentity::NotIdentified,
        }
    } else {
        // The token alone, or the token followed by `;`.
        if consume_literal(&mut view, MARKDOWN_LAUNCH_TOKEN)
            && matches!(view.next(), None | Some(b';'))
        {
            OscIdentity::MarkdownLaunch
        } else {
            OscIdentity::NotIdentified
        }
    }
}

/// Consume `literal` from the front of `view`: true when the view starts with
/// exactly those bytes (the view is advanced past them), false at the first
/// difference or when the view ends early.
fn consume_literal(view: &mut impl Iterator<Item = u8>, literal: &str) -> bool {
    literal
        .bytes()
        .all(|expected| view.next() == Some(expected))
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

    // ---- osc7501-program-status task0006 AC-1 (FR15): program status ----

    /// Bodies whose reconstructed number is 7501: canonical and leading-zero
    /// spellings, reports of any data, the `?` query, a body without data, a
    /// non-digit byte before the first `;` and invalid UTF-8 data.
    fn program_status_forms() -> Vec<Vec<u8>> {
        vec![
            b"7501;?".to_vec(),
            b"7501;state=working".to_vec(),
            b"7501;id=a/b;state=idle;app=claude".to_vec(),
            b"7501;".to_vec(),
            b"7501".to_vec(),
            b"07501;?".to_vec(),
            b"0000007501;state=done".to_vec(),
            b"7501x;?".to_vec(),
            b"75x01;?".to_vec(),
            b"7501emterm;;?".to_vec(),
            b"?7501;?".to_vec(),
            b"7501;\xff\xfe".to_vec(),
            b"7501;emterm;markdown;begin".to_vec(),
            b"7501;777;emterm;agent-status".to_vec(),
        ]
    }

    #[test]
    fn osc7501_ac1_program_status_is_identified_in_all_forms() {
        for body in program_status_forms() {
            assert_eq!(
                identify_body(&body),
                OscIdentity::ProgramStatus,
                "reference identity of {:?}",
                String::from_utf8_lossy(&body)
            );
            assert_eq!(
                osc_body_identity(&body),
                OscIdentity::ProgramStatus,
                "zero-allocation identity of {:?}",
                String::from_utf8_lossy(&body)
            );
        }
    }

    #[test]
    fn osc7501_ac1_numbers_next_to_7501_keep_their_identity() {
        for body in [
            b"7500;?".as_slice(),
            b"7502;?",
            b"750;?",
            b"501;?",
            b"17501;?",
            b"75010;?",
            b"7;501;?",
            b"0;7501",
            b";7501;?",
            b"",
        ] {
            assert_eq!(
                identify_body(body),
                OscIdentity::NotIdentified,
                "reference identity of {:?}",
                String::from_utf8_lossy(body)
            );
            assert_eq!(
                osc_body_identity(body),
                OscIdentity::NotIdentified,
                "zero-allocation identity of {:?}",
                String::from_utf8_lossy(body)
            );
        }
    }

    #[test]
    fn osc7501_ac1_the_other_identities_are_unchanged() {
        assert_eq!(
            osc_body_identity(b"777;emterm;markdown;x"),
            OscIdentity::ViewerLaunch("markdown")
        );
        assert_eq!(
            osc_body_identity(b"777;emterm;agent-status;v=1"),
            OscIdentity::AgentStatusReport
        );
        assert_eq!(
            osc_body_identity(b"9999;emterm-md;x"),
            OscIdentity::MarkdownLaunch
        );
        assert_eq!(
            osc_body_identity(b"777;emterm;fold;x"),
            OscIdentity::NotIdentified
        );
        assert_eq!(
            osc_body_identity(b"9999;emterm-mux;x"),
            OscIdentity::NotIdentified
        );
    }

    // ---- round3 FR8 (b3e644c5e2d31809): allocation-free identification ----

    /// Test-only global allocator wrapper that counts allocation requests
    /// made on a thread while that thread has armed the counter
    /// (IMPLEMENTATION.md Conventions, "Test-only allocation counting").
    ///
    /// It is installed for the whole lib test binary, so it adds no
    /// allocation, no lazy initialization and no destructor of its own: the
    /// thread-local state is constant-initialized `Cell`s of plain integers,
    /// and every request is forwarded to the system allocator unchanged.
    mod alloc_counter {
        use std::alloc::{GlobalAlloc, Layout, System};
        use std::cell::Cell;

        thread_local! {
            static ARMED: Cell<bool> = const { Cell::new(false) };
            static COUNT: Cell<usize> = const { Cell::new(0) };
        }

        /// Record one allocation request when the calling thread is armed.
        /// `try_with` never panics, and neither cell allocates.
        fn note_request() {
            let _ = ARMED.try_with(|armed| {
                if armed.get() {
                    let _ = COUNT.try_with(|count| count.set(count.get() + 1));
                }
            });
        }

        struct CountingAllocator;

        // SAFETY: every method forwards its arguments unchanged to `System`,
        // which upholds the `GlobalAlloc` contract; the counting side effect
        // touches only constant-initialized thread-local cells.
        unsafe impl GlobalAlloc for CountingAllocator {
            unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
                note_request();
                unsafe { System.alloc(layout) }
            }

            unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
                note_request();
                unsafe { System.alloc_zeroed(layout) }
            }

            unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
                note_request();
                unsafe { System.realloc(ptr, layout, new_size) }
            }

            unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
                unsafe { System.dealloc(ptr, layout) }
            }
        }

        #[global_allocator]
        static COUNTING_ALLOCATOR: CountingAllocator = CountingAllocator;

        /// Disarms the calling thread on drop, so a panic inside the
        /// measured closure cannot leave the thread armed.
        struct Armed;

        impl Drop for Armed {
            fn drop(&mut self) {
                let _ = ARMED.try_with(|armed| armed.set(false));
            }
        }

        /// Run `f` with the calling thread armed and return its result with
        /// the number of allocation requests made on this thread meanwhile.
        /// Only `f` runs while armed: arm immediately before, disarm
        /// immediately after.
        pub(super) fn measure<R>(f: impl FnOnce() -> R) -> (R, usize) {
            let before = COUNT.with(Cell::get);
            let guard = Armed;
            ARMED.with(|armed| armed.set(true));
            let result = f();
            drop(guard);
            (result, COUNT.with(Cell::get) - before)
        }
    }

    /// A byte that is not valid UTF-8 anywhere in a string.
    const INVALID: u8 = 0xff;

    /// Every body up to `max_len` symbols long, over `alphabet`.
    fn every_body_over(alphabet: &[&[u8]], max_len: usize) -> Vec<Vec<u8>> {
        let mut all: Vec<Vec<u8>> = vec![Vec::new()];
        let mut frontier: Vec<Vec<u8>> = vec![Vec::new()];
        for _ in 0..max_len {
            let mut next = Vec::new();
            for prefix in &frontier {
                for symbol in alphabet {
                    let mut body = prefix.clone();
                    body.extend_from_slice(symbol);
                    next.push(body);
                }
            }
            all.extend(next.iter().cloned());
            frontier = next;
        }
        all
    }

    /// The equivalence corpus (round3 AC-1): every shape the identification
    /// has to agree with the reference on.
    fn identification_corpus() -> Vec<Vec<u8>> {
        let mut corpus: Vec<Vec<u8>> = Vec::new();

        // Canonical OSC 777 bodies (and the leading-zero / non-digit forms)
        // for every replayable viewer kind, image included, and agent-status.
        for &kind in REPLAYABLE_VIEWER_KINDS
            .iter()
            .chain(std::iter::once(&AGENT_STATUS_OSC_KIND))
        {
            corpus.extend(viewer_forms(kind));
        }

        let fixed: &[&[u8]] = &[
            // OSC 9999: the token itself, with a `;` tail, near misses.
            b"9999;emterm-md",
            b"9999;emterm-md;x",
            b"9999;emterm-md;",
            b"9999;emterm-mdx",
            b"9999;emterm-mdx;x",
            b"9999;emterm-m",
            b"9999;emterm-mux;x",
            b"9999;emterm-md ",
            b"9999;Emterm-md",
            b"9999emterm-md",
            b"9999emterm-md;;x",
            b"9998;emterm-md;x",
            b"9999;",
            b"9999",
            // Leading-zero numbers.
            b"0777;emterm;markdown;x",
            b"00777;emterm;image",
            b"0000777;emterm;agent-status;v=1;state=idle",
            b"09999;emterm-md",
            b"09999;emterm-md;x",
            b"009999;emterm-mdx",
            // Non-digit bytes before the first `;`, digits interleaved.
            b"777emterm;;markdown;x",
            b"7e7m7term;;markdown",
            b"a777emterm;;json",
            b"777x;emterm;markdown",
            b"7x7x7;emterm;yaml",
            b"77 7;emterm;html",
            b"emterm;markdown;x",
            b"777emterm",
            b"777emterm;",
            b"9999emterm-md;",
            b"9e9m9t9erm-md;;x",
            b"9e9m9t9e9r9m9-md;;x",
            // Bodies with no `;`.
            b"777",
            b"9999",
            b"emterm",
            b"markdown",
            b"x",
            b"0",
            // The empty body.
            b"",
            // Other numbers.
            b"0;emterm;markdown",
            b"2;emterm;markdown",
            b"11;?;?",
            b"52;c;?",
            b"133;A",
            b"778;emterm;markdown;x",
            b"7770;emterm;markdown",
            b"77;emterm;markdown",
            // u16 boundary and beyond (saturating, never wrapping).
            b"65535;emterm;markdown",
            b"65536;emterm;markdown;x",
            b"66313;emterm;markdown;x",
            b"70000;emterm-md",
            b"75535;emterm-md",
            b"99999;emterm-md;x",
            b"4294967296;emterm;markdown",
            b"4294967297;emterm;markdown",
            b"4294968073;emterm;markdown",
            b"99999999999999999999;emterm;markdown;x",
            b"65536",
            // Invalid UTF-8 before and after the first `;`.
            b"777;\xffemterm;markdown",
            b"777;emterm\xff;markdown",
            b"777;emterm;\xffmarkdown",
            b"777;emterm;mark\xffdown",
            b"777;emterm;markdown\xff",
            b"777;emterm;markdown;\xff",
            b"777;emterm;markdown;\xe2\x82",
            b"777;\xe2\x82emterm;markdown",
            b"777;emterm;agent-status\xff",
            b"777;emterm;agent-status;\xff\xfe",
            b"777\xff;emterm;markdown",
            b"\xff777;emterm;markdown",
            b"7\xff7\xff7;emterm;markdown",
            b"777;emterm;\xef\xbf\xbdmarkdown",
            b"9999;emterm-md\xff",
            b"9999;emterm-md;\xff",
            b"9999;\xffemterm-md",
            b"9999;emterm\xff-md",
            b"9999\xff;emterm-md",
            b"\xff9999;emterm-md",
            b"\xff",
            b";\xff",
            // OSC 7501 (program status): the number alone, near misses and
            // the same spellings as the other identities.
            b"7501;?",
            b"7501;state=working;id=a/b",
            b"7501",
            b"7501;",
            b"07501;?",
            b"00007501;state=done",
            b"7501x;?",
            b"75x01;?",
            b"7x5x0x1;?",
            b"7501emterm;;?",
            b"7501;emterm;markdown;begin",
            b"7501;\xff\xfe",
            b"\xff7501;?",
            b"7501\xff;?",
            b"7500;?",
            b"7502;?",
            b"750;?",
            b"17501;?",
            b"75010;?",
            b"65535;?",
            b"65536;?",
            b"0;7501;?",
            b";7501;?",
        ];
        corpus.extend(fixed.iter().map(|body| body.to_vec()));

        // Long bodies: a launch with a long tail, a long garbage head, a long
        // run of leading zeros.
        for prefix in [
            "777;emterm;markdown;",
            "777;emterm;agent-status;",
            "9999;emterm-md;",
        ] {
            let mut body = prefix.as_bytes().to_vec();
            body.extend(std::iter::repeat_n(b'x', 100_000));
            corpus.push(body);
        }
        let mut long_head = vec![b'a'; 100_000];
        long_head.extend_from_slice(b";emterm-md");
        corpus.push(long_head);
        let mut zeros = vec![b'0'; 100_000];
        zeros.extend_from_slice(b"777;emterm;json");
        corpus.push(zeros);
        let mut program_status = b"7501;".to_vec();
        program_status.extend(std::iter::repeat_n(b'x', 100_000));
        corpus.push(program_status);
        let mut zeros_then_program_status = vec![b'0'; 100_000];
        zeros_then_program_status.extend_from_slice(b"7501;?");
        corpus.push(zeros_then_program_status);

        // Every body up to 4 bytes long over the significant bytes: digits,
        // `;`, `e`, `m` and one invalid byte.
        let alphabet: &[&[u8]] = &[b"0", b"7", b"9", b";", b"e", b"m", &[INVALID]];
        corpus.extend(every_body_over(alphabet, 4));

        // Every body up to 4 tokens long over the tokens the identity rules
        // are made of, so identified bodies occur in the exhaustive part too.
        let tokens: &[&[u8]] = &[
            b"777",
            b"9999",
            b"7501",
            b"0",
            b"7",
            b";",
            b"?",
            b"emterm;",
            b"emterm-md",
            b"markdown",
            b"agent-status",
            b"x",
            &[INVALID],
        ];
        corpus.extend(every_body_over(tokens, 4));

        corpus
    }

    /// The reference result: recover, then identify.
    fn reference_identity(body: &[u8]) -> OscIdentity {
        identify_osc(&recover_osc(body))
    }

    // ---- AC-1 (FR8 equivalence) ----

    #[test]
    fn round3_ac1_osc_body_identity_matches_the_reference_over_the_corpus() {
        for body in identification_corpus() {
            assert_eq!(
                osc_body_identity(&body),
                reference_identity(&body),
                "body {:?}",
                String::from_utf8_lossy(&body[..body.len().min(64)])
            );
        }
    }

    #[test]
    fn round3_ac1_the_corpus_reaches_every_identity() {
        let corpus = identification_corpus();
        let identities: Vec<OscIdentity> = corpus.iter().map(|b| reference_identity(b)).collect();
        for &kind in REPLAYABLE_VIEWER_KINDS {
            assert!(
                identities.contains(&OscIdentity::ViewerLaunch(kind)),
                "no corpus body identifies as the {kind} launch"
            );
        }
        for expected in [
            OscIdentity::MarkdownLaunch,
            OscIdentity::AgentStatusReport,
            OscIdentity::ProgramStatus,
            OscIdentity::NotIdentified,
        ] {
            assert!(
                identities.contains(&expected),
                "no corpus body identifies as {expected:?}"
            );
        }
    }

    #[test]
    fn round3_ac1_identity_of_the_non_contiguous_view_forms() {
        // The data view is the non-digit bytes before the first `;`
        // followed by the bytes after it, so interleaved digits drop out.
        assert_eq!(
            osc_body_identity(b"7e7m7term;;markdown"),
            OscIdentity::ViewerLaunch("markdown")
        );
        assert_eq!(
            osc_body_identity(b"9e9m9t9erm-md;;x"),
            OscIdentity::MarkdownLaunch
        );
        // Seven interleaved nines overflow the u16 range.
        assert_eq!(
            osc_body_identity(b"9e9m9t9e9r9m9-md;;x"),
            OscIdentity::NotIdentified
        );
        assert_eq!(
            osc_body_identity(b"9999emterm-md"),
            OscIdentity::MarkdownLaunch
        );
        assert_eq!(
            osc_body_identity(b"0000777;emterm;agent-status;v=1"),
            OscIdentity::AgentStatusReport
        );
    }

    // ---- AC-2 (FR8, NFR3): the registry test ----

    #[test]
    fn round3_b3e644c5_osc_identification_is_allocation_free_and_matches_the_reference() {
        // Build everything the measurement needs before arming anything.
        let corpus = identification_corpus();
        let expected: Vec<OscIdentity> = corpus.iter().map(|b| reference_identity(b)).collect();

        // Control: the counter sees the reference recovery's allocation on
        // this very thread, so a zero below means "no allocation", not "the
        // counter is not installed".
        let (recovered, control) = alloc_counter::measure(|| recover_osc(b"777;emterm;markdown;x"));
        assert!(
            control >= 1,
            "the allocation counter recorded {control} requests around the reference recovery"
        );
        drop(recovered);

        let mut total_requests = 0usize;
        for (body, expected) in corpus.iter().zip(&expected) {
            let (actual, requests) = alloc_counter::measure(|| osc_body_identity(body));
            total_requests += requests;
            assert_eq!(
                actual,
                *expected,
                "body {:?}",
                String::from_utf8_lossy(&body[..body.len().min(64)])
            );
        }
        assert_eq!(
            total_requests,
            0,
            "identification allocated over a corpus of {} bodies",
            corpus.len()
        );
    }

    // ---- AC-4 (TM-2, NFR5): large bodies ----

    #[test]
    fn round3_ac4_one_mebibyte_bodies_finish_within_two_seconds_and_match_the_reference() {
        const MIB: usize = 1024 * 1024;
        let mut bodies: Vec<Vec<u8>> = vec![
            // All non-digit, no `;`.
            vec![b'x'; MIB],
            // All zeros, no `;`.
            vec![b'0'; MIB],
            // Digits interleaved with non-digits, no `;`.
            std::iter::repeat_n(*b"7e", MIB / 2).flatten().collect(),
            // All `;`.
            vec![b';'; MIB],
            // Invalid bytes throughout.
            vec![INVALID; MIB],
        ];
        let mut launch = b"777;emterm;markdown;".to_vec();
        launch.resize(MIB, b'x');
        bodies.push(launch);
        let mut agent = b"777;emterm;agent-status;".to_vec();
        agent.resize(MIB, INVALID);
        bodies.push(agent);
        let mut markdown = b"9999;emterm-md;".to_vec();
        markdown.resize(MIB, b'9');
        bodies.push(markdown);
        let mut program_status = b"7501;".to_vec();
        program_status.resize(MIB, INVALID);
        bodies.push(program_status);
        let mut long_head = vec![b'x'; MIB - 16];
        long_head.extend_from_slice(b";emterm-md");
        bodies.push(long_head);

        let expected: Vec<OscIdentity> = bodies.iter().map(|b| reference_identity(b)).collect();
        let started = std::time::Instant::now();
        let actual: Vec<OscIdentity> = bodies.iter().map(|b| osc_body_identity(b)).collect();
        let elapsed = started.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "identifying {} one-MiB bodies took {elapsed:?}",
            bodies.len()
        );
        assert_eq!(actual, expected);
    }

    #[test]
    fn round3_ac4_a_number_of_one_million_digits_finishes_within_two_seconds_and_matches_the_reference()
     {
        let mut bodies: Vec<Vec<u8>> = Vec::new();
        // Overflows the u16 range after a few digits.
        bodies.push(vec![b'1'; 1_000_000]);
        let mut overflow_then_launch = vec![b'9'; 1_000_000];
        overflow_then_launch.extend_from_slice(b";emterm;markdown");
        bodies.push(overflow_then_launch);
        // One million leading zeros keep the value, so the launch after
        // them is still identified.
        let mut zeros_then_viewer = vec![b'0'; 1_000_000];
        zeros_then_viewer.extend_from_slice(b"777;emterm;markdown");
        bodies.push(zeros_then_viewer);
        let mut zeros_then_markdown = vec![b'0'; 1_000_000];
        zeros_then_markdown.extend_from_slice(b"9999;emterm-md");
        bodies.push(zeros_then_markdown);
        let mut zeros_then_program_status = vec![b'0'; 1_000_000];
        zeros_then_program_status.extend_from_slice(b"7501;?");
        bodies.push(zeros_then_program_status);
        // One million zeros, nothing else: number zero.
        bodies.push(vec![b'0'; 1_000_000]);

        let expected: Vec<OscIdentity> = bodies.iter().map(|b| reference_identity(b)).collect();
        let started = std::time::Instant::now();
        let actual: Vec<OscIdentity> = bodies.iter().map(|b| osc_body_identity(b)).collect();
        let elapsed = started.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "identifying {} million-digit bodies took {elapsed:?}",
            bodies.len()
        );
        assert_eq!(actual, expected);
    }

    // ---- osc7501-leading-zero-length task0002 AC-1 / AC-4 (FR3, FR4, NFR1,
    //      NFR3): the allocation-free number entry ----

    /// The number the shared rule (SC-1) gives for bodies of every spelling
    /// the feature names: leading zeros, non-digit bytes before the first
    /// `;`, no `;`, no digits, and the `u16` boundary.
    #[test]
    fn leadzero_ac4_the_number_entry_reports_the_shared_rule_number() {
        let cases: &[(&[u8], Option<u16>)] = &[
            (b"7501;state=idle", Some(7501)),
            (b"07501;state=idle", Some(7501)),
            (b"0000007501;state=idle", Some(7501)),
            (b"x7501;:state=clear", Some(7501)),
            (b"7501x;state=clear", Some(7501)),
            (b"75x01;?", Some(7501)),
            (b"7501", Some(7501)),
            (b"7501;", Some(7501)),
            (b"133;A", Some(133)),
            (b"0133;A", Some(133)),
            (b"133A", Some(133)),
            (b"1A33", Some(133)),
            (b"", Some(0)),
            (b"hello", Some(0)),
            (b";7501", Some(0)),
            (b"0;7501;?", Some(0)),
            (b"65535;x", Some(65535)),
            (b"065535;x", Some(65535)),
            (b"65536;x", None),
            (b"655367501;state=error", None),
            (b"999999999;?", None),
            (b"99999999999999999999", None),
        ];
        for &(body, expected) in cases {
            assert_eq!(
                osc_body_number(body),
                expected,
                "number of {:?}",
                String::from_utf8_lossy(body)
            );
        }
    }

    /// The number entry returns exactly the number the reference recovery
    /// reconstructs, over the whole identification corpus.
    #[test]
    fn leadzero_ac4_the_number_entry_agrees_with_the_reference_over_the_corpus() {
        for body in identification_corpus() {
            assert_eq!(
                osc_body_number(&body),
                recover_osc(&body).number,
                "body {:?}",
                String::from_utf8_lossy(&body[..body.len().min(64)])
            );
        }
    }

    /// The number entry makes no heap allocation over the corpus (the
    /// control shows the counter sees the reference recovery's allocation).
    #[test]
    fn leadzero_ac4_the_number_entry_is_allocation_free() {
        let corpus = identification_corpus();
        let expected: Vec<Option<u16>> = corpus.iter().map(|b| recover_osc(b).number).collect();

        let (recovered, control) = alloc_counter::measure(|| recover_osc(b"07501;state=idle"));
        assert!(
            control >= 1,
            "the allocation counter recorded {control} requests around the reference recovery"
        );
        drop(recovered);

        let mut total_requests = 0usize;
        for (body, expected) in corpus.iter().zip(&expected) {
            let (actual, requests) = alloc_counter::measure(|| osc_body_number(body));
            total_requests += requests;
            assert_eq!(
                actual,
                *expected,
                "body {:?}",
                String::from_utf8_lossy(&body[..body.len().min(64)])
            );
        }
        assert_eq!(
            total_requests,
            0,
            "the number entry allocated over a corpus of {} bodies",
            corpus.len()
        );
    }

    /// One-MiB and one-million-digit bodies finish within two seconds and
    /// agree with the reference.
    #[test]
    fn leadzero_ac4_the_number_entry_finishes_large_bodies_within_two_seconds() {
        const MIB: usize = 1024 * 1024;
        let mut bodies: Vec<Vec<u8>> = vec![
            vec![b'x'; MIB],
            vec![b'0'; MIB],
            vec![b';'; MIB],
            vec![INVALID; MIB],
            vec![b'1'; 1_000_000],
        ];
        let mut zeros_then_status = vec![b'0'; 1_000_000];
        zeros_then_status.extend_from_slice(b"7501;?");
        bodies.push(zeros_then_status);
        let mut overflow_then_status = vec![b'9'; 1_000_000];
        overflow_then_status.extend_from_slice(b";7501");
        bodies.push(overflow_then_status);

        let expected: Vec<Option<u16>> = bodies.iter().map(|b| recover_osc(b).number).collect();
        let started = std::time::Instant::now();
        let actual: Vec<Option<u16>> = bodies.iter().map(|b| osc_body_number(b)).collect();
        let elapsed = started.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "numbering {} large bodies took {elapsed:?}",
            bodies.len()
        );
        assert_eq!(actual, expected);
    }
}
