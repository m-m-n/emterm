use super::*;

#[test]
fn test_empty_buffer() {
    let rb = ScrollbackRingBuffer::new(1024);
    assert!(rb.is_empty());
    assert_eq!(rb.len(), 0);
    assert_eq!(rb.read_all(), Vec::<u8>::new());
}

#[test]
fn test_simple_write_read() {
    let mut rb = ScrollbackRingBuffer::new(1024);
    rb.write(b"hello");
    assert_eq!(rb.len(), 5);
    assert_eq!(rb.read_all(), b"hello");
}

#[test]
fn test_multiple_writes() {
    let mut rb = ScrollbackRingBuffer::new(1024);
    rb.write(b"hello ");
    rb.write(b"world");
    assert_eq!(rb.len(), 11);
    assert_eq!(rb.read_all(), b"hello world");
}

#[test]
fn test_wrap_around() {
    let mut rb = ScrollbackRingBuffer::new(8);
    rb.write(b"ABCDEF"); // 6 bytes, pos=6
    rb.write(b"GHI"); // wraps: pos=1, overwrites first byte
    assert_eq!(rb.len(), 8); // capped at capacity
    assert_eq!(rb.read_all(), b"BCDEFGHI");
}

#[test]
fn test_overflow_large_write() {
    let mut rb = ScrollbackRingBuffer::new(4);
    rb.write(b"ABCDEFGH"); // larger than capacity, keeps last 4
    assert_eq!(rb.len(), 4);
    assert_eq!(rb.read_all(), b"EFGH");
}

#[test]
fn test_exact_capacity() {
    let mut rb = ScrollbackRingBuffer::new(4);
    rb.write(b"ABCD");
    assert_eq!(rb.len(), 4);
    assert_eq!(rb.read_all(), b"ABCD");
}

#[test]
fn test_clear() {
    let mut rb = ScrollbackRingBuffer::new(1024);
    rb.write(b"data");
    rb.clear();
    assert!(rb.is_empty());
    assert_eq!(rb.read_all(), Vec::<u8>::new());
}

#[test]
fn test_write_after_clear() {
    let mut rb = ScrollbackRingBuffer::new(1024);
    rb.write(b"old data");
    rb.clear();
    rb.write(b"new data");
    assert_eq!(rb.read_all(), b"new data");
}

#[test]
fn test_capacity() {
    let rb = ScrollbackRingBuffer::new(4096);
    assert_eq!(rb.capacity(), 4096);
}

/// Perf bench: time `read_all()` on a 2 MiB ring filled past capacity (so
/// the wrap-around branch is exercised). This runs once per mux snapshot
/// rebuild — `handle_request_pane_snapshot` / `reattach::build_snapshot_bytes`
/// call it on the full pane scrollback every tab switch.
///
/// Gated `#[ignore]`. Invoke with:
///
/// ```sh
/// CARGO_TARGET_DIR=src-tauri/target cargo test --release \
///   --manifest-path src-tauri/Cargo.toml --lib --features gui \
///   scrollback_read_all_bench_2mib_wrapped \
///   -- --nocapture --include-ignored
/// ```
#[test]
#[ignore]
fn scrollback_read_all_bench_2mib_wrapped() {
    use std::time::Instant;
    let cap = 2 * 1024 * 1024;
    let mut rb = ScrollbackRingBuffer::new(cap);
    // Fill 3x capacity in 4 KiB chunks so the ring wraps and write_pos
    // lands somewhere in the middle (mimics a long-running pane).
    let chunk = vec![b'x'; 4096];
    let total_writes = (cap * 3) / chunk.len();
    for _ in 0..total_writes {
        rb.write(&chunk);
    }
    // Warm-up.
    for _ in 0..2 {
        let _ = rb.read_all();
    }
    let iters = 50;
    let start = Instant::now();
    for _ in 0..iters {
        let out = rb.read_all();
        std::hint::black_box(out);
    }
    let elapsed = start.elapsed();
    let per = elapsed / iters as u32;
    eprintln!(
        "[bench] ScrollbackRingBuffer::read_all 2MiB wrapped: {iters} iters / {:?} → {:?}/call ({:.1} MiB/s)",
        elapsed,
        per,
        (2.0 * iters as f64) / elapsed.as_secs_f64(),
    );
    // SPEC.md "Performance Goals" (FR5): a plain Vec copy of 2 MiB
    // must stay under 1 ms — if this regresses, the daemon-side ring
    // read has become a bottleneck of its own.
    let threshold = std::time::Duration::from_millis(1);
    assert!(
        per < threshold,
        "ScrollbackRingBuffer::read_all per-call {:?} ≥ threshold {:?} (FR5)",
        per,
        threshold,
    );
}

