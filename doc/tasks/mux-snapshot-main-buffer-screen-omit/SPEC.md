# Feature: mux Snapshot Main-Buffer Screen Omission

## Overview

Fix the apt-progress-bar trash that bleeds into log lines after a snapshot restore (same-tab click, cross-tab switch, window switch, or reattach) on the mux daemon path. Drop the daemon vt100 `contents_formatted()` dump from the snapshot bytes when the pane is on the main buffer; keep it when the pane is on the alt screen. The client's term_core rebuilds the main-buffer state by replaying scrollback bytes alone.

## Objectives

- Eliminate the trashed progress-bar / log merge that appears after a snapshot restore on main-buffer panes (e.g. after `apt install`).
- Preserve current snapshot behavior for alt-screen panes (vim / htop / less / man).
- Remove leftover investigation code (`[DECSTBM-trace]` warn logs, suspect dump, `probe_*` tests).
- Codify the main/alt snapshot-layout split in SPEC and module doc comments.

## User Stories

### US1: apt progress bar survives a tab round-trip
As a mux user, I want a same-tab click or cross-tab switch right after running `sudo apt install <pkg>` to render the saved history correctly, so that progress bars and log lines do not collapse into the same row.

**Acceptance Criteria:**
- [ ] After replay, apt progress-bar bytes (`█`/`▌`/`▏` glyphs and `[ N%]` text) appear on their own row, not glued to the next "Setting up X" line.
- [ ] Repeating the round-trip several times does not progressively degrade the layout.

### US2: alt-screen TUIs restore cleanly
As a TUI user, I want vim/htop/less/man panes to keep rendering their full-screen UI after a tab round-trip, so that there is no regression from the main-buffer fix.

**Acceptance Criteria:**
- [ ] Alt-screen pane content (cursor position, status line, viewport) is identical before and after a tab round-trip.

### US3: investigation noise is gone
As a maintainer, I want the temporary `[DECSTBM-trace]` warn logs and `probe_*` tests cleared from the repo, so that the codebase reflects only the production logic that ships.

**Acceptance Criteria:**
- [ ] No `[DECSTBM-trace]` literal remains anywhere under `src-tauri/` or `crates/`.
- [ ] No `probe_*` test function in `src-tauri/src/mux/ipc/reattach.rs`.
- [ ] No suspect-dump filesystem writes in `build_snapshot_bytes`.

## Technical Requirements

### Functional Requirements

- **FR1 (Main-buffer snapshot omits screen dump):** `build_snapshot_bytes(scrollback, screen, alt_screen=false)` MUST emit
  `ESC[3J ESC[H ESC[2J` + `strip_replayable_rich_content(scrollback)` + `ESC[?1049l`, with no `screen` bytes in between.
- **FR2 (Alt-screen snapshot keeps screen dump):** `build_snapshot_bytes(scrollback, screen, alt_screen=true)` MUST emit
  `ESC[3J ESC[H ESC[2J` + `strip_replayable_rich_content(scrollback)` + `screen` + `ESC[?1049h`, identical to the pre-fix layout.
- **FR3 (Doc comments reflect the split):** The doc comments on `build_snapshot_bytes`, `build_shadow_parser_snapshot`, and `handle_request_pane_snapshot` MUST document the main/alt split and the rationale (`scrollback` carries the full PTY byte history for main-buffer; alt-screen output is not written to scrollback so `contents_formatted()` is the only source).
- **FR4 (Remove investigation code):** All `[DECSTBM-trace]` warn logs and suspect-dump filesystem writes MUST be removed from:
  - `crates/term_core/src/terminal_core.rs::set_scroll_region`
  - `crates/term_core/src/reflow.rs::resize_post_cleanup`
  - `src-tauri/src/tabs.rs::reset_frame_for_replay`
  - `src-tauri/src/tabs.rs::apply_offthread_swap`
  - `src-tauri/src/mux/ipc/reattach.rs::build_snapshot_bytes`
  - All `probe_*` tests in `src-tauri/src/mux/ipc/reattach.rs` (PROBE 2–6 — 5 functions).

### Non-Functional Requirements

- **NFR1 - Performance:** Main-buffer snapshot byte size MUST decrease by the size of the omitted `contents_formatted()` dump. Client replay cost stays proportional to the scrollback length (already bounded by the 2 MiB ring and handled by the existing off-thread `build_from_snapshot` path).
- **NFR2 - Compatibility:** The mux IPC wire protocol MUST NOT change. `RequestPaneSnapshot` / `PtyOutput` framing, codecs, and message types are unchanged; only the internal byte composition of the snapshot payload is altered.
- **NFR3 - Maintainability:** Snapshot layout MUST be expressible as a single doc comment that any future contributor can read without re-deriving the alt-vs-main rationale.

