use super::*;

// ── merge_scrollback_from (scrollback restore) ───────

/// Build a TerminalCore that has `n_rows` rows of red "X" content in its
/// scrollback, using `cols`-wide cells. Convenience for the merge tests.
fn make_core_with_red_x_scrollback(cols: u16, n_rows: u32) -> TerminalCore {
    let mut core = TerminalCore::new(cols, 4, n_rows + 10);
    // Use a non-default fg color so each cell goes through
    // `styles.intern` with a fresh StyleEntry — that is what TS-1
    // checks (the re-intern actually rewrites style_id).
    // 0x1b 5b "31" m = set fg red.
    let mut payload: Vec<u8> = Vec::new();
    for _ in 0..n_rows {
        payload.extend_from_slice(b"\x1b[31mX\x1b[m\r\n");
    }
    core.process_pty_data_fully(&payload);
    core
}

/// TS-1 (FR2): `merge_scrollback_from` re-interns SlimCell ids against
/// the receiver's tables so the merged row resolves to byte-equal style
/// / char entries even when the two cores' intern tables differ.
#[test]
fn test_merge_scrollback_from_intern_rewrites_ids() {
    let mut dst = TerminalCore::new(80, 24, 100);
    // Prime dst.styles with a couple of unrelated entries so id slots
    // are unlikely to coincide with src's by accident (the test would
    // still hold under id-equality by luck).
    for ch in b"abcde" {
        let mut cell = crate::cell::Cell::EMPTY;
        cell.set_char(&(*ch as char).to_string());
        cell.fg = crate::cell::PackedColor::rgb(*ch, 0, 0);
        crate::slim_cell::cell_to_slim(&cell, None, &mut dst.styles, &mut dst.chars);
    }

    let src = make_core_with_red_x_scrollback(80, 6);
    assert!(
        !src.scrollback_slim.is_empty(),
        "src must have non-empty scrollback for the test to be meaningful"
    );

    // Snapshot src's first row's fg before consuming it.
    let src_first_row_fg = {
        let row = src.scrollback_slim.front().unwrap();
        let style = src.styles.get_or_default(row[0].style_id);
        (style.fg, style.bg, style.flags)
    };

    dst.merge_scrollback_from(src, 0);

    // The merged row should appear at the front of dst's scrollback,
    // and resolving its cell against dst.styles must yield the same
    // (fg, bg, flags) tuple — proving the style_id was re-interned
    // against dst.styles to a slot that holds an equivalent entry.
    let merged_row = dst
        .scrollback_slim
        .front()
        .expect("merge made the front non-empty");
    let merged_style = dst.styles.get_or_default(merged_row[0].style_id);
    assert_eq!(
        (merged_style.fg, merged_style.bg, merged_style.flags),
        src_first_row_fg,
        "merged row's style_id must resolve to the same fg/bg/flags via dst.styles"
    );
}

/// TS-2 (NFR5): the merge MUST NOT touch
/// `self.scrollback_evicted_total`. The merged rows pre-date the bypass
/// swap and the live-side delta accounting already covers them.
#[test]
fn test_merge_scrollback_from_preserves_evicted_total() {
    let mut dst = TerminalCore::new(80, 24, 4);
    // Push enough lines that the scrollback ring saturates and the
    // counter has a non-zero baseline.
    let mut bytes: Vec<u8> = Vec::new();
    for _ in 0..30 {
        bytes.extend_from_slice(b"y\r\n");
    }
    dst.process_pty_data_fully(&bytes);
    let evicted_before = dst.scrollback_evicted_total;
    assert!(
        evicted_before > 0,
        "test prerequisite: dst should have a non-zero evicted baseline"
    );

    let src = make_core_with_red_x_scrollback(80, 3);
    dst.merge_scrollback_from(src, 0);

    assert_eq!(
        dst.scrollback_evicted_total, evicted_before,
        "merge must not bump scrollback_evicted_total"
    );
}

/// TS-4 (FR2 defensive): cols mismatch ⇒ no-op (no merge, no panic).
#[test]
fn test_merge_scrollback_from_cols_mismatch_is_noop() {
    let mut dst = TerminalCore::new(80, 24, 100);
    // Push some live content so dst's scrollback has rows to compare
    // before vs. after the noop merge.
    dst.process_pty_data_fully(b"AAA\r\nBBB\r\nCCC\r\n");
    let snapshot_slim: Vec<Vec<crate::slim_cell::SlimCell>> =
        dst.scrollback_slim.iter().cloned().collect();
    let snapshot_wrapped: Vec<bool> = dst.scrollback_wrapped.iter().copied().collect();
    let snapshot_evicted = dst.scrollback_evicted_total;

    // src at a different cols width.
    let src = make_core_with_red_x_scrollback(100, 5);
    dst.merge_scrollback_from(src, 0);

    let after_slim: Vec<Vec<crate::slim_cell::SlimCell>> =
        dst.scrollback_slim.iter().cloned().collect();
    let after_wrapped: Vec<bool> = dst.scrollback_wrapped.iter().copied().collect();
    assert_eq!(
        after_slim, snapshot_slim,
        "scrollback rows must be unchanged"
    );
    assert_eq!(after_wrapped, snapshot_wrapped);
    assert_eq!(dst.scrollback_evicted_total, snapshot_evicted);
}