#[test]
fn test_repeated_small_writes_overflow() {
    let mut rb = ScrollbackRingBuffer::new(8);
    for i in 0..20u8 {
        rb.write(&[i]);
    }
    assert_eq!(rb.len(), 8);
    // Last 8 bytes: 12,13,14,15,16,17,18,19
    assert_eq!(rb.read_all(), vec![12, 13, 14, 15, 16, 17, 18, 19]);
}

// ── task0004 round-4 rework (D1'): dim_markers is the SOLE authority,
// exposed structurally via read_segments() — read_all() never contains
// any marker-shaped bytes, at all, regardless of write_resize_marker /
// attribute_write calls ──────────────────────────────────────────────

/// AC-9 (review round-3 finding `ef69658e6e4d0b05`): `read_all()` is
/// completely unaffected by any number of `write_resize_marker` /
/// `attribute_write` calls — it returns EXACTLY the bytes passed to
/// `write` / `attribute_write`'s `data` parameter, in order, with zero
/// synthesized bytes of any kind. This is what makes `mux-read`
/// (`handle_read_pane`, which calls `read_all()` directly) immune to
/// ever seeing a protocol remnant: there is nothing to leak because
/// nothing is ever inserted.
#[test]
fn read_all_never_contains_any_synthesized_bytes_regardless_of_resize_calls() {
    let mut rb = ScrollbackRingBuffer::new(4096);
    rb.write_resize_marker(80, 24);
    rb.write(b"before");
    rb.write_resize_marker(65535, 65535); // extreme dims — would be highly visible if leaked
    rb.attribute_write(120, 40, b"middle");
    rb.write_resize_marker(1, 1);
    rb.write(b"after");
    assert_eq!(rb.read_all(), b"beforemiddleafter");
}

/// A ring that never calls `write_resize_marker` behaves exactly as a
/// plain byte ring, even after wraparound.
#[test]
fn read_all_without_any_resize_marker_is_unaffected_by_wraparound() {
    let mut rb = ScrollbackRingBuffer::new(8);
    rb.write(b"ABCDEF");
    rb.write(b"GHI"); // wraps
    assert_eq!(rb.read_all(), b"BCDEFGHI");
}

/// `read_segments()` reports the raw bytes (identical to `read_all()`)
/// paired with the segment for the ring's only recorded resize, at
/// position 0 (the whole retained window is under those dims).
#[test]
fn read_segments_matches_read_all_bytes_and_reports_head_segment() {
    let mut rb = ScrollbackRingBuffer::new(1024);
    rb.write_resize_marker(80, 24);
    rb.write(b"hello");
    let (bytes, segments) = rb.read_segments();
    assert_eq!(bytes, rb.read_all());
    assert_eq!(bytes, b"hello");
    assert_eq!(segments, vec![(0usize, 80u16, 24u16)]);
}

/// A ring that never calls `write_resize_marker` reports NO segments —
/// the caller's replay degrades to single-dimension (task0004 AC-11).
#[test]
fn read_segments_reports_no_segments_when_ring_never_recorded_a_resize() {
    let mut rb = ScrollbackRingBuffer::new(1024);
    rb.write(b"plain content, no resize ever recorded");
    let (bytes, segments) = rb.read_segments();
    assert_eq!(bytes, rb.read_all());
    assert!(segments.is_empty());
}

/// Driving the ring past wraparound so the INITIAL marker's offset falls
/// before the retained window still yields a `read_segments()` head
/// segment (position 0) describing the dimensions in effect for
/// whatever content survived — the last marker recorded before the
/// retained window began.
#[test]
fn read_segments_reconstructs_head_segment_after_window_advances_past_it() {
    let capacity = 32;
    let mut rb = ScrollbackRingBuffer::new(capacity);
    rb.write_resize_marker(80, 24);
    // Push enough plain bytes to advance the retained window well past
    // this marker's offset (0).
    rb.write(&vec![b'x'; capacity * 3]);
    let (bytes, segments) = rb.read_segments();
    assert_eq!(bytes, vec![b'x'; capacity]);
    assert_eq!(
        segments,
        vec![(0usize, 80u16, 24u16)],
        "retained window must be preceded by a segment for the dims that \
         produced it, even though its offset now precedes the window"
    );
}

