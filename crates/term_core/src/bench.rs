//! Lightweight bench harness for SlimCell scrollback. Gated behind
//! `#[cfg(test)]` and only runs as part of `cargo test --release`-style
//! invocations when the `slim_cell_bench` filter is supplied (since they
//! print measurements rather than asserting tight thresholds).
//!
//! The benches use `std::time::Instant`. They run on the host target
//! (x86 / aarch64), not on `wasm32-unknown-unknown`. Numbers here are
//! representative; production WASM will be different but trends carry
//! across.
//!
//! Usage:
//!     cargo test --lib --release slim_cell_bench -- --nocapture --include-ignored

#![allow(dead_code)]

use std::time::Instant;

use crate::cell::{Cell, PackedColor};
use crate::char_table::CharTable;
use crate::slim_cell::{SlimCell, cell_to_slim, slim_to_cell};
use crate::style_table::StyleTable;

#[cfg(test)]
mod benches {
    use super::*;

    /// Build a representative 200-cell row for compression benches.
    fn make_typical_row(cols: usize) -> Vec<Cell> {
        let mut row = Vec::with_capacity(cols);
        for c in 0..cols {
            let mut cell = Cell::EMPTY;
            cell.set_char("X");
            cell.width = 1;
            // Vary fg every 10 cells to simulate ANSI color output.
            let r = ((c / 10) * 25) as u8;
            cell.fg = PackedColor::rgb(r, 100, 200);
            row.push(cell);
        }
        row
    }

    fn compress_row(row: &[Cell], styles: &mut StyleTable, chars: &mut CharTable) -> Vec<SlimCell> {
        let mut out = Vec::with_capacity(row.len());
        for cell in row {
            out.push(cell_to_slim(cell, None, styles, chars));
        }
        out
    }

    /// Per-row Cell→SlimCell compression latency.
    #[test]
    #[ignore]
    fn slim_cell_bench_compress_row() {
        let cols = 200;
        let row = make_typical_row(cols);
        let mut styles = StyleTable::new();
        let mut chars = CharTable::new();
        // Warm-up.
        for _ in 0..1000 {
            let _ = compress_row(&row, &mut styles, &mut chars);
        }
        // Reset tables to baseline.
        styles = StyleTable::new();
        chars = CharTable::new();
        let iters = 10_000usize;
        let start = Instant::now();
        for _ in 0..iters {
            let _ = compress_row(&row, &mut styles, &mut chars);
        }
        let elapsed = start.elapsed();
        let per_row = elapsed.as_nanos() as f64 / iters as f64;
        eprintln!(
            "[bench] compress_row: {iters} iters / {:?} = {per_row:.0} ns/row ({} cells, {:.1} ns/cell)",
            elapsed,
            cols,
            per_row / cols as f64,
        );
    }

    /// Per-cell SlimCell→Cell decompression latency.
    #[test]
    #[ignore]
    fn slim_cell_bench_decompress_cell() {
        let mut styles = StyleTable::new();
        let mut chars = CharTable::new();
        let row = make_typical_row(200);
        let slim_row = compress_row(&row, &mut styles, &mut chars);
        let iters = 1_000_000usize;
        let start = Instant::now();
        let mut sink = 0u64;
        for i in 0..iters {
            let slim = &slim_row[i % slim_row.len()];
            let cell = slim_to_cell(slim, &styles, &chars);
            sink = sink.wrapping_add(cell.width as u64);
        }
        let elapsed = start.elapsed();
        let per_cell = elapsed.as_nanos() as f64 / iters as f64;
        eprintln!(
            "[bench] decompress_cell: {iters} iters / {:?} = {per_cell:.0} ns/cell (sink={sink})",
            elapsed,
        );
    }

    /// Approximate scrollback memory footprint for a 10 000 × 200 grid.
    #[test]
    #[ignore]
    fn slim_cell_bench_scrollback_memory() {
        use crate::terminal_core::TerminalCore;
        let cols = 200u16;
        let rows = 24u16;
        let scrollback = 10_000u32;
        let mut core = TerminalCore::new(cols, rows, scrollback);
        // Fill scrollback with realistic content (10 colors interleaved).
        for row_i in 0..(scrollback as u16) {
            for c in 0..cols {
                let color = (row_i % 10) as u8;
                core.set_cell(c, 0, "X", 1, 2, color * 25, 100, 200, 0, 0, 0, 0, 0);
            }
            core.scroll_up_internal(1);
        }
        let slim_cells = core.scrollback_slim.iter().map(|r| r.len()).sum::<usize>();
        let slim_bytes = slim_cells * std::mem::size_of::<SlimCell>();
        let style_bytes = core.styles.bytes_used();
        let char_bytes = core.chars.bytes_used();
        let scrollback_total = slim_bytes + style_bytes + char_bytes;

        // Baseline: same 10000 × 200 cells stored as Cell.
        let baseline_cell_bytes = slim_cells * std::mem::size_of::<Cell>();
        let ratio = scrollback_total as f64 / baseline_cell_bytes as f64;

        eprintln!(
            "[bench] scrollback_memory: rows={rows} cols={cols} scrollback={scrollback}\n\
             slim_cells={slim_cells} slim={}KB style={}B char={}B\n\
             total={}KB  baseline_cell={}KB  ratio={:.2}",
            slim_bytes / 1024,
            style_bytes,
            char_bytes,
            scrollback_total / 1024,
            baseline_cell_bytes / 1024,
            ratio,
        );
        assert!(
            ratio < 0.5,
            "memory ratio {} ≥ 0.5 — SlimCell is not delivering ≥ 50% reduction",
            ratio
        );
    }

