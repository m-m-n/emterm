# mux-suppressed-output-round2-fixes: Decisions (DECISIONS.md)

Decision for each of the eight medium findings left `unresolved` in review
round 2 of mux-suppressed-output-fixes (PR #109). Each of FR1 through FR8
fixes exactly one finding, and the verdict for all eight is "resolved".

## Decision table

| stable_id | requirement | verdict | rationale | regression test |
|-----------|-------------|---------|-----------|-----------------|
| ecc48041b65a5380 | FR1 | resolved | With a full retention window, restart-position selection could choose an ESC that the client consumes as a charset designator byte (an ESC after `ESC (` / `ESC )` at offset 2 or later), so the scan fabricated a query from the bytes after it. Selection now excludes every ESC that may sit in a designator slot (offset 0, or preceded by `(` / `)` that is at offset 0 or preceded by ESC), chains included; the exclusion can only cause misses. | `src-tauri/src/mux/ipc/pty_spawn/suppressed_output.rs` (tests module): `round2_ecc48041_designator_slot_esc_is_never_a_window_restart_position` |
| dd56f3984c74cde1 | FR2 | resolved | `assemble_items` dropped color queries that overlap ring-written ranges because their bytes survive in the snapshot, but the client discards every response produced while replaying a snapshot, so a main-screen color query was never answered. The overlap check and `overlaps_ranges` are removed, and every color query completed in a suppressed chunk is delivered once after the snapshot, like CSI queries. | `src-tauri/src/mux/ipc/pty_spawn/tests.rs`: `round2_dd56f398_main_screen_color_query_is_answered_once_after_the_snapshot` |
| ae48e7cd98084c19 | FR3 | resolved | The write filter forgot that a feed ended with `ESC (` / `ESC )`, so the next feed treated an ESC designator byte as an OSC introducer and held bytes the client had already consumed; those bytes were later re-sent and completed a query that the raw stream never contains. The filter now keeps the awaiting-designator state separately from `pending` across feeds and consumes the first byte of the next feed as the designator, so split input matches the single-call result. | `src-tauri/src/mux/ipc/pty_spawn/tests.rs`: `round2_ae48e7cd_awaiting_designator_is_carried_across_feeds` |
| b600645f1fa94686 | FR4 | resolved | Screen-switch sequences (47/1047/1049 `h`/`l`) removed during main-screen range extraction were invisible to the write filter, although the client closes an in-progress OSC, DCS or APC and a trailing lone ESC at them, so a terminated sequence stayed in `pending` and was re-sent as the tail. The filter now treats a removed switch sequence as closing the pending construct at its position. | `src-tauri/src/mux/ipc/pty_spawn/tests.rs`: `round2_b600645f_removed_screen_switch_closes_the_pending_string` |
| f8b600bcc0ed55da | FR5 | resolved | The pending-based scan exclusion cut the scan at a single position (`tail_exclusion_start` and `limit_in_chunk`), which also hid queries and viewer launches in alternate-screen ranges between main-screen ranges. Only the range of the original chunk that corresponds to `pending` is now excluded, so items in intervening alternate-screen ranges are scanned and delivered. | `src-tauri/src/mux/ipc/pty_spawn/tests.rs`: `round2_f8b600bc_pending_exclusion_keeps_alt_screen_queries` |
| 03ccd5c7702db8db | FR6 | resolved | A viewer launch or color query that began in an earlier read, was carried over in `pending` and completed in the suppressed chunk reached neither the snapshot nor the replacement output when its start lay beyond the retention window. The write filter now hands over the completed sequence with its position mapped to original-chunk coordinates, and it is delivered once after the snapshot; deduplication removes only re-detection through the retention window. | `src-tauri/src/mux/ipc/pty_spawn/tests.rs`: `round2_03ccd5c7_carried_over_viewer_launch_is_delivered_once` |
| a93dffe30438a693 | FR7 | resolved | Replacement extraction recovered OSC numbers in base 10 while scrollback stripping matched the strings `777` / `9999`, so a launch such as `0777;emterm;markdown;...` stayed in the ring and snapshot and was also delivered by the replacement output, arriving twice. OSC number recovery and viewer-launch identification now live in a shared layer independent of `mux::ipc`, used by stripping and by extraction, each keeping its own selection of what to strip or deliver. | `src-tauri/src/mux/ipc/pty_spawn/client_parity_scan.rs` (tests module): `round2_a93dffe3_leading_zero_viewer_launch_reaches_the_client_once` |
| 3eccc254dd278b33 | FR8 | resolved | A main-screen visibility-restore snapshot without a dump block leaves the client parser in an incomplete-tail state (mid UTF-8, after `ESC (` / `ESC )`, or inside a CSI), and the replacement output re-sent the same tail, printing a replacement character or a stray `(`. The snapshot's trailing construct is now kept in the boundary record for its destination and passed to replacement assembly, which no longer re-sends a tail the snapshot already carried; snapshot bytes are unchanged. | `src-tauri/src/mux/ipc/pty_spawn/tests.rs`: `round2_3eccc254_visibility_restore_does_not_resend_a_tail_the_snapshot_carried` |

## Behavior-changing tests

Tests whose expectation changed on purpose (SPEC AC-8). Each was renamed to
describe the new behavior; no other existing test's expectation changed.

| old name | new name | file | reason |
|----------|----------|------|--------|
| `osc_color_query_inside_ring_written_ranges_is_not_redelivered` | `osc_color_query_inside_ring_written_ranges_is_redelivered_once` | `src-tauri/src/mux/ipc/pty_spawn/suppressed_output.rs` (tests module) | FR2: a color query inside ring-written ranges is delivered once, because the snapshot's responses are discarded. |
| `visible_reattach_redelivers_an_alt_screen_color_query_but_not_one_that_reached_the_ring` | `visible_reattach_redelivers_alt_screen_and_main_screen_color_queries_once` | `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | FR2: both the alternate-screen and the main-screen color query are answered once after a visible reattach. |
| `on_demand_snapshot_redelivers_an_alt_screen_color_query_but_not_one_that_reached_the_ring` | `on_demand_snapshot_redelivers_alt_screen_and_main_screen_color_queries_once` | `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | FR2: both the alternate-screen and the main-screen color query are answered once after an on-demand snapshot. |

## Review round 2 record

`feature-docs/mux-suppressed-output-fixes/reviews/round2.yaml` was not modified.