/// Multi-resize variant: after TWO resizes and enough content to advance
/// the retained window past both offsets, the reconstructed head
/// segment reflects the SECOND (most recent surviving) resize's
/// dimensions, not the first.
#[test]
fn read_segments_reconstructs_head_segment_using_latest_surviving_resize() {
    let capacity = 64;
    let mut rb = ScrollbackRingBuffer::new(capacity);
    rb.write_resize_marker(80, 24);
    rb.write(b"some content produced at the first size");
    rb.write_resize_marker(120, 40);
    // Push enough plain bytes to advance the window past both offsets.
    rb.write(&vec![b'y'; capacity * 3]);
    let (bytes, segments) = rb.read_segments();
    assert_eq!(segments, vec![(0usize, 120u16, 40u16)]);
    assert!(
        !bytes
            .windows(b"first size".len())
            .any(|w| w == b"first size"),
        "no trace of the evicted first-resize content should remain: {bytes:?}"
    );
}

/// A resize whose offset falls WITHIN the retained window (not just at
/// its head) is reported at the correct relative position, with the
/// surrounding plain content bytes unaffected.
#[test]
fn read_segments_reports_a_mid_window_segment_at_the_correct_position() {
    let mut rb = ScrollbackRingBuffer::new(4096);
    rb.write(b"before");
    rb.write_resize_marker(100, 40);
    rb.write(b"after");
    let (bytes, segments) = rb.read_segments();
    assert_eq!(bytes, b"beforeafter");
    assert_eq!(segments, vec![("before".len(), 100u16, 40u16)]);
}

/// `clear()` resets the resize bookkeeping too — a subsequent
/// `write_resize_marker` after `clear()` behaves like a fresh ring (no
/// stale offsets from before the clear).
#[test]
fn clear_resets_resize_marker_bookkeeping() {
    let mut rb = ScrollbackRingBuffer::new(1024);
    rb.write_resize_marker(80, 24);
    rb.write(b"old content");
    rb.clear();
    rb.write_resize_marker(100, 30);
    rb.write(b"new content");
    let (bytes, segments) = rb.read_segments();
    assert_eq!(bytes, b"new content");
    assert_eq!(segments, vec![(0usize, 100u16, 30u16)]);
}

// ── task0003 AC-8 (D5, review round-2 finding `0bebe3e6f7b416dd`):
// attribute_write corrects a marker that won the scrollback lock race
// ahead of content produced under different (older) dims ─────────────

/// Simulates the race the finding describes: `MuxPane::resize` wins the
/// scrollback lock first and records its marker for the NEW dims, but
/// the reader thread's chunk — which was actually read (and started
/// processing) BEFORE the resize, so was produced under the OLD dims —
/// only reaches the ring afterward. `attribute_write` must insert a
/// corrective entry for the dims it is TOLD the content was produced
/// under, so that content is not misattributed to the resize's (newer,
/// wrong) dims at replay.
#[test]
fn attribute_write_corrects_a_marker_that_won_the_lock_race_ahead_of_stale_content() {
    let mut rb = ScrollbackRingBuffer::new(4096);
    rb.write_resize_marker(100, 40);
    rb.attribute_write(80, 24, b"stale-dims content");
    let (bytes, segments) = rb.read_segments();
    assert_eq!(bytes, b"stale-dims content");
    assert_eq!(
        segments,
        vec![(0usize, 80u16, 24u16)],
        "content attributed via attribute_write must be preceded by a \
         segment for the dims it was actually produced under, correcting \
         a marker that won the lock race for different dims"
    );
}

