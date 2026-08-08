use super::*;

// ── task0004 D4: per-concern next-deadline getters ────────────────

#[test]
fn next_bell_deadline_none_when_no_bell_active() {
    let app = App::new();
    assert_eq!(app.next_bell_deadline(), None);
}

#[test]
fn next_bell_deadline_is_started_plus_flash_duration() {
    let mut app = App::new();
    let started = Instant::now();
    app.visual_bell_started = Some(started);
    assert_eq!(
        app.next_bell_deadline(),
        Some(started + Duration::from_millis(BELL_FLASH_MS))
    );
}

#[test]
fn next_toast_deadline_none_when_no_toast_active() {
    let app = App::new();
    assert_eq!(app.next_toast_deadline(), None);
}

#[test]
fn next_toast_deadline_some_when_restart_toast_active() {
    let mut app = App::new();
    app.restart_toast.arm(0.0);
    assert!(app.next_toast_deadline().is_some());
}

#[test]
fn next_toast_deadline_some_when_sftp_toast_active() {
    let mut app = App::new();
    app.sftp_ui.toasts.toasts.push(crate::sftp::ui::Toast {
        session_id: "s1".to_string(),
        file_name: "f.txt".to_string(),
        status: crate::sftp::SftpUploadStatus::Uploading,
        bytes_transferred: 0,
        total_bytes: 100,
        error_message: None,
        dismiss_at: None,
    });
    assert!(app.next_toast_deadline().is_some());
}

#[test]
fn next_blink_deadline_none_when_no_active_tab() {
    let app = App::new();
    assert_eq!(app.tabs.len(), 0, "no tab spawned — precondition");
    assert_eq!(app.next_blink_deadline(), None);
}

#[test]
fn next_blink_deadline_none_when_window_unfocused() {
    let mut app = App::new();
    app.spawn_initial_tab();
    app.tabs[0].core.lock().set_cursor_blink(true);
    app.window_focused = false;
    assert_eq!(
        app.next_blink_deadline(),
        None,
        "AC-2: focus loss must not schedule a blink wakeup"
    );
}

#[test]
fn next_blink_deadline_none_when_blink_disabled() {
    let mut app = App::new();
    app.spawn_initial_tab();
    app.tabs[0].core.lock().set_cursor_blink(false);
    app.window_focused = true;
    assert_eq!(
        app.next_blink_deadline(),
        None,
        "AC-2: blink disabled must never schedule a periodic wakeup"
    );
}

#[test]
fn next_blink_deadline_some_after_blink_started_when_enabled_and_focused() {
    let mut app = App::new();
    app.spawn_initial_tab();
    app.tabs[0].core.lock().set_cursor_blink(true);
    app.window_focused = true;
    let deadline = app
        .next_blink_deadline()
        .expect("blink enabled + focused + visible must yield a deadline");
    assert!(
        deadline > app.blink_started,
        "the next flip must lie strictly after the blink reference instant"
    );
}

/// AC-1: a frame with no content change, no cursor move, no blink
/// flip, no selection change, and no scroll returns an empty dirty
/// set — the render-skip decision in `window_host.rs` can only fire
/// when this is actually empty rather than perpetually `[cursor_row]`.
#[test]
fn dirty_set_empty_when_nothing_changed() {
    let mut core = fresh_core(20, 5);
    // Blink is on by default; disable it so the blink-phase check
    // can never introduce timing-dependent flakiness in this test —
    // the phase comparison is skipped entirely when blink is off.
    core.set_cursor_blink(false);
    let app = app_with_cleared_state(&mut core);
    // Cursor at (0,0) unchanged, no selection, nothing written.
    let set = app.dirty_rows_this_frame(&core);
    assert!(set.is_empty(), "expected empty dirty set, got {set:?}");
}

/// AC-2: moving the cursor dirties exactly the old row and the new
/// row; once the move is recorded, a subsequent stationary frame goes
/// back to empty (no permanent cursor-row stowaway).
#[test]
fn dirty_set_includes_cursor_move_origin_and_destination() {
    let mut core = fresh_core(20, 5);
    core.set_cursor_blink(false); // isolate cursor-move behavior from blink
    let mut app = app_with_cleared_state(&mut core);
    // Move cursor to row 3 via CSI Cursor Position (1-based).
    core.process_pty_data(b"\x1b[4;1H");
    core.clear_dirty(); // simulate that the write itself didn't touch cells
    // App still has previous_cursor = (0, 0) from initial record.
    let set = app.dirty_rows_this_frame(&core);
    assert_eq!(
        set,
        vec![0, 3],
        "exactly the vacated row and the destination row should be dirty"
    );
    // Record then ask again — cursor history is now (3, x) and the
    // cursor hasn't moved since, so the set goes back to empty.
    app.record_render_state(&mut core);
    let set2 = app.dirty_rows_this_frame(&core);
    assert!(
        set2.is_empty(),
        "stationary cursor after the move must not stow away a row, got {set2:?}"
    );
}