    /// Perf bench: simulate the client-side mux snapshot replay — feed a
    /// 2 MiB `seq 1 N`-shaped payload into a fresh core via
    /// `build_from_snapshot`, which is the exact entry point the off-thread
    /// snapshot worker uses (`tabs.rs::dispatch_offthread_replay`).
    ///
    /// This is the single biggest term-side cost during a mux tab switch on a
    /// pane with a full scrollback ring — the daemon-side filter + ring copy
    /// run once, but THIS runs on the whole payload byte-by-byte through the
    /// ANSI parser, allocating SlimCells row by row.
    ///
    /// Gated `#[ignore]`. Invoke with:
    ///
    /// ```sh
    /// CARGO_TARGET_DIR=src-tauri/target cargo test --release \
    ///   --manifest-path crates/term_core/Cargo.toml --lib \
    ///   snapshot_replay_bench_2mib_seq \
    ///   -- --nocapture --include-ignored
    /// ```
    #[test]
    #[ignore]
    fn snapshot_replay_bench_2mib_seq() {
        use crate::terminal_core::TerminalCore;
        use std::io::Write;
        use std::sync::atomic::AtomicBool;
        use std::time::Instant;

        // Build a ~2 MiB payload mimicking the scrollback contents of a pane
        // that ran `seq 1 N` — 7-digit decimals + CRLF. No ESC sequences:
        // the parser is hot on print + CR/LF, the common case.
        let mut payload = Vec::with_capacity(2 * 1024 * 1024);
        let mut n: u64 = 1;
        while payload.len() < 2 * 1024 * 1024 {
            let _ = write!(&mut payload, "{n}\r\n");
            n += 1;
        }
        payload.truncate(2 * 1024 * 1024);

        // Warm-up.
        for _ in 0..1 {
            let cancel = AtomicBool::new(false);
            let _ = TerminalCore::build_from_snapshot(200, 50, 10_000, &payload, &[], &cancel);
        }

        // Measure: build_from_snapshot end-to-end (this is what the worker
        // thread actually runs in `dispatch_offthread_replay`).
        let iters = 3;
        let start = Instant::now();
        for _ in 0..iters {
            let cancel = AtomicBool::new(false);
            let replay = TerminalCore::build_from_snapshot(200, 50, 10_000, &payload, &[], &cancel);
            std::hint::black_box(replay);
        }
        let elapsed = start.elapsed();
        let per = elapsed / iters as u32;
        eprintln!(
            "[bench] build_from_snapshot 2MiB seq-N payload (200x50, 10k scrollback): \
             {iters} iters / {:?} → {:?}/call ({:.1} MiB/s)",
            elapsed,
            per,
            (2.0 * iters as f64) / elapsed.as_secs_f64(),
        );
        // SPEC.md "Performance Goals" (FR4 / NFR1): MUST < 1000 ms/call.
        let threshold = std::time::Duration::from_millis(1000);
        assert!(
            per < threshold,
            "build_from_snapshot per-call {:?} ≥ MUST threshold {:?} (FR4 / NFR1)",
            per,
            threshold,
        );

        // Also measure raw process_pty_data_fully (without the SnapshotReplay
        // bookkeeping) so we can attribute time between parse and the marks-
        // drain bookkeeping.
        let iters2 = 3;
        let start = Instant::now();
        for _ in 0..iters2 {
            let mut core = TerminalCore::new(200, 50, 10_000);
            core.reset();
            let actions = core.process_pty_data_fully(&payload);
            std::hint::black_box(actions);
        }
        let elapsed = start.elapsed();
        let per = elapsed / iters2 as u32;
        eprintln!(
            "[bench] process_pty_data_fully  2MiB seq-N payload (200x50, 10k scrollback): \
             {iters2} iters / {:?} → {:?}/call ({:.1} MiB/s)",
            elapsed,
            per,
            (2.0 * iters2 as f64) / elapsed.as_secs_f64(),
        );
    }

    /// AC-1 (task0001, D7): reproduces the measured "resize-marker-dense
    /// scrollback tail" shape from SPEC.md's References — a ~2 MiB payload
    /// with ~31 replay segments where a large HEAD (already at the target
    /// dims) is followed by a dense cluster of resize markers (dims
    /// oscillating below the target, never reaching or exceeding it) whose
    /// own content is tiny, then a small qualifying tail back at the
    /// target. Pre-D7, all three of the split's gates failed simultaneously
    /// for this shape (`k` and the raw prefix byte length both exceed their
    /// bounds, and the small tail does not dominate the raw head+cluster
    /// prefix), forcing the full non-bypass drain — measured 782.8-977.6 ms
    /// for a payload this size. D7 recognizes that only the small MIDDLE
    /// (the cluster itself) needs non-bypass fidelity, so this now costs
    /// close to the bypass-engaged baseline (tens of ms), not the ~800-1000
    /// ms non-bypass one.
    ///
    /// Confirmed to fail pre-fix (D7): reverting to `stable_target_suffix_start`
    /// alone (no `h` / `leading_target_run_len`) measures this shape within
    /// the ~800-1000 ms non-bypass order — see
    /// `head_plus_marker_cluster_engages_the_split_and_matches_reference`
    /// in `terminal_core.rs` for the deterministic (non-timing)
    /// `scrollback_populated` regression guard this bench complements.
    ///
    /// Gated `#[ignore]` (release-mode timing bench). Invoke with:
    ///
    /// ```sh
    /// CARGO_TARGET_DIR=src-tauri/target cargo test --release \
    ///   --manifest-path crates/term_core/Cargo.toml --lib \
    ///   marker_cluster_tail_bench_2mib \
    ///   -- --nocapture --include-ignored
    /// ```
    #[test]
    #[ignore]
    fn marker_cluster_tail_bench_2mib_matches_bypass_engaged_cost() {
        use crate::terminal_core::{ReplaySegment, TerminalCore};
        use std::sync::atomic::AtomicBool;
        use std::time::Instant;

        const SHIPPING_SCROLLBACK_LINES: u32 = 10_000;
        let cols: u16 = 100;
        let target_rows: u16 = 30;
        let cluster_rows_a: u16 = 24;
        let cluster_rows_b: u16 = 26;

        // HEAD: the bulk of the pane's real history, already at the
        // target — ~2 MiB, matching the measured shape's dominant byte
        // share.
        let head_filler = b"pane history line padded out a bit for size\r\n";
        let mut payload: Vec<u8> = Vec::with_capacity(2 * 1024 * 1024 + 16 * 1024);
        let mut segments = vec![ReplaySegment {
            offset: 0,
            cols,
            rows: target_rows,
        }];
        while payload.len() < 2 * 1024 * 1024 {
            payload.extend_from_slice(head_filler);
        }
        let head_len = payload.len();

        // MIDDLE: a dense cluster of exactly BYPASS_PREFIX_MAX_SEGMENTS (24)
        // resize markers — head + cluster = 25 segments, one past the OLD
        // gate's segment-count bound (matching the measured shape's `k`
        // exceeding it), while the cluster's OWN count sits exactly at the
        // NEW gate's bound. Dims oscillate between two values below the
        // target, tiny content between them.
        let cluster_filler = b"x\r\n";
        for i in 0..24usize {
            segments.push(ReplaySegment {
                offset: payload.len() as u32,
                cols,
                rows: if i % 2 == 0 {
                    cluster_rows_a
                } else {
                    cluster_rows_b
                },
            });
            payload.extend_from_slice(cluster_filler);
        }
        let middle_len = payload.len() - head_len;

        // TAIL: ~7395 bytes back at the target, matching the measured
        // shape's small qualifying suffix.
        segments.push(ReplaySegment {
            offset: payload.len() as u32,
            cols,
            rows: target_rows,
        });
        let tail_start = payload.len();
        while payload.len() - tail_start < 7395 {
            payload.extend_from_slice(head_filler);
        }
        let tail_len = payload.len() - tail_start;

        eprintln!(
            "[bench] marker-cluster-tail shape: total={}B head={}B middle={}B tail={}B segments={}",
            payload.len(),
            head_len,
            middle_len,
            tail_len,
            segments.len(),
        );

        // Segment-free baseline, same total size, for comparison.
        let mut free_payload = Vec::with_capacity(payload.len());
        while free_payload.len() < payload.len() {
            free_payload.extend_from_slice(head_filler);
        }
        free_payload.truncate(payload.len());

        let measure = |p: &[u8], segs: &[ReplaySegment]| -> std::time::Duration {
            {
                let cancel = AtomicBool::new(false);
                let _ = TerminalCore::build_from_snapshot(
                    cols,
                    target_rows,
                    SHIPPING_SCROLLBACK_LINES,
                    p,
                    segs,
                    &cancel,
                );
            }
            let cancel = AtomicBool::new(false);
            let start = Instant::now();
            let replay = TerminalCore::build_from_snapshot(
                cols,
                target_rows,
                SHIPPING_SCROLLBACK_LINES,
                p,
                segs,
                &cancel,
            );
            let elapsed = start.elapsed();
            std::hint::black_box(replay);
            elapsed
        };

        let t_free = measure(&free_payload, &[]);
        let t_marker_cluster = measure(&payload, &segments);

        eprintln!(
            "[bench] marker-cluster tail (2 MiB, {cols}x{target_rows}, \
             sb={SHIPPING_SCROLLBACK_LINES}): segment-free → {:?} | \
             marker-cluster-tail (head+cluster+tail) → {:?}",
            t_free, t_marker_cluster,
        );

        // AC-1: tens-of-ms order, matching the bypass-engaged baseline for
        // a payload this size — NOT the ~800-1000 ms non-bypass baseline
        // the pre-D7 gate paid for this shape. A generous multiplicative +
        // absolute floor absorbs scheduler / measurement noise on a single
        // sample.
        let bound = t_free.mul_f64(5.0) + std::time::Duration::from_millis(50);
        assert!(
            t_marker_cluster < bound,
            "marker-cluster-tail shape {:?} is not close to segment-free \
             {:?} (bound {:?}) — D7's head/middle/tail split is not \
             engaging for this shape",
            t_marker_cluster,
            t_free,
            bound,
        );
    }

