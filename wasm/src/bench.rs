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
}
