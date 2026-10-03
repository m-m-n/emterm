//! mux-strip-join-escape-closure task0001 (FR1, FR2, NFR3): the join the write
//! strip and the snapshot strip leave behind never completes an escape the raw
//! stream never made.
//!
//! mux-strip-concat-query-closure D1 (`Written::close_before_removal`) already
//! writes one DEL before a construct removed while the written stream is inside a
//! CSI or right after a written lone ESC. These tests pin that behavior against
//! two of the three join attack scenarios of the task (scenario 1, `ESC[6` + a
//! removed construct + `n`, is pinned by
//! `strip_concat_query::strip_concat_one_call_closes_an_open_csi_at_every_removed_construct`):
//!
//! - scenario 2, `ESC ESC ESC[6n[6n[5n` through the write strip, the snapshot
//!   strip and term_core ([`join_closure_two_stage_strip_gives_zero_responses`]);
//! - scenario 3, `ESC` + a removed construct + `c` (RIS) or `(0` (G0
//!   designation) ([`join_closure_a_removal_after_a_lone_esc_neither_resets_nor_switches_g0_nor_answers`]).
//!
//! Oracle convention (SPEC NFR3): the reference is term_core fed the raw stream,
//! with the removed construct's own effect kept out (`view_after_a_cut`: feed the
//! raw `ESC` + construct, discard what that part produced, then feed the
//! continuation). Rows, cursor and responses are compared.
//!
//! The predecessor closure exists, so the two scenario tests pass from their
//! first run and no red stage can be observed (REQUIREMENTS.md A6). The three
//! `join_closure_control_*` tests show that each assertion can detect the attack
//! it guards against.

use super::round3_write_path::DIMS;
use super::round4_cut_csi::{text, view_after_a_cut, view_of};
use super::strip_concat_query::TARGETS;
use super::*;
use crate::mux::snapshot_bytes::build_snapshot_bytes;
use std::time::{Duration, Instant};

const BUDGET: Duration = Duration::from_secs(10);

const ESC: u8 = 0x1b;
const DEL: u8 = 0x7f;

/// What `build_snapshot_bytes` puts before and after the stripped scrollback of a
/// main-buffer pane (`snapshot_bytes::tests::build_snapshot_bytes_main_buffer_omits_screen_part`
/// pins the layout).
const SNAPSHOT_HEAD: &[u8] = b"\x1b[3J\x1b[H\x1b[2J";
const SNAPSHOT_TAIL: &[u8] = b"\x1b[?1049l";

/// Visible text before the `ESC`. A reset (RIS) erases it, so it makes a reset
/// observable. It ends the row, so a re-emitted CR cannot overwrite it.
const PREFIX: &[u8] = b"keep-me\r\n";
const PREFIX_ROW: &str = "keep-me";

/// Every character is one the DEC Special Graphics set maps to a line-drawing
/// glyph (`lqkxmj` draws a box: upper left, horizontal, upper right, vertical,
/// lower left, lower right).
const PROBE: &[u8] = b"lqkxmj";