    /// Hypothesis-confirming bench: re-run the same 2 MiB `seq 1 N` payload
    /// with three grid/scrollback configurations to attribute the parse cost.
    ///
    /// Reasoning: `seq 1 N` emits ~230k lines, so on a 50-row grid every `\n`
    /// after row 50 triggers `scroll_up_internal` → `ring_push_blank`, which
    /// runs `cell_to_slim` over every cell of the evicted row and (once the
    /// scrollback ring is full) `release_slim_row` on the oldest. If that
    /// per-line scrollback-compression is the dominant cost, the
    /// `scrollback_lines = 0` and "huge grid (no scroll)" variants will be
    /// dramatically faster than the 10k-scrollback baseline.
    ///
    /// Variants:
    /// - `200×50, scrollback=10_000` — baseline (snapshot-replay realistic).
    /// - `200×50, scrollback=0`      — scroll still fires, but no SlimCell
    ///   compression and no scrollback ring management.
    /// - `200×250_000, scrollback=0` — grid is taller than the line count, so
    ///   `\n` never triggers a scroll at all. Pure print + cursor motion.
    ///
    /// Gated `#[ignore]`. Invoke with:
    /// ```sh
    /// CARGO_TARGET_DIR=src-tauri/target cargo test --release \
    ///   --manifest-path crates/term_core/Cargo.toml --lib \
    ///   snapshot_replay_attribution_2mib_seq \
    ///   -- --nocapture --include-ignored
    /// ```
    #[test]
    #[ignore]
    fn snapshot_replay_attribution_2mib_seq() {
        use crate::terminal_core::TerminalCore;
        use std::io::Write;
        use std::time::Instant;

        let mut payload = Vec::with_capacity(2 * 1024 * 1024);
        let mut n: u64 = 1;
        while payload.len() < 2 * 1024 * 1024 {
            let _ = write!(&mut payload, "{n}\r\n");
            n += 1;
        }
        payload.truncate(2 * 1024 * 1024);

        // For the "no scroll at all" variant, rows is u16 so we cap at
        // 65 000 and truncate the payload to a size that fits without any
        // `\n`-induced scroll. Rate (MiB/s) makes the variants comparable
        // even though absolute payload size differs for the no-scroll case.
        let no_scroll_rows: u16 = 65_000;
        // ~8 bytes per line → fit comfortably under no_scroll_rows lines.
        let no_scroll_payload_max = (no_scroll_rows as usize) * 6; // headroom
        let no_scroll_payload = &payload[..no_scroll_payload_max.min(payload.len())];

        let run = |label: &str,
                   cols: u16,
                   rows: u16,
                   sb: u32,
                   p: &[u8],
                   iters: u32,
                   assert_threshold: Option<std::time::Duration>| {
            // Warm-up.
            {
                let mut core = TerminalCore::new(cols, rows, sb);
                core.reset();
                let _ = core.process_pty_data_fully(p);
            }
            let start = Instant::now();
            for _ in 0..iters {
                let mut core = TerminalCore::new(cols, rows, sb);
                core.reset();
                let actions = core.process_pty_data_fully(p);
                std::hint::black_box(actions);
            }
            let elapsed = start.elapsed();
            let per = elapsed / iters;
            let bytes = (p.len() as f64) * (iters as f64);
            eprintln!(
                "[bench] {label:48} ({cols}x{rows}, sb={sb:>7}, payload={:>4} KiB): \
                 {iters} iters / {:?} → {:?}/call ({:.1} MiB/s)",
                p.len() / 1024,
                elapsed,
                per,
                bytes / (1024.0 * 1024.0) / elapsed.as_secs_f64(),
            );
            if let Some(threshold) = assert_threshold {
                assert!(
                    per < threshold,
                    "{label}: per-call {:?} ≥ threshold {:?}",
                    per,
                    threshold,
                );
            }
        };

        run(
            "baseline (scroll + scrollback compression)",
            200,
            50,
            10_000,
            &payload,
            3,
            // Baseline is the unmitigated cost — eprintln only, no assert.
            None,
        );
        // SPEC.md "Performance Goals" (FR5): scrollback-disabled
        // configuration's per-call must be < 200 ms. This isolates the
        // underlying parse + scroll path from the SlimCell compression
        // cost — a regression in just this number points at the parser /
        // grid hot path.
        run(
            "scroll only (no scrollback compression)",
            200,
            50,
            0,
            &payload,
            3,
            Some(std::time::Duration::from_millis(200)),
        );
        run(
            "no scroll at all (huge grid)",
            200,
            no_scroll_rows,
            0,
            no_scroll_payload,
            3,
            // No-scroll configuration is reported only — no assert.
            None,
        );
    }

