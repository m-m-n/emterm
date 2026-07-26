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
    /// `6c650908ea8e95e9`): reproduces the round-4 measurement methodology —
    /// a ~0.95 MiB snapshot replayed through `TerminalCore::build_from_snapshot`
    /// at the shipping scrollback default, varying segment count — and
    /// asserts replay stays fast at the segment-count bound this task's fix
    /// enforces daemon-side
    /// (`src-tauri/src/mux/scrollback_buffer.rs::MAX_DIM_MARKERS`, currently
    /// 16). `term_core` has no dependency on the daemon crate, so that
    /// value is duplicated here as a literal, cross-referenced by name —
    /// keep the two in sync if either changes.
    ///
    /// Round-4's raw (unbounded) measurement: segments=0 → 134 ms / 5 →
    /// 176 ms / 20 → 272 ms / 30 → 2078 ms / 50 → 3322 ms / 80 → 5350 ms —
    /// replay cost jumps sharply once segment count crosses roughly 20-30.
    /// This task's fix does not change `replay_segments`' per-call reflow
    /// cost; instead it keeps every REAL snapshot's segment count on the
    /// cheap side of that cliff by bounding what the daemon ever records
    /// (see `scrollback_buffer.rs`'s `MAX_DIM_MARKERS` tests for the
    /// recording-side guarantee). This bench validates that choice
    /// empirically: replay at the FULL bound (16 segments) stays under 1
    /// second — comfortably under the several-second blowup the unbounded
    /// case measured — and going from a quarter of the bound to the full
    /// bound (4x the segment count) does not multiply cost anywhere near
    /// the ~8x jump round-4 measured for only a 1.5x count increase (20 →
    /// 30), ruling out the same superlinear blowup within the bounded
    /// range.
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
        const DAEMON_SEGMENT_CAP: usize = 16;
        // Mirrors `src-tauri/src/settings.rs::DEFAULT_SCROLLBACK_LINES`.
        const SHIPPING_SCROLLBACK_LINES: u32 = 10_000;
        const TARGET_PAYLOAD_LEN: usize = 950 * 1024;
        let cols: u16 = 100;
        let rows_a: u16 = 30;
        let rows_b: u16 = 24;

        let build_payload = |segment_count: usize| -> (Vec<u8>, Vec<ReplaySegment>) {
            let filler = b"line of scrollback content padded out a bit\r\n";
            if segment_count == 0 {
                let mut payload = Vec::with_capacity(TARGET_PAYLOAD_LEN + filler.len());
                while payload.len() < TARGET_PAYLOAD_LEN {
                    payload.extend_from_slice(filler);
                }
                return (payload, Vec::new());
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

        let measure = |segment_count: usize| -> std::time::Duration {
            let (payload, segments) = build_payload(segment_count);
            // Warm-up.
            {
                let cancel = AtomicBool::new(false);
                let _ = TerminalCore::build_from_snapshot(
                    cols,
                    rows_a,
                    SHIPPING_SCROLLBACK_LINES,
                    &payload,
                    &segments,
                    &cancel,
                );
            }
            let cancel = AtomicBool::new(false);
            let start = Instant::now();
            let replay = TerminalCore::build_from_snapshot(
                cols,
                rows_a,
                SHIPPING_SCROLLBACK_LINES,
                &payload,
                &segments,
                &cancel,
            );
            let elapsed = start.elapsed();
            std::hint::black_box(replay);
            elapsed
        };

        let t_baseline = measure(0);
        let t_quarter = measure(DAEMON_SEGMENT_CAP / 4);
        let t_cap = measure(DAEMON_SEGMENT_CAP);

        eprintln!(
            "[bench] segment-bounded replay (0.95 MiB, {cols}x{rows_a}, sb={SHIPPING_SCROLLBACK_LINES}): \
             0 segs → {:?} | {} segs → {:?} | {} segs (daemon cap) → {:?}",
            t_baseline,
            DAEMON_SEGMENT_CAP / 4,
            t_quarter,
            DAEMON_SEGMENT_CAP,
            t_cap,
        );

        let bound = std::time::Duration::from_millis(1000);
        assert!(
            t_cap < bound,
            "replay at the daemon's segment cap ({DAEMON_SEGMENT_CAP}) took \
             {:?}, at or above the {:?} NFR1 bound — round-4 measured 2+ \
             seconds once segment count crossed ~30 unbounded; this bound \
             exists precisely so a real snapshot (capped at \
             {DAEMON_SEGMENT_CAP}) never gets there",
            t_cap,
            bound,
        );
        // Ratio check: going from a quarter of the cap to the full cap (4x
        // the segment count) must not multiply cost anywhere near the ~8x
        // jump round-4 measured going from 20 to 30 segments (only a 1.5x
        // count increase). A generous 6x ceiling still catches a
        // reintroduced superlinear blowup while tolerating ordinary
        // linear-ish scaling plus measurement noise.
        let ratio = t_cap.as_secs_f64() / t_quarter.as_secs_f64().max(0.0001);
        assert!(
            ratio < 6.0,
            "cost at the full daemon cap ({:?}) is {:.1}x the cost at a \
             quarter of it ({:?}) — a ratio this high for only a 4x \
             segment-count increase would reproduce the superlinear blowup \
             this bound exists to prevent",
            t_cap,
            ratio,
            t_quarter,
        );
    }
}