/// AC-3 (task0004 round-4 rework, review round-3 finding
/// `9d4f7176af1b264e`): a correction entry for bytes read BEFORE a
/// resize survives SUBSEQUENT recording at the resize's own dims — the
/// three-step sequence the finding names: resize, stale chunk (via
/// `attribute_write` at the OLD dims), then a fresh chunk recorded at
/// the NEW dims. The stale chunk must keep its OLD-dims attribution
/// even after the fresh chunk is recorded.
///
/// Confirmed to fail pre-fix: against the removed
/// `RESIZE_MARKER_COALESCE_MAX_BYTES`-based coalescing (which merged
/// any two `write_resize_marker` calls within 8 KiB of each other by
/// overwriting the EARLIER entry's dimensions in place), the fresh
/// chunk's `write_resize_marker(100, 40)` call — happening right after
/// the short `stale-dims content` write — landed within that window and
/// overwrote the entry this test expects to survive at `(80, 24)`,
/// collapsing both entries into one at `(100, 40)` and misattributing
/// the stale chunk to the new dims.
#[test]
fn attribute_write_correction_survives_a_subsequent_recording_at_new_dims() {
    let mut rb = ScrollbackRingBuffer::new(4096);
    // Step 1: resize records the NEW dims marker.
    rb.write_resize_marker(100, 40);
    // Step 2: a stale chunk (produced before the resize) is attributed
    // to the OLD dims, correcting the marker that won the lock race.
    rb.attribute_write(80, 24, b"stale chunk");
    // Step 3: a fresh chunk, genuinely produced at the NEW dims,
    // arrives next.
    rb.attribute_write(100, 40, b"fresh chunk");

    let (bytes, segments) = rb.read_segments();
    assert_eq!(bytes, b"stale chunkfresh chunk");
    assert_eq!(
        segments,
        vec![(0usize, 80u16, 24u16), ("stale chunk".len(), 100u16, 40u16),],
        "the stale chunk's correction must survive the subsequent \
         fresh-chunk recording, not get swallowed back into the resize's \
         dims"
    );
}

/// The overwhelmingly common case (no concurrent resize): dims already
/// match the ring's last-recorded dims, so `attribute_write` is just a
/// plain write — no extra marker.
#[test]
fn attribute_write_is_a_plain_write_when_dims_already_match() {
    let mut rb = ScrollbackRingBuffer::new(4096);
    rb.write_resize_marker(80, 24);
    rb.attribute_write(80, 24, b"normal content");
    let (bytes, segments) = rb.read_segments();
    assert_eq!(bytes, b"normal content");
    assert_eq!(segments, vec![(0usize, 80u16, 24u16)]);
    assert_eq!(
        rb.dim_markers_len(),
        1,
        "no extra marker when the attributed dims already match"
    );
}

/// `attribute_write` on a ring that never recorded ANY marker (e.g. a
/// bare `ScrollbackRingBuffer` used directly, outside `MuxPane`) still
/// seeds one for the FIRST call's dims — `last_effective_dims()` is
/// `None` initially, which never equals `Some((cols, rows))`.
#[test]
fn attribute_write_seeds_a_marker_on_a_ring_with_no_prior_marker() {
    let mut rb = ScrollbackRingBuffer::new(4096);
    rb.attribute_write(80, 24, b"first content");
    let (bytes, segments) = rb.read_segments();
    assert_eq!(bytes, b"first content");
    assert_eq!(segments, vec![(0usize, 80u16, 24u16)]);
}

// ── task0004 round-4 rework (D2', review round-3 finding
// `ab54fae335086db3`): coalescing reverted to the only sound rule ─────

/// Two `write_resize_marker` calls with literally ZERO bytes recorded
/// between them (the safe case: nothing exists yet to misattribute)
/// coalesce into a single entry, taking the LATEST dimensions.
#[test]
fn write_resize_marker_coalesces_only_at_the_exact_same_offset() {
    let mut rb = ScrollbackRingBuffer::new(4096);
    rb.write_resize_marker(80, 24);
    rb.write_resize_marker(81, 25); // zero bytes since the previous call
    rb.write_resize_marker(82, 26); // zero bytes since the previous call
    assert_eq!(
        rb.dim_markers_len(),
        1,
        "consecutive resize calls with zero intervening bytes must \
         coalesce (nothing was ever recorded under the superseded dims)"
    );
    rb.write(b"content");
    let (_, segments) = rb.read_segments();
    assert_eq!(segments, vec![(0usize, 82u16, 26u16)]);
}