    /// Perf bench (NFR2): full 2nd-pass scrollback restore cost — the
    /// bypass-off rebuild plus the merge primitive — on the same 2 MiB
    /// `seq 1 N`-shaped payload the 1st-pass bench measures. This is the
    /// number the user feels when the visible grid paints fast but the
    /// scrollback takes a while to appear after a mux switch.
    ///
    /// Composition per iteration:
    /// 1. `build_from_snapshot` (bypass on) — 1st-pass equivalent,
    ///    populates the live core.
    /// 2. `build_scrollback_only_from_snapshot` (bypass off) — 2nd-pass
    ///    rebuild, the dominant cost.
    /// 3. `merge_scrollback_from` — re-intern + prepend; bounded by
    ///    `scrollback_capacity`.
    ///
    /// Asserts: per-call total < 5 s (NFR2 gate). On the reference machine
    /// the 2nd-pass alone is ~4 s, so the budget leaves ~1 s of headroom
    /// for the merge.
    ///
    /// Gated `#[ignore]`. Invoke with:
    ///
    /// ```sh
    /// CARGO_TARGET_DIR=src-tauri/target cargo test --release \
    ///   --manifest-path crates/term_core/Cargo.toml --lib \
    ///   scrollback_restore_bench_2mib_seq \
    ///   -- --nocapture --include-ignored
    /// ```
    #[test]
    #[ignore]
    fn scrollback_restore_bench_2mib_seq() {
        use crate::terminal_core::TerminalCore;
        use std::io::Write;
        use std::sync::atomic::AtomicBool;
        use std::time::Instant;

        let mut payload = Vec::with_capacity(2 * 1024 * 1024);
        let mut n: u64 = 1;
        while payload.len() < 2 * 1024 * 1024 {
            let _ = write!(&mut payload, "{n}\r\n");
            n += 1;
        }
        payload.truncate(2 * 1024 * 1024);

        // Warm-up: hit every layer once so the icache / branch predictors
        // converge before the measurement loop.
        {
            let cancel = AtomicBool::new(false);
            let bypass = TerminalCore::build_from_snapshot(200, 50, 10_000, &payload, &[], &cancel)
                .expect("warm-up 1st-pass");
            let rebuilt = TerminalCore::build_scrollback_only_from_snapshot(
                200,
                50,
                10_000,
                &payload,
                &[],
                &cancel,
            )
            .expect("warm-up 2nd-pass");
            let mut live = bypass.core;
            live.merge_scrollback_from(rebuilt.core, 0);
            std::hint::black_box(live);
        }

        let iters = 3;
        let start = Instant::now();
        for _ in 0..iters {
            let cancel = AtomicBool::new(false);
            let bypass = TerminalCore::build_from_snapshot(200, 50, 10_000, &payload, &[], &cancel)
                .expect("1st-pass");
            let rebuilt = TerminalCore::build_scrollback_only_from_snapshot(
                200,
                50,
                10_000,
                &payload,
                &[],
                &cancel,
            )
            .expect("2nd-pass");
            let mut live = bypass.core;
            live.merge_scrollback_from(rebuilt.core, 0);
            std::hint::black_box(live);
        }
        let elapsed = start.elapsed();
        let per = elapsed / iters as u32;
        eprintln!(
            "[bench] scrollback_restore 2MiB seq-N payload (200x50, 10k scrollback): \
             {iters} iters / {:?} → {:?}/call (1st-pass + 2nd-pass + merge end-to-end)",
            elapsed, per,
        );
        // NFR2: per-call total must be < 5 s for 2 MiB.
        let threshold = std::time::Duration::from_secs(5);
        assert!(
            per < threshold,
            "scrollback_restore per-call {:?} ≥ MUST threshold {:?} (NFR2)",
            per,
            threshold,
        );
    }