/// The continuations that follow the removed construct: the single byte `c` and
/// `(0` followed by the probe text.
fn continuations() -> [(&'static str, Vec<u8>); 2] {
    [
        ("c", b"c".to_vec()),
        ("(0 + probe text", [&b"(0"[..], PROBE].concat()),
    ]
}

/// The write strip: `fed` in one cut-free call to one filter. Returns the ring.
fn write_strip(fed: &[u8], ctx: &str) -> Vec<u8> {
    let mut filter = ScrollbackWriteFilter::new();
    let ring = filter.feed(fed, DIMS).1;
    assert!(
        filter.pending().is_empty(),
        "{ctx}: the filter holds {:?}",
        text(filter.pending())
    );
    ring
}

/// The snapshot strip: the snapshot assembly entry point over `ring`. Returns the
/// whole payload and the stripped scrollback part of it.
#[track_caller]
fn snapshot_strip(ring: &[u8], ctx: &str) -> (Vec<u8>, Vec<u8>) {
    let (payload, _segments) = build_snapshot_bytes(ring, &[], b"", false, DIMS);
    let stripped = payload
        .strip_prefix(SNAPSHOT_HEAD)
        .and_then(|rest| rest.strip_suffix(SNAPSHOT_TAIL))
        .unwrap_or_else(|| panic!("{ctx}: unexpected snapshot layout {:?}", text(&payload)))
        .to_vec();
    (payload, stripped)
}

// ── AC-1 (FR1, NFR3, TM-1): scenario 2 ───────────────────────────────────

/// AC-1 (FR1, NFR3, TM-1): `ESC ESC ESC[6n[6n[5n` in one cut-free write-filter
/// call leaves `ESC ESC DEL [6n[5n` in the ring. The snapshot strip leaves those
/// bytes unchanged, and replaying the whole snapshot payload in a fresh term_core
/// gives an empty response buffer (the `[6n[5n` is text). `strip_concat_query`'s
/// reattach helper replays through the snapshot but never asserts the empty
/// response, so it is asserted here.
#[test]
fn join_closure_two_stage_strip_gives_zero_responses() {
    let start = Instant::now();
    let ctx = "ESC ESC ESC[6n[6n[5n";
    let fed: &[u8] = b"\x1b\x1b\x1b[6n[6n[5n";
    assert_eq!(fed.len(), 12);

    // Stage 1: the write strip removes the third ESC with the query and writes
    // one DEL where it stood.
    let ring = write_strip(fed, ctx);
    let expected_ring = [&[ESC, ESC, DEL][..], b"[6n[5n"].concat();
    assert_eq!(expected_ring.len(), 9);
    assert_eq!(CSI_CLOSING, [DEL], "the closing is one DEL");
    assert!(
        ring == expected_ring,
        "{ctx}: the ring holds {:?}, expected {:?}",
        text(&ring),
        text(&expected_ring)
    );

    // Stage 2: the snapshot strip finds nothing to remove.
    let (payload, stripped) = snapshot_strip(&ring, ctx);
    assert!(
        stripped == ring,
        "{ctx}: the snapshot strip changed the ring {:?} into {:?}",
        text(&ring),
        text(&stripped)
    );

    // Replay: the whole snapshot payload in a fresh term_core.
    let replayed = view_of(&payload);
    assert!(
        replayed.responses.is_empty(),
        "{ctx}: replaying the snapshot answers {:?}",
        replayed.responses
    );
    assert_eq!(
        replayed.rows[0], "[6n[5n",
        "{ctx}: the bytes after the removal are text"
    );

    // The oracle: the raw stream with the removed query's own answer excluded.
    let reference = view_after_a_cut(b"\x1b\x1b\x1b[6n", b"[6n[5n");
    assert!(reference.responses.is_empty(), "{ctx}");
    assert_eq!(replayed.rows, reference.rows, "{ctx}: rows");
    assert_eq!(replayed.cursor, reference.cursor, "{ctx}: cursor");
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

// ── AC-2 (FR2, NFR3, TM-2): scenario 3 ───────────────────────────────────

/// AC-2 (FR2, NFR3, TM-2): visible prefix text, `ESC`, a removed construct of
/// each of the eight kinds, then `c` or `(0` + probe text, in one cut-free
/// write-filter call, then the snapshot strip, then replay in a fresh term_core.
/// The ring holds exactly one DEL where the construct stood. Rows, cursor and
/// responses equal the raw-stream reference with the construct's own effect
/// excluded; the prefix is still on screen (no reset), the continuation is
/// displayed as typed (G0 not switched), and the replay gives no response.
#[test]
fn join_closure_a_removal_after_a_lone_esc_neither_resets_nor_switches_g0_nor_answers() {
    let start = Instant::now();
    assert_eq!(TARGETS.len(), 8, "the eight removed construct kinds");
    for target in TARGETS {
        for (cont_name, cont) in continuations() {
            let ctx = format!("{} + {cont_name}", target.name);
            let raw_before = [PREFIX, &[ESC][..], target.bytes].concat();
            let fed = [&raw_before[..], &cont[..]].concat();

            // Write strip: one DEL where the construct stood, before any C0 byte
            // the construct re-emits.
            let ring = write_strip(&fed, &ctx);
            let expected_ring = [PREFIX, &[ESC][..], CSI_CLOSING, target.c0, &cont[..]].concat();
            assert!(
                ring == expected_ring,
                "{ctx}: the ring holds {:?}, expected {:?}",
                text(&ring),
                text(&expected_ring)
            );

            // Snapshot strip: nothing more to remove.
            let (payload, stripped) = snapshot_strip(&ring, &ctx);
            assert!(
                stripped == ring,
                "{ctx}: the snapshot strip changed the ring {:?} into {:?}",
                text(&ring),
                text(&stripped)
            );

            // Replay against the oracle.
            let replayed = view_of(&payload);
            let reference = view_after_a_cut(&raw_before, &cont);
            assert!(
                reference.responses.is_empty(),
                "{ctx}: the raw stream answers {:?} after the construct",
                reference.responses
            );
            assert!(
                replayed.responses.is_empty(),
                "{ctx}: replaying the snapshot answers {:?}",
                replayed.responses
            );
            assert_eq!(replayed.responses, reference.responses, "{ctx}: responses");
            assert_eq!(replayed.rows, reference.rows, "{ctx}: rows");
            assert_eq!(replayed.cursor, reference.cursor, "{ctx}: cursor");

            // Observable outcomes: no reset, and the continuation as typed.
            assert_eq!(
                replayed.rows[0], PREFIX_ROW,
                "{ctx}: the prefix is gone, the terminal was reset"
            );
            assert_eq!(
                replayed.rows[1],
                text(&cont),
                "{ctx}: the continuation is not displayed as typed"
            );
        }
    }
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

// ── AC-7 (FR1, FR2; TM-1, TM-2): sensitivity controls ────────────────────

/// AC-7 control for AC-1: `ESC[5n` is the query a closure-less two-stage strip
/// joins out of `ESC ESC ESC[6n[6n[5n`. The replay setup (`view_of`) answers it,
/// so the empty response buffer of AC-1 is evidence of the closure.
#[test]
fn join_closure_control_the_replay_core_answers_the_joined_query() {
    let joined = view_of(b"\x1b[5n");
    assert!(
        !joined.responses.is_empty(),
        "the replay core must answer ESC[5n for the AC-1 assertion to detect the attack"
    );
    // The same core shows the closed form as text.
    let closed = view_of(b"\x1b\x1b\x7f[6n[5n");
    assert!(closed.responses.is_empty());
}

/// AC-7 control for AC-2 (a): prefix text followed by `ESC c`, the closure-less
/// join of continuation `c`, replays with the prefix gone.
#[test]
fn join_closure_control_a_closure_less_join_with_c_resets_the_terminal() {
    let before = view_of(PREFIX);
    assert_eq!(before.rows[0], PREFIX_ROW, "the prefix is visible");

    let joined = view_of(&[PREFIX, &[ESC][..], b"c"].concat());
    assert!(
        !joined.rows.iter().any(|row| row.contains(PREFIX_ROW)),
        "ESC c must reset the terminal and erase the prefix; rows {:?}",
        joined.rows
    );
}

/// AC-7 control for AC-2 (b): `ESC(0` followed by the probe text, the
/// closure-less join of continuation `(0` + probe text, displays characters that
/// differ from the typed probe text: every probe character is drawn as a
/// line-drawing glyph.
#[test]
fn join_closure_control_a_closure_less_join_with_a_designation_switches_g0() {
    let joined = view_of(&[&[ESC][..], b"(0", PROBE].concat());
    let row = &joined.rows[0];
    assert_ne!(row, &text(PROBE), "the probe text is shown as typed");
    assert_eq!(
        row.chars().count(),
        PROBE.len(),
        "one glyph per probe character; row {row:?}"
    );
    assert!(
        row.chars().all(|ch| !ch.is_ascii()),
        "every probe character must be drawn as a line-drawing glyph; row {row:?}"
    );
}