/// ANY real content between two resize calls — even a single byte —
/// must NOT be coalesced away: doing so would misattribute that content
/// to the wrong dimensions (review round-3 finding `ab54fae335086db3`,
/// critical — the round-2 8 KiB-threshold coalescing did exactly this
/// for "negligible" redraws).
///
/// Confirmed to fail pre-fix: against the removed
/// `RESIZE_MARKER_COALESCE_MAX_BYTES` (8 KiB) rule, this single byte of
/// intervening content was well under the threshold, so the second
/// `write_resize_marker` call coalesced into the first — overwriting
/// its dims and misattributing the 1-byte write to (81, 25) instead of
/// the (80, 24) it was actually recorded under.
#[test]
fn write_resize_marker_never_coalesces_across_any_real_content() {
    let mut rb = ScrollbackRingBuffer::new(4096);
    rb.write_resize_marker(80, 24);
    rb.write(b"X"); // a single byte of real content
    rb.write_resize_marker(81, 25);
    assert_eq!(
        rb.dim_markers_len(),
        2,
        "even one byte of real content between two resize calls must \
         prevent coalescing"
    );
    let (bytes, segments) = rb.read_segments();
    assert_eq!(bytes, b"X");
    assert_eq!(
        segments,
        vec![(0usize, 80u16, 24u16), (1usize, 81u16, 25u16)],
        "the single byte must stay attributed to the FIRST dims, not the \
         second"
    );
}

/// AC-2 end-to-end: a resize immediately followed by under-threshold
/// (well under the OLD 8 KiB coalescing window) real output, then
/// another resize, must keep the intervening output under its ORIGINAL
/// dimensions — never retroactively reattributed by a later resize.
#[test]
fn resize_then_small_output_then_resize_keeps_original_attribution() {
    let mut rb = ScrollbackRingBuffer::new(4096);
    rb.write_resize_marker(80, 24);
    rb.write(b"small output under 8KiB");
    rb.write_resize_marker(120, 40);
    rb.write(b"more output");
    let (bytes, segments) = rb.read_segments();
    assert_eq!(bytes, b"small output under 8KiBmore output");
    assert_eq!(
        segments,
        vec![
            (0usize, 80u16, 24u16),
            ("small output under 8KiB".len(), 120u16, 40u16),
        ]
    );
}

// ── task0005 rework D3'' (review round-4 finding `6c650908ea8e95e9`,
// resolving the round-3 residual `981230284d7d3273`): a resize storm
// whose EVERY step emits real (non-empty) redraw bytes — so none of the
// steps can coalesce via the exact-offset rule above — must still
// produce a BOUNDED `dim_markers` count, independent of redraw byte
// volume, via `enforce_dim_marker_cap` (not a byte-threshold coalescing
// rule, which round-4 already proved unsound above). ──────────────────

/// AC-4: a resize storm where each step's redraw exceeds the old 8 KiB
/// coalescing threshold — so exact-offset coalescing never fires — is
/// still bounded to `MAX_DIM_MARKERS` entries by the daemon-side cap,
/// regardless of how many steps occur.
///
/// Confirmed to fail pre-fix: before `enforce_dim_marker_cap` existed,
/// this recorded exactly `step_count` entries — the prior version of
/// this test asserted that UNBOUNDED growth as the then-accepted
/// trade-off (round-3 finding `981230284d7d3273`); task0005 changes
/// that trade-off, so this test now asserts the opposite. `step_count`
/// is tied to `MAX_DIM_MARKERS` (round-6 rework raised it) rather than a
/// literal, so this stays a genuine over-the-cap scenario regardless of
/// where the cap sits.
#[test]
fn resize_storm_with_large_per_step_redraw_stays_bounded_by_max_dim_markers() {
    let mut rb = ScrollbackRingBuffer::new(4 * 1024 * 1024);
    let step_count = (MAX_DIM_MARKERS + 34) as u16;
    let redraw = vec![b'r'; 10 * 1024]; // > the old 8 KiB coalescing threshold
    for step in 0..step_count {
        rb.write_resize_marker(80 + step, 24);
        rb.write(&redraw);
    }
    assert_eq!(
        rb.dim_markers_len(),
        MAX_DIM_MARKERS,
        "even though none of the {step_count} resize calls could \
         coalesce (each redraw exceeds the old 8 KiB coalescing \
         threshold), the daemon-side cap must still bound the recorded \
         segment count to MAX_DIM_MARKERS"
    );
}

