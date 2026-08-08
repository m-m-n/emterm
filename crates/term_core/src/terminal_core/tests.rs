use super::*;

mod grid;
mod head_middle_tail;
mod offthread_snapshot;
mod parse_modes;
mod replay_segments;
mod resize_markers;
mod scrollback_restore;
mod segment_budget;

/// Collect the observable grid text + cursor into a comparable shape so
/// the pure builder and the synchronous path can be asserted
/// grid-identical. The post-replay scrollback length is intentionally
/// excluded: per FR2, the snapshot-replay bypass leaves
/// `scrollback_count() == 0` on the built core (contents are not
/// repopulated), while the synchronous `reset_and_replay` path retains
/// up to `scrollback_capacity` rows of contents. The
/// observable bookkeeping that consumers depend on
/// (`SnapshotReplay.evicted_total` and mark `abs_row`/`evicted_total`)
/// is asserted separately in `test_build_from_snapshot_matches_reset_and_replay`.
fn grid_fingerprint(core: &TerminalCore) -> (Vec<String>, u16, u16) {
    let mut rows = Vec::with_capacity(core.rows() as usize);
    for r in 0..core.rows() {
        let mut line = String::new();
        for c in 0..core.cols() {
            line.push_str(&core.get_cell_char(c, r));
        }
        rows.push(line);
    }
    (rows, core.get_cursor_col(), core.get_cursor_row())
}