## Implementation Approach

### Architecture

The change is local to one function and its callers:

```
RequestPaneSnapshot (client)
        │
        ▼
handle_request_pane_snapshot (daemon)
        │
        ▼
build_shadow_parser_snapshot
        │   (reads scrollback bytes + vt100 contents_formatted())
        ▼
build_snapshot_bytes(scrollback, screen, alt_screen)
        │   ← FIX: branch on alt_screen
        ▼
PtyOutput (wire)
        │
        ▼
client tabs.rs reset_and_replay → term_core
```

### Data Flow (post-fix)

```
alt_screen == false (main-buffer):
    [ESC[3J ESC[H ESC[2J] + strip(scrollback) + [ESC[?1049l]
    → client term_core replays scrollback bytes from a fresh core, reconstructing
      the visible viewport without daemon vt100 trash leaking in.

alt_screen == true (TUI on alt buffer):
    [ESC[3J ESC[H ESC[2J] + strip(scrollback) + screen + [ESC[?1049h]
    → client term_core replays scrollback (which never holds alt-buffer output),
      then the daemon vt100 dump paints the visible alt-screen UI, then 1049h
      switches the client into alt mode so scrolling/keys behave correctly.
```

### Code Change Sketch (illustrative — non-binding)

```rust
pub(super) fn build_snapshot_bytes(scrollback: &[u8], screen: &[u8], alt_screen: bool) -> Vec<u8> {
    let scrollback = strip_replayable_rich_content(scrollback);
    let alt_mode: &[u8] = if alt_screen { b"\x1b[?1049h" } else { b"\x1b[?1049l" };
    // Main-buffer panes: the scrollback bytes are the complete PTY byte history
    // (including DECSTBM region toggles and progress redraws), so a fresh
    // client term_core replays to the correct visible state on its own. The
    // daemon vt100 dump is omitted to avoid pulling in trashed cells caused by
    // a vt100 resize race that lands progress-bar glyphs on the wrong rows.
    //
    // Alt-screen panes: alt-buffer output is *not* written to scrollback
    // (see pty_spawn.rs:373), so the daemon vt100 dump is the only way to
    // restore the visible TUI surface.
    let screen_to_include: &[u8] = if alt_screen { screen } else { &[] };
    let mut combined = Vec::with_capacity(
        SNAPSHOT_CLEAR_HOME.len() + scrollback.len() + screen_to_include.len() + alt_mode.len(),
    );
    combined.extend_from_slice(SNAPSHOT_CLEAR_HOME);
    combined.extend_from_slice(&scrollback);
    combined.extend_from_slice(screen_to_include);
    combined.extend_from_slice(alt_mode);
    combined
}
```

### Dependencies

**Internal Dependencies:**
- `crate::mux::scrollback_filter::strip_replayable_rich_content` — unchanged, still applied to both modes.
- `vt100` 0.16.2 — unchanged. Continues to drive the daemon's shadow terminal; we just stop including its `contents_formatted()` output in main-buffer snapshots.
- `crate::tabs::reset_and_replay` (client) — unchanged.
- `crate::tabs::build_from_snapshot` off-thread path — unchanged.

**External Dependencies:**
- None added or removed.

### File Structure (affected only)

```
src-tauri/src/mux/ipc/reattach.rs          # build_snapshot_bytes branch + doc + test updates + probe_* removal
src-tauri/src/tabs.rs                      # remove [DECSTBM-trace] logs
crates/term_core/src/terminal_core.rs      # remove [DECSTBM-trace] log in set_scroll_region
crates/term_core/src/reflow.rs             # remove [DECSTBM-trace] log in resize_post_cleanup
doc/tasks/mux-snapshot-main-buffer-screen-omit/
    SPEC.md
    要件定義書.md
    IMPLEMENTATION.md
    VERIFICATION.md
    sdd.yaml
```

## Test Scenarios

### Unit Tests

- [ ] `build_snapshot_bytes_main_buffer_omits_screen_part` (new): Given `alt_screen=false`, the returned bytes consist of `SNAPSHOT_CLEAR_HOME` + stripped scrollback + `ESC[?1049l`. The bytes MUST NOT contain the `screen` slice.
- [ ] `build_snapshot_bytes_alt_screen_includes_screen_part` (rewrite of `build_snapshot_bytes_layout_is_clear_scrollback_screen` / `build_shadow_parser_snapshot_emits_scrollback_before_screen`): Given `alt_screen=true`, the returned bytes consist of `SNAPSHOT_CLEAR_HOME` + stripped scrollback + screen + `ESC[?1049h`.
- [ ] Adjacent existing tests (`build_snapshot_bytes_strips_rich_content_from_scrollback`, `build_shadow_parser_snapshot_emits_scrollback_before_screen`, `build_shadow_parser_snapshot_empty_scrollback_is_clear_plus_shadow`) continue to pass after being refactored to target a single branch of the new contract.