/// TS-6 (FR1 / NFR6 — primary equivalence gate): bypass-on
/// `build_from_snapshot` + bypass-off `build_scrollback_only_from_snapshot`
/// + `merge_scrollback_from` with `live_growth = 0` settles in a state
/// observably equal to a single bypass-off build.
///
/// This is the unit-level proof that the 2nd-pass restore worker plus
/// the merge primitive is a drop-in replacement for the synchronous
/// reset_and_replay path.
#[test]
fn test_bypass_plus_merge_equivalence() {
    // Payload that scrolls more than the viewport so scrollback has
    // non-trivial content to compare.
    let mut payload: Vec<u8> = Vec::new();
    payload.extend_from_slice(b"\x1b]133;A\x07$ ls\x1b]133;B\x07hello\r\n");
    for i in 0..40u32 {
        payload.extend_from_slice(format!("scroll {i}\r\n").as_bytes());
    }

    // Reference: single synchronous bypass-off build.
    let never = std::sync::atomic::AtomicBool::new(false);
    let reference =
        TerminalCore::build_scrollback_only_from_snapshot(80, 24, 100, &payload, &[], &never)
            .expect("reference build not cancelled");

    // Production path: bypass-on 1st-pass + bypass-off 2nd-pass + merge.
    let bypass_replay =
        TerminalCore::build_from_snapshot(80, 24, 100, &payload, &[], &never).expect("1st-pass");
    let mut live = bypass_replay.core;
    // Bypass leaves scrollback empty by design.
    assert_eq!(live.scrollback_count(), 0);
    let rebuilt =
        TerminalCore::build_scrollback_only_from_snapshot(80, 24, 100, &payload, &[], &never)
            .expect("2nd-pass");
    // live_growth == 0: no trim necessary, merge whole rebuilt scrollback.
    live.merge_scrollback_from(rebuilt.core, 0);

    // Equivalence vs. the synchronous reference.
    assert_eq!(
        grid_fingerprint(&live),
        grid_fingerprint(&reference.core),
        "viewport grid must match"
    );
    assert_eq!(
        live.scrollback_count(),
        reference.core.scrollback_count(),
        "scrollback row count must match the synchronous reference"
    );
    // scrollback_evicted_total: both code paths produce the same
    // bypass-driven baseline (the 1st-pass produced the baseline; the
    // merge must NOT bump it). For a payload that does not saturate
    // the scrollback ring this is 0, but the contract is that the
    // counter is byte-identical to the reference regardless of saturation.
    assert_eq!(
        live.scrollback_evicted_total,
        reference.core.scrollback_evicted_total
    );
    // Cell-by-cell scrollback equality (decompressed view, robust to
    // intern slot reassignment).
    for (row_idx, (l, r)) in live
        .scrollback_slim
        .iter()
        .zip(reference.core.scrollback_slim.iter())
        .enumerate()
    {
        assert_eq!(l.len(), r.len(), "row {row_idx} length");
        for (col_idx, (sa, sb)) in l.iter().zip(r.iter()).enumerate() {
            let ca = crate::slim_cell::slim_to_cell(sa, &live.styles, &live.chars);
            let cb =
                crate::slim_cell::slim_to_cell(sb, &reference.core.styles, &reference.core.chars);
            assert_eq!(
                (ca.char_data, ca.char_len, ca.width, ca.fg, ca.bg, ca.flags),
                (cb.char_data, cb.char_len, cb.width, cb.fg, cb.bg, cb.flags),
                "row {row_idx} col {col_idx}"
            );
        }
    }
}