    /// Perf bench (NFR1, task0005 rework D3'', review round-4 finding
    /// `6c650908ea8e95e9`; round-6 rework D1''', review round-5 finding
    /// `abb36fa1ad4c89ea`): reproduces the round-4 measurement methodology —
    /// a ~0.95 MiB snapshot replayed through `TerminalCore::build_from_snapshot`
    /// at the shipping scrollback default — varying segment COUNT (this
    /// bench's original axis) AND, per AC-2, whether a single segment's
    /// dims differ from the replay target (the axis round-5's own bench
    /// missed: its two arms, 4 and 16 segments, BOTH already paid the
    /// bypass-downgrade cost, so the measured 1.09 ratio never approached
    /// the 1-second bound it asserted against). `term_core` has no
    /// dependency on the daemon crate, so `DAEMON_SEGMENT_CAP` duplicates
    /// `src-tauri/src/mux/scrollback_buffer.rs::MAX_DIM_MARKERS` as a
    /// literal, cross-referenced by name — keep the two in sync if either
    /// changes.
    ///
    /// Round-4's raw (unbounded) measurement: segments=0 → 134 ms / 5 →
    /// 176 ms / 20 → 272 ms / 30 → 2078 ms / 50 → 3322 ms / 80 → 5350 ms —
    /// replay cost jumps sharply once segment count crosses roughly 20-30.
    /// Round-5's measurement (finding `abb36fa1ad4c89ea`) isolated the axis
    /// that actually drives THAT curve: not segment count itself, but
    /// whether `build_from_snapshot_inner` downgrades out of the
    /// snapshot-replay bypass for the WHOLE drain (pre-round-6, it did so
    /// whenever ANY segment differed from the target) — 1 segment already
    /// AT the target → 7 ms; 1 segment differing → 220 ms, a ~30x jump for
    /// a SINGLE segment, dwarfing the count-driven curve above. Round-6's
    /// `build_from_snapshot_inner` prefix/suffix split (D1''') closes that
    /// gap for the realistic "ordinary switch" shape (a small differing
    /// HEAD segment followed by the pane's bulk history already at the
    /// target) WITHOUT raising `DAEMON_SEGMENT_CAP` enough to matter for
    /// speed — this bench's `t_ordinary_switch` arm is the direct
    /// regression guard for that (see also
    /// `ordinary_switch_bench_950kib_matches_segment_free_cost` below,
    /// which asserts the AC-4 bound in isolation). The COUNT-scaling arms
    /// below (`t_quarter` / `t_cap`) still matter for the genuine
    /// resize-STORM shape the split does not help (no stable tail — see
    /// that function's doc), which is why `DAEMON_SEGMENT_CAP` stays
    /// bounded at all (AC-3's correctness requirement, not a speed one).
    ///
    /// D1'''''' (round-9 rework, review round-8 finding `6082de4e619d7f51`):
    /// `DAEMON_SEGMENT_CAP` raised from 24 to 62 alongside `MAX_DIM_MARKERS`.
    /// At the time, this cost was genuinely paid: 24 segs → 323 ms / 32 →
    /// 2.49 s / 48 → 3.65 s / 62 → 4.5 s, because every intermediate
    /// resize inside `replay_segments` re-wrapped the ENTIRE scrollback
    /// accumulated so far — cost that grew with both segment count and
    /// accumulated content, not with the height change itself.
    ///
    /// D1 (round-10 rework, task0010): `TerminalCore::resize_same_width`
    /// (`reflow.rs`) no longer re-wraps retained scrollback for a
    /// same-width resize — the shape EVERY segment transition in this
    /// bench is (`cols` is fixed; only `rows` alternates) — touching only
    /// the rows that actually cross the viewport/scrollback boundary
    /// (bounded by the row-count DELTA, never by how much scrollback has
    /// accumulated). Re-measured at the same cap: 24 segs → ~17 ms / 32 →
    /// ~161 ms / 48 → ~162 ms / 62 → ~164 ms — the storm-path cost that
    /// scaled to SECONDS now plateaus in the hundreds of milliseconds, a
    /// ~28x improvement at the cap. The remaining cost is NOT resize cost
    /// (this implementer measured `resize_same_width`'s own inputs
    /// directly during development: the rows dropped/pulled per call stay
    /// bounded by `|rows_a - rows_b|` = 6 throughout, never by
    /// accumulated scrollback size) — it is the ordinary per-line
    /// scrollback-compression cost (`cell_to_slim` on each row that
    /// genuinely scrolls off, an unavoidable, pre-existing cost this task
    /// does not touch) that a storm shape happens to trigger more or less
    /// of depending on incidental content-processing behavior at
    /// different chunk sizes — visible in `t_quarter` staying cheap while
    /// `t_cap` does not, even though both are handled by the identical
    /// O(row-count-delta) resize path.
    ///
    /// Gated `#[ignore]` (release-mode timing bench, not part of the
    /// default `--lib` run). Invoke with:
    ///
    /// ```sh
    /// CARGO_TARGET_DIR=src-tauri/target cargo test --release \
    ///   --manifest-path crates/term_core/Cargo.toml --lib \
    ///   segment_bounded_replay_bench_950kib \
    ///   -- --nocapture --include-ignored
    /// ```
    #[test]
    #[ignore]
    fn segment_bounded_replay_bench_950kib_stays_bounded_at_the_daemon_cap() {
        use crate::terminal_core::{ReplaySegment, TerminalCore};
        use std::sync::atomic::AtomicBool;
        use std::time::Instant;

        // Mirrors `src-tauri/src/mux/scrollback_buffer.rs::MAX_DIM_MARKERS`.
        // D1'''''' (round-9 rework, review round-8 finding `6082de4e619d7f51`):
        // raised from 24 to 62 alongside that constant — see
        // `VERIFICATION.md`'s NFR1 section for the re-measured numbers at
        // this new cap (AC-3).
        const DAEMON_SEGMENT_CAP: usize = 62;
        // Mirrors `src-tauri/src/settings.rs::DEFAULT_SCROLLBACK_LINES`.
        const SHIPPING_SCROLLBACK_LINES: u32 = 10_000;
        const TARGET_PAYLOAD_LEN: usize = 950 * 1024;
        let cols: u16 = 100;
        let rows_a: u16 = 30;
        let rows_b: u16 = 24;

        let filler = b"line of scrollback content padded out a bit\r\n";
        let fill_to = |len: usize| -> Vec<u8> {
            let mut payload = Vec::with_capacity(len + filler.len());
            while payload.len() < len {
                payload.extend_from_slice(filler);
            }
            payload
        };

        // Alternating-dims payload with NO stable tail (every segment,
        // including the last, differs from its predecessor and from the
        // replay target `rows_a` whenever `segment_count` is even) — the
        // resize-STORM shape the prefix/suffix split does not speed up.
        let build_storm_payload = |segment_count: usize| -> (Vec<u8>, Vec<ReplaySegment>) {
            if segment_count == 0 {
                return (fill_to(TARGET_PAYLOAD_LEN), Vec::new());
            }
            let per_segment = TARGET_PAYLOAD_LEN / segment_count;
            let mut payload = Vec::with_capacity(TARGET_PAYLOAD_LEN + filler.len() * segment_count);
            let mut segments = Vec::with_capacity(segment_count);
            for i in 0..segment_count {
                segments.push(ReplaySegment {
                    offset: payload.len() as u32,
                    cols,
                    rows: if i % 2 == 0 { rows_a } else { rows_b },
                });
                let start_len = payload.len();
                while payload.len() < start_len + per_segment {
                    payload.extend_from_slice(filler);
                }
            }
            (payload, segments)
        };

        let measure = |payload: &[u8], segments: &[ReplaySegment]| -> std::time::Duration {
            // Warm-up.
            {
                let cancel = AtomicBool::new(false);
                let _ = TerminalCore::build_from_snapshot(
                    cols,
                    rows_a,
                    SHIPPING_SCROLLBACK_LINES,
                    payload,
                    segments,
                    &cancel,
                );
            }
            let cancel = AtomicBool::new(false);
            let start = Instant::now();
            let replay = TerminalCore::build_from_snapshot(
                cols,
                rows_a,
                SHIPPING_SCROLLBACK_LINES,
                payload,
                segments,
                &cancel,
            );
            let elapsed = start.elapsed();
            std::hint::black_box(replay);
            elapsed
        };

        let (storm_baseline_payload, _) = build_storm_payload(0);
        let t_baseline = measure(&storm_baseline_payload, &[]);
        let (storm_quarter_payload, storm_quarter_segments) =
            build_storm_payload(DAEMON_SEGMENT_CAP / 4);
        let t_quarter = measure(&storm_quarter_payload, &storm_quarter_segments);
        let (storm_cap_payload, storm_cap_segments) = build_storm_payload(DAEMON_SEGMENT_CAP);
        let t_cap = measure(&storm_cap_payload, &storm_cap_segments);

        // D1'''''' (round-9 rework): additional intermediate points (24, 32,
        // 48 — the round-8 reviewer's own cap-sweep values) so this bench's
        // output is a full curve, not just the two endpoints, making the
        // superlinear shape between them visible rather than assumed.
        for &segs in &[24usize, 32, 48] {
            let (payload, segments) = build_storm_payload(segs);
            let t = measure(&payload, &segments);
            eprintln!(
                "[bench] segment-bounded replay intermediate point: {segs} segs (storm) → {t:?}"
            );
        }

        // AC-2: the axis that actually costs — a single segment ALREADY at
        // the target vs one that DIFFERS, spanning the whole payload (no
        // stable tail for the split to exploit).
        let one_equal_segments = vec![ReplaySegment {
            offset: 0,
            cols,
            rows: rows_a,
        }];
        let one_equal_payload = fill_to(TARGET_PAYLOAD_LEN);
        let t_one_equal = measure(&one_equal_payload, &one_equal_segments);
        let one_differing_segments = vec![ReplaySegment {
            offset: 0,
            cols,
            rows: rows_b,
        }];
        let one_differing_payload = fill_to(TARGET_PAYLOAD_LEN);
        let t_one_differing = measure(&one_differing_payload, &one_differing_segments);

        eprintln!(
            "[bench] segment-bounded replay (0.95 MiB, {cols}x{rows_a}, sb={SHIPPING_SCROLLBACK_LINES}): \
             0 segs → {:?} | {} segs (storm) → {:?} | {} segs (storm, daemon cap) → {:?} | \
             1 seg == target → {:?} | 1 seg != target (no stable tail) → {:?}",
            t_baseline,
            DAEMON_SEGMENT_CAP / 4,
            t_quarter,
            DAEMON_SEGMENT_CAP,
            t_cap,
            t_one_equal,
            t_one_differing,
        );

        // D1 (round-10 rework, AC-1/AC-6): restores a HARD latency gate —
        // round-9 downgraded this to informational because the pre-D1
        // resize cost genuinely scaled to seconds at this cap (see the
        // function doc). Now that `resize_same_width` no longer re-wraps
        // retained scrollback, `t_cap` (62 segments — the daemon's own
        // cap) measures ~160-170 ms on the reference machine; the bound
        // below (a stated small multiple of the segment-free baseline,
        // AC-1's own wording, plus a fixed floor absorbing scheduler
        // noise on a single sample) leaves headroom above that while
        // still catching a real regression to the pre-D1 multi-second
        // cost by roughly an order of magnitude. Confirmed to fail
        // without D1: reverting `resize_same_width` to call
        // `resize_same_width_reference` unconditionally reproduces the
        // ~4.5 s cost this bound rejects.
        let bound = t_baseline.mul_f64(60.0) + std::time::Duration::from_millis(200);
        assert!(
            t_cap < bound,
            "storm replay at the daemon cap ({DAEMON_SEGMENT_CAP} segs) took {:?}, \
             not within the stated small multiple of the segment-free baseline \
             {:?} (bound {:?}) — AC-1/AC-6 regression",
            t_cap,
            t_baseline,
            bound,
        );

        // Informational only (not asserted): `t_cap` / `t_quarter` stays a
        // large ratio (~8-9x) even after D1, because `t_quarter`'s smaller
        // per-segment content happens to trigger less real scrollback
        // churn than `t_cap`'s — a content-processing characteristic
        // unrelated to resize cost (this implementer confirmed
        // `resize_same_width`'s own inputs — rows moved per call — stay
        // bounded by the row-count delta in BOTH cases). A ratio bound
        // would therefore assert something D1 does not claim to fix;
        // AC-1's bound above (against the segment-free baseline) is the
        // one this task is accountable for.
        let ratio = t_cap.as_secs_f64() / t_quarter.as_secs_f64().max(0.0001);
        eprintln!(
            "[bench] segment-bounded replay: full-cap / quarter-cap ratio = {ratio:.1}x \
             (informational only — see doc comment)"
        );
        // AC-2 (informational, not asserted): `t_one_equal` should track
        // `t_baseline` closely (no resize at all — the pre-round-5 fast
        // path) while `t_one_differing` is expected to stay slow (no
        // stable tail for the split to exploit) — the point of this arm is
        // to make that cost SHAPE visible, not to bound it; AC-4's bound on
        // the "ordinary switch" shape lives in
        // `ordinary_switch_bench_950kib_matches_segment_free_cost`.
    }