/// D1''''' (round-8 rework, review round-7 finding `01f91fe698ceb287`):
/// with EXACTLY ONE eviction, `capped_head_dims` still synthesizes a
/// head segment at position 0 carrying the ONE evicted entry's exact
/// dimensions — this recovers full uncapped attribution exactly (the
/// reviewer measured 0 mixed rows for this shape, matching the oracle).
/// Every surviving entry keeps its own recorded position/dims unchanged.
///
/// Confirmed to fail pre-fix: reverting D1''''' to always fall back
/// to the oldest-SURVIVING marker's position (round-6's behavior)
/// instead of `capped_head_dims` makes the `segments[0]` assertions
/// below fail (position would be non-zero and rows would be the
/// survivor's, not the evicted entry's).
#[test]
fn enforce_dim_marker_cap_with_exactly_one_eviction_recovers_full_attribution_exactly() {
    let mut rb = ScrollbackRingBuffer::new(4 * 1024 * 1024);
    let extra = 1usize;
    let total_steps = MAX_DIM_MARKERS + extra;
    let content_per_step = b"distinct-step-content;";
    for step in 0..total_steps {
        rb.write_resize_marker(80, 24 + step as u16);
        rb.write(content_per_step);
    }
    assert_eq!(rb.dim_markers_len(), MAX_DIM_MARKERS);
    let (_, segments) = rb.read_segments();
    assert_eq!(
        segments.len(),
        MAX_DIM_MARKERS + 1,
        "with exactly one eviction, the single evicted entry's gap \
         still becomes its own head segment"
    );
    assert_eq!(segments[0].0, 0, "the head segment starts at position 0");
    assert_eq!(
        segments[0].2,
        24 + (extra - 1) as u16,
        "the head segment must carry the ONE evicted entry's rows \
         (step `extra - 1` == 0), recovering full attribution exactly"
    );
    let expected_rows: Vec<u16> = (extra..total_steps).map(|s| 24 + s as u16).collect();
    let actual_rows: Vec<u16> = segments[1..].iter().map(|&(_, _, rows)| rows).collect();
    assert_eq!(actual_rows, expected_rows);
}

/// D1''''' (round-8 rework, review round-7 finding `01f91fe698ceb287`):
/// with TWO OR MORE evictions, the cap no longer synthesizes ANY head
/// segment for the gap — round-7's `capped_head_dims` fallback only
/// names the LAST of several evicted entries, and the reviewer measured
/// that attributing the WHOLE multi-entry gap to it can replay MORE
/// cross-line mixing than leaving the gap unattributed entirely (up to
/// 13 mixed rows vs 3 for "no segments" on one tested shape, against 0
/// for full attribution). Leaving `head` unset here means
/// `TerminalCore::replay_segments` treats the leading gap as ordinary
/// content at the caller's TARGET dimensions (AC-3) instead of
/// misattributing it to a single evicted resize's dims. Every
/// surviving entry keeps its own recorded position/dims unchanged —
/// only the synthesized head disappears.
///
/// Confirmed to fail pre-fix: round-7's `capped_head_dims` fallback
/// (used unconditionally whenever ANY eviction had ever happened) makes
/// `segments.len()` come out as `MAX_DIM_MARKERS + 1` (one more than the
/// cap) with `segments[0]` at position 0 carrying the LAST evicted
/// entry's (wrong, partial-gap) dimensions — both assertions below fail.
#[test]
fn enforce_dim_marker_cap_with_multiple_evictions_leaves_the_gap_unattributed() {
    let mut rb = ScrollbackRingBuffer::new(4 * 1024 * 1024);
    let extra = 3usize;
    let total_steps = MAX_DIM_MARKERS + extra;
    let content_per_step = b"distinct-step-content;";
    for step in 0..total_steps {
        // `rows` encodes the step index so each entry is
        // distinguishable in the assertion below.
        rb.write_resize_marker(80, 24 + step as u16);
        rb.write(content_per_step);
    }
    assert_eq!(
        rb.dim_markers_len(),
        MAX_DIM_MARKERS,
        "cap must hold even with {extra} entries beyond it"
    );
    let (_, segments) = rb.read_segments();
    // Exactly MAX_DIM_MARKERS: no synthesized head segment for the
    // multi-entry gap — only the surviving entries, each at its own
    // real position.
    assert_eq!(
        segments.len(),
        MAX_DIM_MARKERS,
        "with 2+ evictions, no head segment is synthesized for the gap \
         — only the surviving entries appear"
    );
    // Every SURVIVING entry (step `extra` onward) keeps its EXACT
    // originally-recorded dimensions, in its ORIGINAL relative order.
    let expected_rows: Vec<u16> = (extra..total_steps).map(|s| 24 + s as u16).collect();
    let actual_rows: Vec<u16> = segments.iter().map(|&(_, _, rows)| rows).collect();
    assert_eq!(
        actual_rows, expected_rows,
        "every surviving entry must keep its EXACT originally-recorded \
         dimensions — only the discarded oldest entries lose precision"
    );
    // The oldest SURVIVOR (segments[0]) keeps its OWN real, NON-ZERO
    // position — no head segment is spliced in ahead of it.
    let step_extra_offset = extra * content_per_step.len();
    assert_eq!(
        segments[0].0, step_extra_offset,
        "the oldest surviving marker must keep its OWN recorded \
         position; the gap ahead of it is left unattributed, not \
         folded into a segment at position 0"
    );
}