/// AC-3 (blink enabled): the cursor row is dirtied only on the frame
/// where the blink phase actually flips, not on every frame.
#[test]
fn dirty_set_includes_cursor_row_only_on_blink_phase_flip() {
    let mut core = fresh_core(20, 5);
    core.set_cursor_blink(true);
    let mut app = app_with_cleared_state(&mut core);
    // Immediately after recording, the phase has not flipped — the
    // dirty set stays empty (no cursor move, no blink flip yet).
    let set_before_flip = app.dirty_rows_this_frame(&core);
    assert!(
        set_before_flip.is_empty(),
        "no blink flip yet — expected empty, got {set_before_flip:?}"
    );
    // Back-date the blink reference so the phase has crossed into its
    // other half-cycle relative to the snapshot taken by
    // `record_render_state` above.
    app.blink_started = Instant::now()
        .checked_sub(Duration::from_millis(BLINK_HALF_MS as u64 + 10))
        .expect("test clock too close to process start to back-date");
    let set_after_flip = app.dirty_rows_this_frame(&core);
    assert_eq!(
        set_after_flip,
        vec![0],
        "blink flip must dirty exactly the cursor row"
    );
}

/// AC-3 (blink disabled): no blink-driven push ever occurs, no matter
/// how far the (unused) blink clock is back-dated.
#[test]
fn dirty_set_never_pushes_blink_row_when_blink_disabled() {
    let mut core = fresh_core(20, 5);
    core.set_cursor_blink(false);
    let mut app = app_with_cleared_state(&mut core);
    app.blink_started = Instant::now()
        .checked_sub(Duration::from_millis(BLINK_HALF_MS as u64 * 5))
        .expect("test clock too close to process start to back-date");
    let set = app.dirty_rows_this_frame(&core);
    assert!(
        set.is_empty(),
        "blink disabled must never push a blink-driven dirty row, got {set:?}"
    );
}

/// AC-4: PTY output dirties only the rows it actually touched — a
/// content edit elsewhere on screen must not smuggle the (unmoved)
/// cursor row into the set. Exercised via save/restore cursor (`ESC 7`
/// / `ESC 8`) so the cursor ends the frame exactly where it started.
#[test]
fn dirty_set_after_pty_write_excludes_stationary_cursor_row() {
    let mut core = fresh_core(20, 5);
    core.set_cursor_blink(false);
    let app = app_with_cleared_state(&mut core);
    // Cursor starts at (0, 0). Save it, write to row 2 elsewhere, then
    // restore — the cursor ends this frame at (0, 0), unmoved.
    core.process_pty_data(b"\x1b7"); // DECSC: save cursor
    core.process_pty_data(b"\x1b[3;1Hworld"); // move to row 2, write
    core.process_pty_data(b"\x1b8"); // DECRC: restore cursor
    assert_eq!(
        (core.get_cursor_row(), core.get_cursor_col()),
        (0, 0),
        "cursor must be restored to its starting cell"
    );
    let set = app.dirty_rows_this_frame(&core);
    assert_eq!(
        set,
        vec![2],
        "only the edited row should be dirty — no stationary-cursor stowaway"
    );
}

#[test]
fn dirty_set_includes_selection_extent_rows() {
    let mut core = fresh_core(20, 5);
    let mut app = app_with_cleared_state(&mut core);
    app.selection = Some(Selection {
        anchor: Pos { row: 1, col: 0 },
        extent: Pos { row: 3, col: 0 },
        mode: SelectionMode::Character,
        origin: Pos { row: 1, col: 0 },
    });
    let set = app.dirty_rows_this_frame(&core);
    // 0 = cursor, 1..=3 = selection.
    assert!(set.contains(&1));
    assert!(set.contains(&2));
    assert!(set.contains(&3));
}

#[test]
fn dirty_set_includes_previous_selection_rows_after_shrink() {
    let mut core = fresh_core(20, 5);
    let mut app = app_with_cleared_state(&mut core);
    // Frame 1: select rows 1..=3.
    app.selection = Some(Selection {
        anchor: Pos { row: 1, col: 0 },
        extent: Pos { row: 3, col: 0 },
        mode: SelectionMode::Character,
        origin: Pos { row: 1, col: 0 },
    });
    let _ = app.dirty_rows_this_frame(&core);
    app.record_render_state(&mut core);
    // Frame 2: shrink to row 1 only.
    app.selection = Some(Selection {
        anchor: Pos { row: 1, col: 0 },
        extent: Pos { row: 1, col: 0 },
        mode: SelectionMode::Character,
        origin: Pos { row: 1, col: 0 },
    });
    let set = app.dirty_rows_this_frame(&core);
    assert!(set.contains(&1));
    // Rows 2 and 3 must redraw to clear the old highlight.
    assert!(set.contains(&2), "vacated selection row 2 must redraw");
    assert!(set.contains(&3), "vacated selection row 3 must redraw");
}

#[test]
fn force_full_redraw_returns_all_rows() {
    let mut core = fresh_core(10, 4);
    let mut app = app_with_cleared_state(&mut core);
    app.force_full_redraw = true;
    let set = app.dirty_rows_this_frame(&core);
    assert_eq!(set, vec![0, 1, 2, 3]);
}

#[test]
fn needs_full_redraw_returns_all_rows_until_recorded() {
    let core = fresh_core(10, 3);
    let app = App::new();
    // Default state: needs_full_redraw = true.
    let set = app.dirty_rows_this_frame(&core);
    assert_eq!(set, vec![0, 1, 2]);
}

#[test]
fn dirty_set_after_single_line_write() {
    let mut core = fresh_core(20, 5);
    let app = app_with_cleared_state(&mut core);
    // Write text to row 0 (cursor was at (0,0)).
    core.process_pty_data(b"hello");
    let set = app.dirty_rows_this_frame(&core);
    // Row 0 was edited → in get_dirty_rows. Cursor stayed on row 0.
    assert_eq!(set, vec![0]);
    assert!(
        set.len() < core.rows() as usize,
        "should not be full redraw"
    );
}