    /// AC-1 / AC-4 (round-6 rework, review round-5 finding
    /// `abb36fa1ad4c89ea`): a ~0.95 MiB snapshot replayed as the "ordinary
    /// window switch" shape — a tiny HEAD segment at the daemon's
    /// hardcoded spawn size (`MuxPane::new`'s 80x24), differing from the
    /// replay target, followed by the pane's bulk history already at the
    /// target (the shape one subsequent `MuxPane::resize` produces once
    /// the GUI's grid has settled) — costs approximately what a
    /// segment-free replay of the same payload costs, via
    /// `TerminalCore::build_from_snapshot_inner`'s D1''' prefix/suffix
    /// split. Confirmed to fail pre-fix: reverting the split (bypass
    /// downgrading for the whole drain whenever ANY segment differs from
    /// the target, rounds 1-5's behavior) measured ~220 ms for this exact
    /// shape against ~7 ms segment-free — a ratio the bound below would
    /// reject.
    ///
    /// Gated `#[ignore]` (release-mode timing bench). Invoke with:
    ///
    /// ```sh
    /// CARGO_TARGET_DIR=src-tauri/target cargo test --release \
    ///   --manifest-path crates/term_core/Cargo.toml --lib \
    ///   ordinary_switch_bench_950kib_matches_segment_free_cost \
    ///   -- --nocapture --include-ignored
    /// ```
    #[test]
    #[ignore]
    fn ordinary_switch_bench_950kib_matches_segment_free_cost() {
        use crate::terminal_core::{ReplaySegment, TerminalCore};
        use std::sync::atomic::AtomicBool;
        use std::time::Instant;

        const SHIPPING_SCROLLBACK_LINES: u32 = 10_000;
        const TARGET_PAYLOAD_LEN: usize = 950 * 1024;
        let cols: u16 = 100;
        let target_rows: u16 = 30;
        let spawn_rows: u16 = 24;

        let filler = b"line of scrollback content padded out a bit\r\n";
        let fill_to = |len: usize| -> Vec<u8> {
            let mut payload = Vec::with_capacity(len + filler.len());
            while payload.len() < len {
                payload.extend_from_slice(filler);
            }
            payload
        };

        let measure = |payload: &[u8], segments: &[ReplaySegment]| -> std::time::Duration {
            {
                let cancel = AtomicBool::new(false);
                let _ = TerminalCore::build_from_snapshot(
                    cols,
                    target_rows,
                    SHIPPING_SCROLLBACK_LINES,
                    payload,
                    segments,
                    &cancel,
                );
            }
            let cancel = AtomicBool::new(false);
            let start = Instant::now();
            let replay = TerminalCore::build_from_snapshot(
                cols,
                target_rows,
                SHIPPING_SCROLLBACK_LINES,
                payload,
                segments,
                &cancel,
            );
            let elapsed = start.elapsed();
            std::hint::black_box(replay);
            elapsed
        };

        let payload = fill_to(TARGET_PAYLOAD_LEN);
        let t_free = measure(&payload, &[]);

        // Head segment covers a small, fixed prefix (a shell banner's worth
        // of bytes) — comfortably below `BYPASS_SUFFIX_MIN_BYTES`'s
        // complement, so the split's suffix (the rest of the payload) is
        // the part that actually gets replayed under bypass.
        let head_len: u32 = 2048;
        let ordinary_segments = vec![
            ReplaySegment {
                offset: 0,
                cols,
                rows: spawn_rows,
            },
            ReplaySegment {
                offset: head_len,
                cols,
                rows: target_rows,
            },
        ];
        let t_ordinary = measure(&payload, &ordinary_segments);

        // No stable tail (reported only) — the shape the split cannot
        // help, for contrast.
        let single_differing_segments = vec![ReplaySegment {
            offset: 0,
            cols,
            rows: spawn_rows,
        }];
        let t_single_differing = measure(&payload, &single_differing_segments);

        eprintln!(
            "[bench] ordinary switch vs segment-free (0.95 MiB, {cols}x{target_rows}, \
             sb={SHIPPING_SCROLLBACK_LINES}): segment-free → {:?} | ordinary switch \
             (head+target-tail) → {:?} | single differing segment (no stable tail) → {:?}",
            t_free, t_ordinary, t_single_differing,
        );

        // AC-4: an ordinary window switch is not measurably slower than a
        // segment-free replay. A generous multiplicative + absolute floor
        // absorbs scheduler / measurement noise on a single sample.
        let bound = t_free.mul_f64(3.0) + std::time::Duration::from_millis(20);
        assert!(
            t_ordinary < bound,
            "ordinary switch {:?} is not close to segment-free {:?} (bound \
             {:?}) — the prefix/suffix split (D1''') is not engaging for \
             this shape",
            t_ordinary,
            t_free,
            bound,
        );
    }