// ── task0003 AC-7 (D4, review round-2 finding `0b0c18ff4ab911f4`):
// pruning is linear (VecDeque::pop_front), not quadratic ────────────

/// D3'' interaction: `enforce_dim_marker_cap` (count-based) and
/// `prune_dim_markers` (retained-window-based) cooperate correctly — a
/// resize storm whose total marker count exceeds `MAX_DIM_MARKERS` AND
/// whose accumulated content exceeds the ring's capacity (so BOTH
/// eviction mechanisms fire on the same run) never leaves `dim_markers`
/// above the cap, and `read_segments` still reconstructs a coherent
/// head segment for whatever content survives.
///
/// Supersedes `prune_dim_markers_collapses_a_large_backlog_to_a_bounded_tail_in_one_pass`
/// (task0003 AC-7 / review round-2 finding `0b0c18ff4ab911f4`): that
/// test's premise — building a 2,000-entry `dim_markers` backlog before
/// a single mass-eviction write — can no longer be constructed now that
/// `enforce_dim_marker_cap` keeps the count at `MAX_DIM_MARKERS` (16)
/// continuously; a backlog that large would need exactly the unbounded
/// growth task0005 closes. The O(1)-vs-O(n) `pop_front` concern that
/// test guarded is consequently moot at this bound (16 entries is cheap
/// either way); this replacement instead pins the CORRECTNESS of the
/// two eviction mechanisms cooperating, which is the property that
/// actually matters now.
#[test]
fn dim_markers_stays_capped_when_window_pruning_and_count_cap_both_fire() {
    let chunk_len = 4096;
    let marker_count = 40u32; // well above MAX_DIM_MARKERS
    // Small capacity relative to the total content written, so the
    // retained-window pruning ALSO advances past many early markers —
    // both eviction mechanisms are exercised on the same run.
    let capacity = (marker_count as usize / 2) * chunk_len;
    let mut rb = ScrollbackRingBuffer::new(capacity);
    for i in 0..marker_count {
        rb.write_resize_marker(80, 24 + (i % 50) as u16);
        rb.write(&vec![b'q'; chunk_len]);
        assert!(
            rb.dim_markers_len() <= MAX_DIM_MARKERS,
            "dim_markers must never exceed MAX_DIM_MARKERS, even \
             mid-storm (saw {} after marker {})",
            rb.dim_markers_len(),
            i
        );
    }
    let (bytes, segments) = rb.read_segments();
    assert_eq!(bytes.len(), capacity.min(marker_count as usize * chunk_len));
    assert!(
        !segments.is_empty(),
        "a ring that recorded resize markers must still report a head \
         segment for the retained window"
    );
    assert_eq!(
        segments[0].0, 0,
        "the head segment must always describe position 0 of the \
         retained window"
    );
}