/// review round-1 rework, finding `1698d9b52a89e241` (medium,
/// correctness-relevant) / task0002 AC-7: a snapshot >= 64 KiB
/// containing a ROW-COUNT-SHRINKING marker produces no
/// duplicated / out-of-order scrollback rows and reports the SAME
/// eviction bookkeeping as a fully synchronous (bypass-off) replay of
/// the same payload — not merely "close".
///
/// task0003 D6 update (review round-2 finding `893241823258fce3`): this
/// payload's setup needs a GROW step before the shrink under test (to
/// produce grown-size content for the shrink to push into scrollback).
/// `build_from_snapshot_inner`'s D6 pre-scan sees that grow and
/// downgrades the WHOLE replay out of the bypass fast path (see that
/// function's doc comment), so `build_from_snapshot` alone now already
/// returns the complete, correct scrollback for this payload — the
/// former "manually run the 2nd-pass rebuild + merge, then assert the
/// 1st-pass core was left empty by the bypass" recipe no longer
/// applies (there is nothing left for a 2nd pass to add; that
/// combination is covered instead by `test_bypass_plus_merge_equivalence`
/// for a payload that genuinely stays bypassed). What still matters —
/// and what this test still proves — is that the RESULT is correct: no
/// duplicated / dropped rows and byte-identical eviction bookkeeping
/// against the synchronous reference.
#[test]
fn test_bypass_plus_merge_equivalence_across_row_shrinking_resize_marker() {
    let cols: u16 = 80;
    let small_rows: u16 = 10;
    let grown_rows: u16 = 24;
    let mut payload: Vec<u8> = Vec::new();
    let mut segments: Vec<ReplaySegment> = Vec::new();
    // A handful of lines that fit within the small viewport with no
    // eviction yet, so the upcoming grow needs no history bypass would
    // have discarded.
    segments.push(ReplaySegment {
        offset: payload.len() as u32,
        cols,
        rows: small_rows,
    });
    for i in 0..5u32 {
        payload.extend_from_slice(format!("early line {i}\r\n").as_bytes());
    }
    segments.push(ReplaySegment {
        offset: payload.len() as u32,
        cols,
        rows: grown_rows,
    });
    // Bulk content at the grown size — large enough to comfortably
    // exceed 64 KiB and to populate substantial (virtual, under
    // bypass) scrollback before the shrink.
    for i in 0..3000u32 {
        payload.extend_from_slice(
            format!("grown-size scroll line {i} padded for size\r\n").as_bytes(),
        );
    }
    // The row-count-SHRINKING transition under test (AC-7): pushes the
    // rows that no longer fit the smaller viewport into scrollback —
    // exactly the content-preserving reflow finding `1698d9b52a89e241`
    // is about.
    segments.push(ReplaySegment {
        offset: payload.len() as u32,
        cols,
        rows: small_rows,
    });
    // A few lines after the shrink so the range following the
    // transition is non-empty (the resize is actually applied) — this
    // also leaves the core already at `small_rows`, matching the
    // construction / target size.
    for i in 0..5u32 {
        payload.extend_from_slice(format!("after-shrink line {i}\r\n").as_bytes());
    }
    assert!(
        payload.len() >= 64 * 1024,
        "payload must be >= 64 KiB to match AC-7's off-thread-path scenario, got {}",
        payload.len()
    );

    let never = std::sync::atomic::AtomicBool::new(false);

    // Reference: single synchronous bypass-off build.
    let reference = TerminalCore::build_scrollback_only_from_snapshot(
        cols, small_rows, 5000, &payload, &segments, &never,
    )
    .expect("reference build not cancelled");

    // Under test: `build_from_snapshot` — D6 downgrades it out of the
    // bypass for this payload (it contains a growing transition), so
    // its result alone must already match the synchronous reference.
    let bypass_replay =
        TerminalCore::build_from_snapshot(cols, small_rows, 5000, &payload, &segments, &never)
            .expect("build not cancelled");
    let live = bypass_replay.core;

    assert_eq!(
        grid_fingerprint(&live),
        grid_fingerprint(&reference.core),
        "viewport grid must match the synchronous reference"
    );
    assert_eq!(
        live.scrollback_count(),
        reference.core.scrollback_count(),
        "scrollback row count must match — no duplicated / dropped rows from a bypass leak"
    );
    assert_eq!(
        live.scrollback_evicted_total, reference.core.scrollback_evicted_total,
        "eviction bookkeeping must be byte-identical, not merely close"
    );
    for (row_idx, (l, r)) in live
        .scrollback_slim
        .iter()
        .zip(reference.core.scrollback_slim.iter())
        .enumerate()
    {
        assert_eq!(l.len(), r.len(), "row {row_idx} length");
        for (col_idx, (sa, sb)) in l.iter().zip(r.iter()).enumerate() {
            let ca = crate::slim_cell::slim_to_cell(sa, &live.styles, &live.chars);
            let cb =
                crate::slim_cell::slim_to_cell(sb, &reference.core.styles, &reference.core.chars);
            assert_eq!(
                (ca.char_data, ca.char_len, ca.width, ca.fg, ca.bg, ca.flags),
                (cb.char_data, cb.char_len, cb.width, cb.fg, cb.bg, cb.flags),
                "row {row_idx} col {col_idx}"
            );
        }
    }
}