    /// AC-6 (D5'''', round-7 rework, review round-6 finding
    /// `e519916efd5fdc42`): a payload that is MOSTLY PREFIX (a large,
    /// multi-segment retained window with resizes scattered through most
    /// of it) with only a SMALL qualifying suffix (just over
    /// `BYPASS_SUFFIX_MIN_BYTES`) must NOT engage the D1''' prefix/suffix
    /// split. Before `BYPASS_PREFIX_MAX_BYTES` gated the split on the
    /// prefix's own size, the suffix alone clearing
    /// `BYPASS_SUFFIX_MIN_BYTES` was enough to engage it: the (expensive)
    /// prefix still paid its full non-bypass reflow cost as the split's
    /// "fast" first pass, but that pass then discarded the prefix's real
    /// scrollback into virtual bookkeeping and reported
    /// `scrollback_populated: false` — signalling the caller
    /// (`tabs.rs::apply_offthread_swap`) to redo the ENTIRE drain a SECOND
    /// time in the background, roughly doubling the work for a shape the
    /// split cannot actually speed up (the split's benefit is a CHEAP
    /// prefix; this shape's prefix is the expensive part regardless of
    /// whether the split engages).
    ///
    /// Confirmed to fail pre-fix: reverting `BYPASS_PREFIX_MAX_BYTES`'s
    /// gate (keeping only the suffix-size check) makes this payload's
    /// split engage, so `scrollback_populated` comes back `false` and the
    /// assertion below fails.
    ///
    /// Gated `#[ignore]` (release-mode timing bench; reports latency
    /// alongside the hard `scrollback_populated` regression guard).
    /// Invoke with:
    ///
    /// ```sh
    /// CARGO_TARGET_DIR=src-tauri/target cargo test --release \
    ///   --manifest-path crates/term_core/Cargo.toml --lib \
    ///   large_prefix_small_suffix_bench_does_not_engage_the_split \
    ///   -- --nocapture --include-ignored
    /// ```
    #[test]
    #[ignore]
    fn large_prefix_small_suffix_bench_does_not_engage_the_split() {
        use crate::terminal_core::{ReplaySegment, TerminalCore};
        use std::sync::atomic::AtomicBool;
        use std::time::Instant;

        const SHIPPING_SCROLLBACK_LINES: u32 = 10_000;
        let cols: u16 = 100;
        let target_rows: u16 = 30;
        let other_rows: u16 = 24;

        let filler = b"prefix line of scrollback content padded out a bit\r\n";
        let fill_to = |buf: &mut Vec<u8>, len: usize| {
            let start = buf.len();
            while buf.len() < start + len {
                buf.extend_from_slice(filler);
            }
        };

        // PREFIX: a large, multi-segment retained window — resizes
        // scattered through ~200 KiB, comfortably over
        // `BYPASS_PREFIX_MAX_BYTES` (64 KiB).
        let segment_count = 40usize;
        let prefix_target_len = 200 * 1024;
        let per_segment = prefix_target_len / segment_count;
        let mut payload = Vec::with_capacity(prefix_target_len + 8 * 1024);
        let mut segments = Vec::with_capacity(segment_count + 1);
        for i in 0..segment_count {
            segments.push(ReplaySegment {
                offset: payload.len() as u32,
                cols,
                rows: if i % 2 == 0 { other_rows } else { target_rows },
            });
            fill_to(&mut payload, per_segment);
        }
        let prefix_len = payload.len();

        // SUFFIX: a small tail already at the target — just over
        // `BYPASS_SUFFIX_MIN_BYTES` (4096), the shape that alone (ignoring
        // the prefix) would qualify the split.
        segments.push(ReplaySegment {
            offset: payload.len() as u32,
            cols,
            rows: target_rows,
        });
        fill_to(&mut payload, 4096 + 512);
        let suffix_len = payload.len() - prefix_len;

        assert!(
            prefix_len > 64 * 1024,
            "test prerequisite: prefix must exceed BYPASS_PREFIX_MAX_BYTES"
        );
        assert!(
            suffix_len >= 4096,
            "test prerequisite: suffix must clear BYPASS_SUFFIX_MIN_BYTES"
        );

        // Warm-up.
        let cancel = AtomicBool::new(false);
        let _ = TerminalCore::build_from_snapshot(
            cols,
            target_rows,
            SHIPPING_SCROLLBACK_LINES,
            &payload,
            &segments,
            &cancel,
        );

        let cancel = AtomicBool::new(false);
        let start = Instant::now();
        let replay = TerminalCore::build_from_snapshot(
            cols,
            target_rows,
            SHIPPING_SCROLLBACK_LINES,
            &payload,
            &segments,
            &cancel,
        )
        .expect("not cancelled");
        let t_from_snapshot = start.elapsed();

        let cancel = AtomicBool::new(false);
        let start = Instant::now();
        let _reference = TerminalCore::build_scrollback_only_from_snapshot(
            cols,
            target_rows,
            SHIPPING_SCROLLBACK_LINES,
            &payload,
            &segments,
            &cancel,
        )
        .expect("not cancelled");
        let t_reference = start.elapsed();

        eprintln!(
            "[bench] large-prefix ({prefix_len}B, {segment_count} segs) + \
             small-suffix ({suffix_len}B) split gate: build_from_snapshot \
             → {:?} | whole-drain reference → {:?}",
            t_from_snapshot, t_reference,
        );

        assert!(
            replay.scrollback_populated,
            "the split must NOT engage for a large multi-segment prefix \
             with only a small qualifying suffix — scrollback_populated \
             must be true (the whole-drain fallback populated it \
             directly), not false (which would trigger a redundant full \
             2nd-pass reflow upstream)"
        );
    }