### Integration Tests

- [ ] None new. The mux IPC integration tests in `src-tauri/tests/mux_throughput.rs` MUST continue to pass.

### E2E Tests

**Existing E2E tests**: None (no `docker-compose.e2e.yml`, no `e2e-tests/`).
**Run command**: Not applicable.
- [ ] Manual verification (see Section "Manual Verification").

### Edge Cases

- [ ] Pane with empty scrollback + `alt_screen=false` → snapshot bytes = `SNAPSHOT_CLEAR_HOME` + `ESC[?1049l` (no screen). Client replay leaves a blank fresh core.
- [ ] Pane with empty scrollback + `alt_screen=true` → snapshot bytes = `SNAPSHOT_CLEAR_HOME` + `screen` + `ESC[?1049h`. Client replay paints the TUI surface and switches to alt mode.
- [ ] Scrollback bytes exceed the 2 MiB ring — old bytes are dropped before snapshot is built (pre-existing behavior; not changed by this fix). The reduced fidelity for very long sessions is documented as a known limitation.

### Performance Tests

- [ ] No new perf test required. The change strictly reduces main-buffer snapshot payload size and adds a single boolean branch.

## Security Considerations

- **Input Validation:** Not applicable; the change rearranges internal byte composition only.
- **Data Protection:** Not applicable.

## Error Handling

This change does not introduce new error paths. `build_snapshot_bytes` is infallible; `Vec::with_capacity` may panic only on OOM, which is the existing contract.

## Performance Optimization

### Performance Goals

- Main-buffer snapshot byte size strictly decreases vs. pre-fix (by the size of the omitted vt100 dump).
- Client replay cost remains bounded by the existing 2 MiB scrollback ring.

### Optimization Strategies

- Pre-fix capacity hint for the `Vec` accounts for the chosen `screen_to_include`, avoiding any spurious reallocation when the screen is omitted.

## Manual Verification

After the fix lands, the following manual scenarios MUST pass on the developer host:

1. Run `sudo apt reinstall <package>` in an emterm mux tab.
2. Click the same tab while apt is running and again right after it finishes → no row collapse.
3. Switch to another tab and back during/after apt → no row collapse.
4. Run an alt-screen TUI (vim / htop / less / man) and perform the same round-trip → alt-screen contents are restored cleanly.
5. Inspect `~/.local/share/net.laser5.app.emterm/logs/emterm.log` → no `[DECSTBM-trace]` lines.

## Success Criteria

- [ ] FR1 / FR2 are implemented and covered by unit tests.
- [ ] FR3 (doc comments) is reflected in the touched modules.
- [ ] FR4 (investigation-code removal) is complete and verifiable via `grep -R DECSTBM-trace` returning no hits across `src-tauri/` and `crates/`.
- [ ] All `--lib` tests in `src-tauri` and `crates/term_core` pass with `CARGO_TARGET_DIR=src-tauri/target`.
- [ ] Manual verification scenarios 1–5 pass.

## Known Limitations

- Sessions whose scrollback exceeds the 2 MiB ring lose the oldest bytes; the pre-fix daemon-vt100-dump path also lost rows beyond the visible viewport, so user-visible behavior on this boundary is equivalent.
- The underlying daemon-vt100 resize-race trash is not addressed by this fix. It is captured as a follow-up task; main-buffer rendering is no longer affected because the trashed dump is not read.

## Open Questions

> **Note**: 未解決の要件は sdd.yaml で `status: tbd` として管理されています。`/em-sdd:sdd.2-create-plan` の実行前に解決してください。

- None.

## References

- Discussion document: `tmp/discussion-mux-snapshot-scroll-region.md` (採用方針確定済み, 2026-06-28)
- Background: `MEMORY: project_mux_snapshot_scroll_region_loss`
- Snapshot architecture context: `MEMORY: project_mux_altscreen_scroll_architecture`, `MEMORY: project_mux_snapshot_viewer_relaunch`
- Code touchpoints:
  - `src-tauri/src/mux/ipc/reattach.rs` — `build_snapshot_bytes`, `build_shadow_parser_snapshot`, `handle_request_pane_snapshot`, `probe_*` tests
  - `src-tauri/src/tabs.rs` — `reset_frame_for_replay`, `apply_offthread_swap`
  - `crates/term_core/src/terminal_core.rs` — `set_scroll_region`
  - `crates/term_core/src/reflow.rs` — `resize_post_cleanup`