/// Historical form of the test above, kept to pin the ORIGINAL
/// task0003 AC-7 guarantee in isolation: `prune_dim_markers` alone
/// (the count cap not yet in play, capacity comfortably larger than the
/// content written) still collapses a large backlog to a bounded tail
/// in one pass once the retained window is deliberately advanced past
/// it — the property this test previously required a 2,000-entry
/// backlog to observe now shows up at the (much smaller) count-cap
/// bound instead, so this variant caps the backlog at `MAX_DIM_MARKERS`
/// (the largest `dim_markers` can ever actually reach) rather than
/// asserting an unreachable precondition.
#[test]
fn prune_dim_markers_collapses_a_capped_backlog_to_a_bounded_tail_in_one_pass() {
    let chunk_len = 8 * 1024 + 1;
    let marker_count = MAX_DIM_MARKERS as u32;
    // Capacity comfortably larger than the whole backlog so every
    // marker's offset is still within the retained window before the
    // deliberate mass-eviction write below.
    let capacity = marker_count as usize * chunk_len + (1024 * 1024);
    let mut rb = ScrollbackRingBuffer::new(capacity);
    for i in 0..marker_count {
        rb.write_resize_marker(80, 24 + (i % 50) as u16);
        rb.write(&vec![b'q'; chunk_len]);
    }
    assert_eq!(
        rb.dim_markers_len(),
        marker_count as usize,
        "test prerequisite: all markers must still be within the \
         retained window (none pruned yet)"
    );
    // Advance the retained window past every recorded offset at once
    // (a single write >= capacity hits the ring's "keep only the tail"
    // fast path, so this itself stays O(capacity), not O(capacity^2)).
    rb.write(&vec![b'r'; capacity + 1]);
    rb.write_resize_marker(100, 40);
    assert!(
        rb.dim_markers_len() <= 2,
        "a single prune pass must collapse a {marker_count}-entry backlog \
         down to a bounded tail, got {}",
        rb.dim_markers_len()
    );
}

// ── task0003 AC-4: whole-buffer capture / load round-trip for the
// handoff snapshot ─────────────────────────────────────────────────

/// AC-4: a simple, non-wrapped ring round-trips its content bytes
/// exactly through capture -> load_snapshot.
#[test]
fn capture_then_load_snapshot_round_trips_bytes_for_a_simple_ring() {
    let mut rb = ScrollbackRingBuffer::new(1024);
    rb.write(b"hello world");
    let snap = rb.capture();
    let restored = ScrollbackRingBuffer::load_snapshot(&snap);
    assert_eq!(restored.read_all(), rb.read_all());
    assert_eq!(restored.capacity(), rb.capacity());
}

/// AC-4: a ring that has wrapped around still round-trips exactly the
/// bytes `read_all()` reports (not the raw internal layout).
#[test]
fn capture_then_load_snapshot_round_trips_bytes_after_wraparound() {
    let mut rb = ScrollbackRingBuffer::new(8);
    rb.write(b"ABCDEF");
    rb.write(b"GHI"); // wraps
    let snap = rb.capture();
    let restored = ScrollbackRingBuffer::load_snapshot(&snap);
    assert_eq!(restored.read_all(), b"BCDEFGHI");
    assert_eq!(restored.read_all(), rb.read_all());
}

/// AC-4: an empty ring round-trips to another empty ring with the same
/// capacity.
#[test]
fn capture_then_load_snapshot_round_trips_an_empty_ring() {
    let rb = ScrollbackRingBuffer::new(2048);
    let snap = rb.capture();
    let restored = ScrollbackRingBuffer::load_snapshot(&snap);
    assert!(restored.is_empty());
    assert_eq!(restored.capacity(), 2048);
}

/// AC-4: arbitrary bytes, including NUL bytes and invalid UTF-8,
/// round-trip exactly (the buffer is opaque bytes, never interpreted
/// as text).
#[test]
fn capture_then_load_snapshot_round_trips_bytes_containing_invalid_utf8_and_nuls() {
    let mut rb = ScrollbackRingBuffer::new(1024);
    let data: Vec<u8> = vec![0x00, 0xFF, 0x80, 0x00, b'x', 0xC0, 0xC1];
    rb.write(&data);
    let snap = rb.capture();
    let restored = ScrollbackRingBuffer::load_snapshot(&snap);
    assert_eq!(restored.read_all(), data);
}