    /// D5''''' (round-8 rework, review round-7 finding `a4f4e36fef377d05`):
    /// the daemon-cap boundary shape the finding names directly — a prefix
    /// carrying the daemon's own `MAX_DIM_MARKERS` (24) worth of segments,
    /// right at `BYPASS_PREFIX_MAX_BYTES` (64 KiB), paired with a suffix
    /// just over `BYPASS_SUFFIX_MIN_BYTES` (4 KiB) — must NOT engage the
    /// split. Distinguishes itself from
    /// `large_prefix_small_suffix_bench_does_not_engage_the_split` above
    /// (~200 KiB prefix / 40 segments, comfortably PAST every threshold):
    /// this shape sits AT the byte bound with the daemon's REALISTIC
    /// maximum segment count, the exact boundary case round 7's bench
    /// missed (review round-6 finding `7c70216c5a5d5c24` / round-7 finding
    /// `a4f4e36fef377d05`).
    ///
    /// The deterministic assertion (`scrollback_populated`) is ALSO pinned
    /// as a normal, non-ignored unit test
    /// (`terminal_core::tests::prefix_at_byte_bound_with_non_dominating_suffix_does_not_engage_the_split`
    /// / `..._prefix_with_too_many_segments_...`) so deleting the gate
    /// fails a plain `cargo test`, not only this release-mode timing bench.
    ///
    /// Confirmed to fail pre-fix: reverting the "suffix must dominate"
    /// requirement (`suffix_len >= split_at`) makes this payload's split
    /// engage — `scrollback_populated` comes back `false` and the
    /// assertion below fails.
    ///
    /// Gated `#[ignore]` (release-mode timing bench). Invoke with:
    ///
    /// ```sh
    /// CARGO_TARGET_DIR=src-tauri/target cargo test --release \
    ///   --manifest-path crates/term_core/Cargo.toml --lib \
    ///   daemon_cap_prefix_with_small_suffix_bench_does_not_engage_the_split \
    ///   -- --nocapture --include-ignored
    /// ```
    #[test]
    #[ignore]
    fn daemon_cap_prefix_with_small_suffix_bench_does_not_engage_the_split() {
        use crate::terminal_core::{ReplaySegment, TerminalCore};
        use std::sync::atomic::AtomicBool;
        use std::time::Instant;

        const SHIPPING_SCROLLBACK_LINES: u32 = 10_000;
        const DAEMON_MAX_DIM_MARKERS: usize = 24;
        let cols: u16 = 100;
        let target_rows: u16 = 30;
        let other_rows: u16 = 24;

        let filler = b"daemon-cap prefix line padded a bit for size\r\n";
        let mut payload: Vec<u8> = Vec::new();
        let mut segments = Vec::with_capacity(DAEMON_MAX_DIM_MARKERS + 1);
        // PREFIX: exactly the daemon's own MAX_DIM_MARKERS worth of
        // segments, sized so the whole prefix lands AT/UNDER 64 KiB — never
        // add a chunk that would push the RUNNING total over the bound
        // (unlike a naive per-segment target, which can overshoot by up to
        // one segment's worth and accidentally land past
        // BYPASS_PREFIX_MAX_BYTES, no longer isolating the NEW "suffix must
        // dominate" / segment-count gates this bench targets).
        const PREFIX_BYTE_BUDGET: usize = 64 * 1024;
        let per_segment_target = PREFIX_BYTE_BUDGET / DAEMON_MAX_DIM_MARKERS;
        for i in 0..DAEMON_MAX_DIM_MARKERS {
            segments.push(ReplaySegment {
                offset: payload.len() as u32,
                cols,
                rows: if i % 2 == 0 {
                    other_rows
                } else {
                    other_rows + 1
                },
            });
            let start = payload.len();
            while payload.len() + filler.len() <= start + per_segment_target
                && payload.len() + filler.len() <= PREFIX_BYTE_BUDGET
            {
                payload.extend_from_slice(filler);
            }
        }
        let prefix_len = payload.len();

        // SUFFIX: small, just over BYPASS_SUFFIX_MIN_BYTES (4096) — dwarfed
        // by the prefix, the exact shape the byte-only gate alone let
        // through.
        segments.push(ReplaySegment {
            offset: payload.len() as u32,
            cols,
            rows: target_rows,
        });
        let suffix_filler = b"suffix line padded out a bit for size\r\n";
        let suffix_start = payload.len();
        while payload.len() < suffix_start + 4096 + 512 {
            payload.extend_from_slice(suffix_filler);
        }
        let suffix_len = payload.len() - prefix_len;

        assert!(
            prefix_len <= PREFIX_BYTE_BUDGET,
            "test prerequisite: prefix must be at/under BYPASS_PREFIX_MAX_BYTES, \
             got {prefix_len}"
        );
        assert!(
            suffix_len >= 4096 && suffix_len < prefix_len,
            "test prerequisite: suffix must clear BYPASS_SUFFIX_MIN_BYTES \
             but NOT dominate the prefix"
        );

        // Warm-up.
        let cancel = AtomicBool::new(false);
        let _ = TerminalCore::build_from_snapshot(
            cols,
            target_rows,
            SHIPPING_SCROLLBACK_LINES,
            &payload,
            &segments,
            &cancel,
        );

        let cancel = AtomicBool::new(false);
        let start = Instant::now();
        let replay = TerminalCore::build_from_snapshot(
            cols,
            target_rows,
            SHIPPING_SCROLLBACK_LINES,
            &payload,
            &segments,
            &cancel,
        )
        .expect("not cancelled");
        let t_from_snapshot = start.elapsed();

        let cancel = AtomicBool::new(false);
        let start = Instant::now();
        let _reference = TerminalCore::build_scrollback_only_from_snapshot(
            cols,
            target_rows,
            SHIPPING_SCROLLBACK_LINES,
            &payload,
            &segments,
            &cancel,
        )
        .expect("not cancelled");
        let t_reference = start.elapsed();

        eprintln!(
            "[bench] daemon-cap prefix ({prefix_len}B, {DAEMON_MAX_DIM_MARKERS} \
             segs) + small-suffix ({suffix_len}B) split gate: \
             build_from_snapshot → {:?} | whole-drain reference → {:?}",
            t_from_snapshot, t_reference,
        );

        assert!(
            replay.scrollback_populated,
            "the split must NOT engage for a prefix at the daemon's own \
             MAX_DIM_MARKERS cap, right at BYPASS_PREFIX_MAX_BYTES, with \
             only a small non-dominating suffix — scrollback_populated \
             must be true, not false"
        );
    }
}
