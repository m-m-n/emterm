use super::*;
use crate::agent_status::{AgentState, AgentStatusEvent};
use crate::mux::session::pane::PaneOutputTarget;
use crate::prompts::PromptMarkKind;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::sync::{Arc as StdArc, Mutex as StdMutex};
use tokio::sync::mpsc;

fn test_output_target() -> SharedOutputTarget {
    let (tx, _rx) = mpsc::channel(1);
    StdArc::new(StdMutex::new(PaneOutputTarget::Connected(tx)))
}

fn test_restore_channels() -> (
    TitleChangeSender,
    NotificationSender,
    AgentStatusReportSender,
    SharedPaneExitSender,
) {
    let (title_tx, _title_rx) = mpsc::channel(16);
    let (notification_tx, _notification_rx) = mpsc::channel(16);
    let (agent_status_tx, _agent_status_rx) = mpsc::channel(16);
    let (pane_exit_tx, _pane_exit_rx) = mpsc::channel(16);
    let pane_exit_sender: SharedPaneExitSender = StdArc::new(StdMutex::new(Some(pane_exit_tx)));
    (title_tx, notification_tx, agent_status_tx, pane_exit_sender)
}

fn has_cloexec(fd: RawFd) -> bool {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    assert!(flags >= 0, "fcntl(F_GETFD) failed");
    flags & libc::FD_CLOEXEC != 0
}

/// `(st_dev, st_ino)` for `fd`, or `None` if `fd` is not currently open.
/// Used (rather than a plain "is this fd number valid" check) so a test
/// asserting a descriptor was closed stays correct even when `cargo
/// test`'s parallel threads — sharing one process-wide fd table — hand
/// that freed NUMBER to an unrelated descriptor from a concurrently
/// running test before the assertion runs (mirrors
/// `inherited_pty.rs::tests::stat_rdev`'s identical rationale, adapted to
/// regular files via device+inode identity instead of a device's rdev).
fn stat_dev_ino(fd: RawFd) -> Option<(u64, u64)> {
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::fstat(fd, &mut st) };
    if rc == 0 {
        Some((st.st_dev, st.st_ino))
    } else {
        None
    }
}

fn minimal_document() -> HandoffDocument {
    HandoffDocument {
        schema_version: HANDOFF_SCHEMA_VERSION,
        incarnation: "deadbeef".to_string(),
        listen_fd: 3,
        next_session_id: 1,
        next_pane_id: 1,
        sessions: Vec::new(),
    }
}

// ── AC-1 / AC-3: tree / ordering / active selections / counters /
// incarnation round-trip ──────────────────────────────────────────────

#[test]
fn snapshot_then_restore_round_trips_tree_ordering_active_selections_counters_and_incarnation() {
    let mut mgr = SessionManager::new();
    let sid1 = mgr.create_session("first".to_string());
    let wid1a = mgr.create_window(sid1, "a".to_string()).unwrap();
    let wid1b = mgr.create_window(sid1, "b".to_string()).unwrap();
    let pid1 = mgr.alloc_pane_id();
    let pid2 = mgr.alloc_pane_id();
    {
        let session = mgr.get_session_mut(sid1).unwrap();
        session
            .windows
            .get_mut(&wid1a)
            .unwrap()
            .add_pane(MuxPane::new_test(pid1, 80, 24, test_output_target()));
        session
            .windows
            .get_mut(&wid1b)
            .unwrap()
            .add_pane(MuxPane::new_test(pid2, 100, 30, test_output_target()));
        // Reorder so wid1b becomes first — proves window_order (not
        // creation order) is what round-trips.
        assert!(session.move_window(wid1b, 0));
    }
    let sid2 = mgr.create_session("second".to_string());
    let wid2 = mgr.create_window(sid2, "c".to_string()).unwrap();
    let pid3 = mgr.alloc_pane_id();
    mgr.get_session_mut(sid2)
        .unwrap()
        .windows
        .get_mut(&wid2)
        .unwrap()
        .add_pane(MuxPane::new_test(pid3, 80, 24, test_output_target()));

    let incarnation = mgr.incarnation().to_string();
    let next_session_id = mgr.next_session_id_counter();
    let next_pane_id = mgr.next_pane_id_counter();

    let dir = tempfile::tempdir().expect("tempdir");
    let socket_path = dir.path().join("mux-default.sock");
    let listen_file = tempfile::NamedTempFile::new_in(dir.path()).expect("listen fd stand-in");
    let listen_fd = listen_file.as_raw_fd();

    let document = snapshot(&mgr, listen_fd, &socket_path).expect("snapshot must succeed");

    assert_eq!(document.incarnation, incarnation);
    assert_eq!(document.next_session_id, next_session_id);
    assert_eq!(document.next_pane_id, next_pane_id);
    assert_eq!(document.sessions.len(), 2);
    assert_eq!(document.sessions[0].id, sid1);
    assert_eq!(
        document.sessions[0].window_order,
        vec![wid1b, wid1a],
        "reordered window_order must be preserved"
    );
    assert_eq!(
        document.sessions[0].active_window_id,
        mgr.get_session(sid1).unwrap().active_window_id
    );
    assert_eq!(document.sessions[1].id, sid2);

    let (title_tx, notification_tx, agent_status_tx, pane_exit_sender) = test_restore_channels();
    let mut restored = restore(
        &document,
        &title_tx,
        &notification_tx,
        &agent_status_tx,
        &pane_exit_sender,
    );

    assert_eq!(restored.incarnation(), incarnation);
    assert_eq!(restored.next_session_id_counter(), next_session_id);
    assert_eq!(restored.next_pane_id_counter(), next_pane_id);
    assert_eq!(
        restored.alloc_pane_id(),
        next_pane_id,
        "next allocated pane id must continue the original sequence"
    );

    let restored_s1 = restored.get_session(sid1).expect("session 1 must restore");
    assert_eq!(restored_s1.window_order, vec![wid1b, wid1a]);
    assert_eq!(
        restored_s1.windows.get(&wid1b).unwrap().name,
        "b",
        "window identity must be preserved despite the reorder"
    );
    assert!(
        restored.get_session(sid2).is_some(),
        "session 2 must restore"
    );
}

// ── AC-2: descriptor flags cleared, verified by querying them ─────────

#[test]
fn snapshot_clears_cloexec_on_live_pane_master_and_listen_descriptor() {
    let pty_system = portable_pty::native_pty_system();
    let size = portable_pty::PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };
    let pair = pty_system.openpty(size).unwrap();
    let master_fd = pair.master.as_raw_fd().unwrap();
    assert!(
        has_cloexec(master_fd),
        "portable_pty opens the master with FD_CLOEXEC set (precondition)"
    );
    let writer = pair.master.take_writer().unwrap();
    let mut mgr = SessionManager::new();
    let sid = mgr.create_session("s".to_string());
    let wid = mgr.create_window(sid, "w".to_string()).unwrap();
    let pid = mgr.alloc_pane_id();
    let pane = MuxPane::new(pid, 80, 24, test_output_target(), writer, pair.master, None);
    mgr.get_session_mut(sid)
        .unwrap()
        .windows
        .get_mut(&wid)
        .unwrap()
        .add_pane(pane);

    let dir = tempfile::tempdir().expect("tempdir");
    let socket_path = dir.path().join("mux-default.sock");
    let listen_file = tempfile::NamedTempFile::new_in(dir.path()).expect("listen fd stand-in");
    let listen_fd = listen_file.as_raw_fd();
    assert!(
        has_cloexec(listen_fd),
        "freshly opened files default to FD_CLOEXEC (precondition)"
    );

    snapshot(&mgr, listen_fd, &socket_path).expect("snapshot must succeed");

    let live_master_fd = mgr
        .get_session(sid)
        .unwrap()
        .windows
        .get(&wid)
        .unwrap()
        .panes
        .get(&pid)
        .unwrap()
        .master_raw_fd()
        .unwrap();
    assert_eq!(live_master_fd, master_fd);
    assert!(
        !has_cloexec(live_master_fd),
        "snapshot must clear FD_CLOEXEC on the live pane's master descriptor"
    );
    assert!(
        !has_cloexec(listen_fd),
        "snapshot must clear FD_CLOEXEC on the listen descriptor"
    );
}

// ── AC-4 / AC-5: scrollback byte-for-byte + restored pane stays
// writable through its (re-adopted) master ────────────────────────────

#[test]
fn snapshot_then_restore_round_trips_scrollback_and_the_restored_pane_is_writable() {
    let pty_system = portable_pty::native_pty_system();
    let size = portable_pty::PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };
    let pair = pty_system.openpty(size).unwrap();
    let writer = pair.master.take_writer().unwrap();
    let mut mgr = SessionManager::new();
    let sid = mgr.create_session("s".to_string());
    let wid = mgr.create_window(sid, "w".to_string()).unwrap();
    let pid = mgr.alloc_pane_id();
    let pane = MuxPane::new(pid, 80, 24, test_output_target(), writer, pair.master, None);
    pane.scrollback
        .lock()
        .unwrap()
        .write(b"hello from before the upgrade");
    mgr.get_session_mut(sid)
        .unwrap()
        .windows
        .get_mut(&wid)
        .unwrap()
        .add_pane(pane);

    let dir = tempfile::tempdir().expect("tempdir");
    let socket_path = dir.path().join("mux-default.sock");
    let listen_file = tempfile::NamedTempFile::new_in(dir.path()).expect("listen fd stand-in");
    let listen_fd = listen_file.as_raw_fd();

    let mut document = snapshot(&mgr, listen_fd, &socket_path).expect("snapshot must succeed");
    let pane_doc = &document.sessions[0].windows[0].panes[0];
    assert_eq!(pane_doc.scrollback, b"hello from before the upgrade");
    let original_fd = pane_doc
        .master_fd
        .expect("live pane must record a master fd");

    // Simulate the fd surviving a process replacement: duplicate it so
    // the restore path (which will close its adopted copy on drop)
    // never races the ORIGINAL pane's own master (still alive in `mgr`,
    // which a real hot-upgrade would have replaced entirely) — same
    // technique `inherited_pty`'s own tests use to avoid a double-close.
    let dup_fd = unsafe { libc::dup(original_fd) };
    assert!(dup_fd >= 0, "dup(2) failed");
    document.sessions[0].windows[0].panes[0].master_fd = Some(dup_fd);

    let (title_tx, notification_tx, agent_status_tx, pane_exit_sender) = test_restore_channels();
    let restored = restore(
        &document,
        &title_tx,
        &notification_tx,
        &agent_status_tx,
        &pane_exit_sender,
    );

    let restored_session = restored.get_session(sid).unwrap();
    let restored_window = restored_session.windows.get(&wid).unwrap();
    let restored_pane = restored_window.panes.get(&pid).unwrap();
    assert!(
        !restored_pane.exited,
        "a live pane's descriptor must adopt successfully in this test"
    );
    assert_eq!(
        restored_pane.scrollback.lock().unwrap().read_all(),
        b"hello from before the upgrade"
    );
    // AC-5: the restored pane is still writable through its adopted
    // master (the dedicated pane-level test
    // `from_restored_pane_can_write_and_read_through_its_adopted_master`
    // additionally proves the read side deterministically, without a
    // background reader thread in the way).
    restored_pane
        .write_input(b"after-restore\n")
        .expect("restored pane must remain writable through its adopted master");
}

/// AC-3/AC-5: a full `snapshot` -> `restore` round trip carries an
/// alt-screen pane's state end to end -- the restored pane's shadow
/// parser reports the alternate screen active with the dump's content
/// on it AND the replayed scrollback beneath it, mirroring the
/// dedicated `pane.rs`-level `from_restored` test but through the
/// document codec this module owns (SPEC AC-3: the next reattach's
/// snapshot would take the alt branch, gated on exactly the
/// `alternate_screen()` flag asserted below).
#[test]
fn snapshot_then_restore_round_trips_alt_screen_state() {
    let pty_system = portable_pty::native_pty_system();
    let size = portable_pty::PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };
    let pair = pty_system.openpty(size).unwrap();
    let writer = pair.master.take_writer().unwrap();
    let mut mgr = SessionManager::new();
    let sid = mgr.create_session("s".to_string());
    let wid = mgr.create_window(sid, "w".to_string()).unwrap();
    let pid = mgr.alloc_pane_id();
    let pane = MuxPane::new(pid, 80, 24, test_output_target(), writer, pair.master, None);
    pane.scrollback
        .lock()
        .unwrap()
        .write(b"pre-upgrade main-buffer history");
    pane.shadow_parser.lock().unwrap().process(b"\x1b[?1049h");
    pane.shadow_parser
        .lock()
        .unwrap()
        .process(b"ROUND-TRIP-ALT-CONTENT");
    mgr.get_session_mut(sid)
        .unwrap()
        .windows
        .get_mut(&wid)
        .unwrap()
        .add_pane(pane);

    let dir = tempfile::tempdir().expect("tempdir");
    let socket_path = dir.path().join("mux-default.sock");
    let listen_file = tempfile::NamedTempFile::new_in(dir.path()).expect("listen fd stand-in");
    let listen_fd = listen_file.as_raw_fd();

    let mut document = snapshot(&mgr, listen_fd, &socket_path).expect("snapshot must succeed");
    {
        let pane_doc = &document.sessions[0].windows[0].panes[0];
        assert!(
            pane_doc.alt_screen,
            "sanity: snapshot must record the alt-screen state"
        );
        assert!(!pane_doc.alt_screen_dump.is_empty());
    }
    let original_fd = document.sessions[0].windows[0].panes[0]
        .master_fd
        .expect("live pane must record a master fd");
    let dup_fd = unsafe { libc::dup(original_fd) };
    assert!(dup_fd >= 0, "dup(2) failed");
    document.sessions[0].windows[0].panes[0].master_fd = Some(dup_fd);

    let (title_tx, notification_tx, agent_status_tx, pane_exit_sender) = test_restore_channels();
    let restored = restore(
        &document,
        &title_tx,
        &notification_tx,
        &agent_status_tx,
        &pane_exit_sender,
    );

    let restored_pane = restored
        .get_session(sid)
        .unwrap()
        .windows
        .get(&wid)
        .unwrap()
        .panes
        .get(&pid)
        .unwrap();
    assert!(!restored_pane.exited);
    {
        let parser = restored_pane.shadow_parser.lock().unwrap();
        assert!(
            parser.screen().alternate_screen(),
            "AC-5: the restored pane's shadow parser must report the alternate \
             screen active -- exactly the flag a reattach snapshot builder \
             (build_snapshot_bytes) branches on to take the alt path (SPEC AC-3)"
        );
        let content = parser.screen().contents_formatted();
        assert!(
            content
                .windows(b"ROUND-TRIP-ALT-CONTENT".len())
                .any(|w| w == b"ROUND-TRIP-ALT-CONTENT"),
            "AC-5: the restored alt screen must show the captured dump's content"
        );
    }
    restored_pane
        .shadow_parser
        .lock()
        .unwrap()
        .process(b"\x1b[?1049l");
    let main_content = restored_pane
        .shadow_parser
        .lock()
        .unwrap()
        .screen()
        .contents_formatted();
    assert!(
        main_content
            .windows(b"pre-upgrade main-buffer history".len())
            .any(|w| w == b"pre-upgrade main-buffer history"),
        "AC-5: the replayed scrollback must survive beneath the restored alt screen"
    );
}

// ── AC-3: snapshot_pane captures alt-screen state ──────────────────────

/// AC-3: `snapshot` records an alt-screen pane's document entry with
/// flag true and a dump equal to the parser's formatted alt-screen
/// contents.
#[test]
fn snapshot_records_alt_screen_state_for_an_alt_screen_pane() {
    let mut mgr = SessionManager::new();
    let sid = mgr.create_session("s".to_string());
    let wid = mgr.create_window(sid, "w".to_string()).unwrap();
    let pid = mgr.alloc_pane_id();
    let pane = MuxPane::new_test(pid, 80, 24, test_output_target());
    pane.shadow_parser.lock().unwrap().process(b"\x1b[?1049h");
    pane.shadow_parser
        .lock()
        .unwrap()
        .process(b"SNAPSHOT-ALT-CONTENT");
    mgr.get_session_mut(sid)
        .unwrap()
        .windows
        .get_mut(&wid)
        .unwrap()
        .add_pane(pane);

    let dir = tempfile::tempdir().expect("tempdir");
    let socket_path = dir.path().join("mux-default.sock");
    let listen_file = tempfile::NamedTempFile::new_in(dir.path()).expect("listen fd stand-in");
    let listen_fd = listen_file.as_raw_fd();

    let document = snapshot(&mgr, listen_fd, &socket_path).expect("snapshot must succeed");
    let pane_doc = &document.sessions[0].windows[0].panes[0];
    assert!(
        pane_doc.alt_screen,
        "AC-3: an alt-screen pane must record flag true"
    );
    assert!(
        pane_doc
            .alt_screen_dump
            .windows(b"SNAPSHOT-ALT-CONTENT".len())
            .any(|w| w == b"SNAPSHOT-ALT-CONTENT"),
        "AC-3: the dump must equal the parser's formatted alt-screen contents"
    );
}

/// AC-3: `snapshot` records an exited pane's document entry with flag
/// false and an empty dump, even when the pane's shadow parser was left
/// on the alternate screen before it exited -- an exited pane has no
/// live alternate-screen semantics to carry.
#[test]
fn snapshot_records_no_alt_screen_state_for_an_exited_pane() {
    let mut mgr = SessionManager::new();
    let sid = mgr.create_session("s".to_string());
    let wid = mgr.create_window(sid, "w".to_string()).unwrap();
    let pid = mgr.alloc_pane_id();
    let mut pane = MuxPane::new_test(pid, 80, 24, test_output_target());
    pane.shadow_parser.lock().unwrap().process(b"\x1b[?1049h");
    pane.shadow_parser
        .lock()
        .unwrap()
        .process(b"STALE-ALT-CONTENT-BEFORE-EXIT");
    pane.mark_exited();
    mgr.get_session_mut(sid)
        .unwrap()
        .windows
        .get_mut(&wid)
        .unwrap()
        .add_pane(pane);

    let dir = tempfile::tempdir().expect("tempdir");
    let socket_path = dir.path().join("mux-default.sock");
    let listen_file = tempfile::NamedTempFile::new_in(dir.path()).expect("listen fd stand-in");
    let listen_fd = listen_file.as_raw_fd();

    let document = snapshot(&mgr, listen_fd, &socket_path).expect("snapshot must succeed");
    let pane_doc = &document.sessions[0].windows[0].panes[0];
    assert!(pane_doc.exited);
    assert!(
        !pane_doc.alt_screen,
        "AC-3: an exited pane must record flag false regardless of its parser's last state"
    );
    assert!(
        pane_doc.alt_screen_dump.is_empty(),
        "AC-3: an exited pane must record an empty dump"
    );
}

// ── AC-6 / AC-7: exited-pane restore + unadoptable-descriptor restore,
// rest of the tree still restores ──────────────────────────────────────

#[test]
fn restore_handles_exited_and_unadoptable_panes_while_the_rest_of_the_tree_still_restores() {
    let bogus_fd: i32 = 999_999; // never a live fd in the test process
    let doc = HandoffDocument {
        schema_version: HANDOFF_SCHEMA_VERSION,
        incarnation: "abc123".to_string(),
        listen_fd: 3,
        next_session_id: 2,
        next_pane_id: 3,
        sessions: vec![HandoffSession {
            id: 1,
            name: "s".to_string(),
            window_order: vec![1],
            active_window_id: Some(1),
            next_window_id: 2,
            windows: vec![HandoffWindow {
                id: 1,
                name: "w".to_string(),
                active_pane_id: Some(1),
                next_pane_id: 3,
                panes: vec![
                    // AC-7: recorded live, but the descriptor cannot be
                    // adopted.
                    HandoffPane {
                        id: 1,
                        cols: 80,
                        rows: 24,
                        cwd: None,
                        title: None,
                        agent_state: None,
                        agent_name: None,
                        agent_revision: 0,
                        exited: false,
                        child_pid: Some(1234),
                        master_fd: Some(bogus_fd),
                        scrollback: b"leftover".to_vec(),
                        // task0004: a mid-flight latch, to prove it
                        // still carries over even when the descriptor
                        // fails to adopt and the pane restores as
                        // exited (AC-7's outcome).
                        latch_armed: true,
                        latch_command_ended: true,
                        latch_generation: 5,
                        // mux-hot-upgrade-alt-screen task0002: alt state
                        // is irrelevant on this path — an unadoptable
                        // descriptor restores via `build_exited_pane`,
                        // which never reads these fields.
                        alt_screen: false,
                        alt_screen_dump: Vec::new(),
                    },
                    // AC-6: recorded exited — adopts no descriptor.
                    HandoffPane {
                        id: 2,
                        cols: 80,
                        rows: 24,
                        cwd: Some("/tmp".to_string()),
                        title: None,
                        agent_state: None,
                        agent_name: None,
                        agent_revision: 0,
                        exited: true,
                        child_pid: None,
                        master_fd: None,
                        scrollback: Vec::new(),
                        latch_armed: false,
                        latch_command_ended: false,
                        latch_generation: 0,
                        alt_screen: false,
                        alt_screen_dump: Vec::new(),
                    },
                ],
            }],
        }],
    };

    let (title_tx, notification_tx, agent_status_tx, pane_exit_sender) = test_restore_channels();
    let mgr = restore(
        &doc,
        &title_tx,
        &notification_tx,
        &agent_status_tx,
        &pane_exit_sender,
    );

    // The rest of the tree still restores despite pane 1's unadoptable
    // descriptor.
    let session = mgr.get_session(1).expect("session must still restore");
    let window = session.windows.get(&1).expect("window must still restore");

    let pane1 = window.panes.get(&1).unwrap();
    assert!(
        pane1.exited,
        "AC-7: an unadoptable descriptor must restore the pane as exited"
    );
    assert_eq!(
        pane1.scrollback.lock().unwrap().read_all(),
        b"leftover",
        "non-descriptor attributes still restore even when adoption fails"
    );
    // AC-7 "the reason is logged": verified by inspection of the
    // `log::warn!` call in `restore_pane`'s adopt-failure arm (matching
    // this project's established convention for asserting on log output
    // — see `child_reaper.rs`'s equivalent tests).
    // task0004: the inferred-clear latch still carries over verbatim
    // even on the "restore as exited" path taken when a live pane's
    // descriptor fails to adopt.
    assert_eq!(
        *pane1.agent_status_exit_latch.lock().unwrap(),
        AgentStatusExitLatch::from_state_parts(true, true, 5),
        "task0004: a mid-flight latch must survive restore even when the pane's \
         descriptor could not be adopted"
    );

    let pane2 = window.panes.get(&2).unwrap();
    assert!(
        pane2.exited,
        "AC-6: a pane recorded exited restores as exited"
    );
    assert_eq!(*pane2.cwd.lock().unwrap(), Some("/tmp".to_string()));
    assert_eq!(
        *pane2.agent_status_exit_latch.lock().unwrap(),
        AgentStatusExitLatch::new(),
        "task0004: a disarmed latch restores disarmed"
    );
}

// ── AC-4: restore sets close-on-exec back on every adopted descriptor,
// and a pane spawned AFTERWARDS does not inherit it ────────────────────

#[test]
fn restore_sets_cloexec_back_on_the_adopted_pane_master() {
    let pty_system = portable_pty::native_pty_system();
    let size = portable_pty::PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };
    let pair = pty_system.openpty(size).unwrap();
    let master_fd = pair.master.as_raw_fd().unwrap();
    // Simulate what snapshot does pre-exec: clear CLOEXEC on a
    // (duplicated, to avoid a double-close race with `pair.master`'s own
    // Drop) descriptor recording this pane as live.
    let dup_fd = unsafe { libc::dup(master_fd) };
    assert!(dup_fd >= 0, "dup(2) failed");
    clear_cloexec(dup_fd).expect("simulate snapshot's clear_cloexec");
    assert!(
        !has_cloexec(dup_fd),
        "sanity: CLOEXEC must be cleared before adoption, mirroring a real handoff"
    );

    let master = adopt_master(dup_fd).expect("a live descriptor must adopt");

    assert!(
        has_cloexec(master.as_raw_fd().unwrap()),
        "AC-4: restore must set CLOEXEC back on the adopted descriptor"
    );
}

/// AC-4 (continued): a pane child spawned AFTER a restored pane's master
/// is adopted must not inherit that master's descriptor — proven with a
/// REAL spawned child inspecting its own fd table, per the Test Notes
/// ("AC-4 needs a real spawned child to inspect what it inherited").
#[test]
fn a_pane_child_spawned_after_restore_does_not_inherit_the_adopted_master() {
    let pty_system = portable_pty::native_pty_system();
    let size = portable_pty::PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };
    let pair = pty_system.openpty(size).unwrap();
    let master_fd = pair.master.as_raw_fd().unwrap();
    let dup_fd = unsafe { libc::dup(master_fd) };
    assert!(dup_fd >= 0, "dup(2) failed");
    clear_cloexec(dup_fd).expect("simulate snapshot's clear_cloexec");

    let master = adopt_master(dup_fd).expect("a live descriptor must adopt");
    let adopted_fd = master.as_raw_fd().unwrap();

    // A fresh child process, spawned in THIS process after adoption —
    // standing in for a brand-new pane's shell spawned after the
    // hot-upgrade. `std::process::Command`'s fork+exec inherits every
    // open descriptor that lacks FD_CLOEXEC; if the adopted master were
    // still missing that flag, `/proc/self/fd/<adopted_fd>` would exist
    // in the CHILD too.
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!(
            "if [ -e /proc/self/fd/{adopted_fd} ]; then echo INHERITED; else echo NOT_INHERITED; fi"
        ))
        .output()
        .expect("spawn a real child process");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("NOT_INHERITED"),
        "AC-4: a pane child spawned after restore must not inherit the adopted master \
         (child fd table output: {stdout:?})"
    );

    drop(master);
}

// ── AC-11 / medium performance finding: a descriptor that fails
// adoption is closed, not leaked ────────────────────────────────────────

#[test]
fn adopt_master_closes_the_descriptor_when_construction_fails() {
    // An ordinary (non-terminal) file: `InheritedMasterPty::new`'s own
    // `isatty` check fails, so `adopt_master` must close it itself
    // (Contract: "on any Err, fd is guaranteed already closed").
    let file = tempfile::tempfile().expect("must create a temp file");
    let fd = file.as_raw_fd();
    // Duplicate so we can assert on the ORIGINAL descriptor number
    // independently of `file`'s own (later) Drop.
    let dup_fd = unsafe { libc::dup(fd) };
    assert!(dup_fd >= 0, "dup(2) failed");
    // Identity of the file `dup_fd` currently refers to, captured before
    // adoption closes it. Compared by (dev, ino) rather than fd validity
    // alone (matching `inherited_pty.rs`'s established convention): if
    // `cargo test`'s parallel threads — sharing one process-wide fd
    // table — hand this freed NUMBER to an unrelated descriptor from a
    // concurrently running test before the assertion below runs, that
    // descriptor can only ever refer to a DIFFERENT file, which still
    // proves `dup_fd` was closed.
    let original_identity = stat_dev_ino(dup_fd).expect("sanity: fd must be open before adoption");

    let result = adopt_master(dup_fd);
    assert!(result.is_err(), "a non-terminal fd must fail adoption");

    match stat_dev_ino(dup_fd) {
        None => {}
        Some(identity) => assert_ne!(
            identity, original_identity,
            "AC-11: a descriptor that fails adoption must be closed, not leaked"
        ),
    }
}

// ── AC-6: listener adoption validates before taking ownership ─────────

#[test]
fn adopt_listener_succeeds_over_a_real_listening_unix_socket_and_sets_cloexec() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock_path = dir.path().join("adopt-listener.sock");
    let std_listener =
        std::os::unix::net::UnixListener::bind(&sock_path).expect("bind real listener");
    let fd = std_listener.as_raw_fd();
    // Duplicate so `adopt_listener` can take ownership of ITS OWN
    // descriptor independently of `std_listener`'s own Drop (which would
    // otherwise double-close the same fd number once this test ends).
    let dup_fd = unsafe { libc::dup(fd) };
    assert!(dup_fd >= 0, "dup(2) failed");
    clear_cloexec(dup_fd).expect("simulate snapshot's clear_cloexec on the listen descriptor");

    let adopted = adopt_listener(dup_fd).expect("a real listening AF_UNIX socket must adopt");

    assert_eq!(adopted.as_raw_fd(), dup_fd);
    assert!(
        has_cloexec(dup_fd),
        "AC-4: adopt_listener must set CLOEXEC back on the adopted listener"
    );
}

#[test]
fn adopt_listener_rejects_a_non_socket_descriptor_without_taking_ownership() {
    let file = tempfile::tempfile().expect("must create a temp file");
    let fd = file.as_raw_fd();

    let result = adopt_listener(fd);

    assert!(
        result.is_err(),
        "a regular file must not adopt as a listener"
    );
    // AC-6: the descriptor is still open and unchanged afterward — no
    // ownership was taken, so `file`'s own (still-live) handle proves
    // this by continuing to work normally.
    assert!(
        std::fs::metadata(format!("/proc/self/fd/{fd}")).is_ok(),
        "AC-6: a failed adoption must leave the descriptor open and unchanged"
    );
    drop(file);
}

#[test]
fn adopt_listener_rejects_a_connected_non_listening_socket_without_taking_ownership() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock_path = dir.path().join("not-listening.sock");
    let listener = std::os::unix::net::UnixListener::bind(&sock_path).expect("bind real listener");
    let client =
        std::os::unix::net::UnixStream::connect(&sock_path).expect("connect a client socket");
    let fd = client.as_raw_fd();

    let result = adopt_listener(fd);

    assert!(
        result.is_err(),
        "AC-6: a connected (non-listening) socket must not adopt as a listener"
    );
    assert!(
        std::fs::metadata(format!("/proc/self/fd/{fd}")).is_ok(),
        "AC-6: a failed adoption must leave the descriptor open and unchanged"
    );
    drop(client);
    drop(listener);
}

// ── AC-8: handoff file 0600, removed in all three outcomes ────────────

#[test]
fn handoff_file_is_created_0600_and_removed_after_a_successful_read() {
    let mgr = SessionManager::new();
    let dir = tempfile::tempdir().expect("tempdir");
    let socket_path = dir.path().join("mux-default.sock");
    let listen_file = tempfile::NamedTempFile::new_in(dir.path()).expect("listen fd stand-in");
    let listen_fd = listen_file.as_raw_fd();

    let document = snapshot(&mgr, listen_fd, &socket_path).expect("snapshot must succeed");
    let path = handoff_file_path(&socket_path);
    assert!(path.exists(), "handoff file must be created");
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "handoff file must be owner-only (0600)");

    let read_back = read_and_remove_handoff_file(&path).expect("read must succeed");
    assert_eq!(read_back, document);
    assert!(
        !path.exists(),
        "handoff file must be removed after a successful read"
    );
}

#[test]
fn handoff_file_is_removed_after_a_failed_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("mux-handoff.bin");
    std::fs::write(&path, b"not a valid handoff document").unwrap();
    assert!(path.exists());

    let result = read_and_remove_handoff_file(&path);
    assert!(result.is_err(), "malformed bytes must fail to decode");
    assert!(
        !path.exists(),
        "handoff file must be removed even after a failed read"
    );
}

/// Forces `write_all` to fail deterministically via `/dev/full` (a
/// standard Linux device that always reports `ENOSPC` on write),
/// instead of manipulating a live `std::fs::File`'s raw descriptor
/// directly — closing an fd a `File` still believes it owns trips
/// std's own IO-safety double-close abort, so that technique is
/// unusable here. `path` still refers to a REAL regular file (created
/// via `create_handoff_file`, matching production's real target), so
/// the removal this test asserts on is genuine.
#[test]
fn write_bytes_or_remove_cleans_up_the_file_when_the_write_itself_fails() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("mux-handoff.bin");
    // Create the real target file (as `write_handoff_file` would), then
    // release the handle — only the on-disk file matters for the
    // removal assertion below.
    drop(create_handoff_file(&path).expect("create must succeed"));
    assert!(path.exists());

    let full_device = OpenOptions::new()
        .write(true)
        .open("/dev/full")
        .expect("/dev/full must be openable for writing on Linux");

    let result = write_bytes_or_remove(full_device, &path, b"some bytes");
    assert!(
        result.is_err(),
        "every write to /dev/full must fail with ENOSPC"
    );
    assert!(
        !path.exists(),
        "a partially written handoff file must be removed on write failure \
         (aborted-snapshot outcome)"
    );
}

#[test]
fn write_handoff_file_refuses_to_overwrite_an_existing_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("mux-handoff.bin");
    std::fs::write(&path, b"leftover").unwrap();

    let result = write_handoff_file(&minimal_document(), &path);
    assert!(
        result.is_err(),
        "must fail rather than follow/reuse an existing path"
    );
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"leftover",
        "the pre-existing file must be left untouched"
    );
}

// ── task0004 AC-1/AC-2/AC-3: the daemon-side inferred-clear latch
// (task0001 AgentStatusExitLatch, wired per-pane by task0003) survives a
// snapshot/restore round trip in each of its three reachable states,
// preserved verbatim rather than reset or re-derived ────────────────────

/// Build a live pane on a real PTY, registered in a fresh single-session/
/// single-window `SessionManager`, mirroring this file's own established
/// live-pane round-trip fixture (see
/// `snapshot_then_restore_round_trips_scrollback_and_the_restored_pane_is_writable`).
/// Returns the manager plus the ids needed to look the pane back up.
fn single_live_pane_manager() -> (SessionManager, u32, u32, u32) {
    let pty_system = portable_pty::native_pty_system();
    let size = portable_pty::PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };
    let pair = pty_system.openpty(size).unwrap();
    let writer = pair.master.take_writer().unwrap();
    let mut mgr = SessionManager::new();
    let sid = mgr.create_session("s".to_string());
    let wid = mgr.create_window(sid, "w".to_string()).unwrap();
    let pid = mgr.alloc_pane_id();
    let pane = MuxPane::new(pid, 80, 24, test_output_target(), writer, pair.master, None);
    mgr.get_session_mut(sid)
        .unwrap()
        .windows
        .get_mut(&wid)
        .unwrap()
        .add_pane(pane);
    (mgr, sid, wid, pid)
}

/// Snapshot `mgr` and restore it back, simulating the fd surviving a
/// process replacement by duplicating the recorded master fd before
/// restoring (same technique
/// `snapshot_then_restore_round_trips_scrollback_and_the_restored_pane_is_writable`
/// uses, so the ORIGINAL pane still alive in `mgr` is never double-closed).
fn snapshot_then_restore_live_pane(mgr: &SessionManager) -> SessionManager {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket_path = dir.path().join("mux-default.sock");
    let listen_file = tempfile::NamedTempFile::new_in(dir.path()).expect("listen fd stand-in");
    let listen_fd = listen_file.as_raw_fd();

    let mut document = snapshot(mgr, listen_fd, &socket_path).expect("snapshot must succeed");
    let pane_doc = &mut document.sessions[0].windows[0].panes[0];
    let original_fd = pane_doc
        .master_fd
        .expect("live pane must record a master fd");
    let dup_fd = unsafe { libc::dup(original_fd) };
    assert!(dup_fd >= 0, "dup(2) failed");
    pane_doc.master_fd = Some(dup_fd);

    let (title_tx, notification_tx, agent_status_tx, pane_exit_sender) = test_restore_channels();
    restore(
        &document,
        &title_tx,
        &notification_tx,
        &agent_status_tx,
        &pane_exit_sender,
    )
}

/// AC-1: a pane whose latch is armed (`Set` observed, no `D`/`A` yet) at
/// the moment of a hot-upgrade remains armed with the SAME generation
/// immediately after the upgrade completes.
#[test]
fn armed_latch_with_no_d_or_a_yet_survives_restore_with_the_same_generation() {
    let (mut mgr, sid, wid, pid) = single_live_pane_manager();
    {
        let pane = mgr
            .get_session_mut(sid)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .panes
            .get_mut(&pid)
            .unwrap();
        pane.apply_agent_status_event(AgentStatusEvent::Set {
            state: AgentState::Working,
            name: Some("claude".to_string()),
        });
    }
    let expected_latch = *mgr
        .get_session(sid)
        .unwrap()
        .windows
        .get(&wid)
        .unwrap()
        .panes
        .get(&pid)
        .unwrap()
        .agent_status_exit_latch
        .lock()
        .unwrap();
    assert_eq!(
        expected_latch,
        AgentStatusExitLatch::from_state_parts(true, false, 1),
        "sanity: a bare Set arms the latch with generation 1"
    );

    let restored = snapshot_then_restore_live_pane(&mgr);

    let restored_pane = restored
        .get_session(sid)
        .unwrap()
        .windows
        .get(&wid)
        .unwrap()
        .panes
        .get(&pid)
        .unwrap();
    assert!(
        !restored_pane.exited,
        "AC-1 setup: the pane's descriptor must adopt successfully in this test"
    );
    assert_eq!(
        *restored_pane.agent_status_exit_latch.lock().unwrap(),
        expected_latch,
        "AC-1: an armed latch (Set observed, no D/A yet) must survive a hot-upgrade \
         with the same generation"
    );
}

/// AC-2: a pane whose latch has recorded `command_ended` (`Set` -> live
/// `D`, no `A` yet) at the moment of a hot-upgrade still fires exactly
/// one inferred clear when its matching `A` arrives after the upgrade.
#[test]
fn command_ended_latch_fires_exactly_once_on_the_first_live_a_after_restore() {
    let (mut mgr, sid, wid, pid) = single_live_pane_manager();
    {
        let pane = mgr
            .get_session_mut(sid)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .panes
            .get_mut(&pid)
            .unwrap();
        pane.apply_agent_status_event(AgentStatusEvent::Set {
            state: AgentState::Working,
            name: Some("claude".to_string()),
        });
        let fired = pane.record_live_osc133_mark(PromptMarkKind::CommandEnd);
        assert_eq!(fired, None, "a lone D must not fire a clear");
    }

    let restored = snapshot_then_restore_live_pane(&mgr);

    let restored_pane = restored
        .get_session(sid)
        .unwrap()
        .windows
        .get(&wid)
        .unwrap()
        .panes
        .get(&pid)
        .unwrap();
    assert!(
        !restored_pane.exited,
        "AC-2 setup: the pane's descriptor must adopt successfully in this test"
    );
    assert_eq!(
        *restored_pane.agent_status_exit_latch.lock().unwrap(),
        AgentStatusExitLatch::from_state_parts(true, true, 1),
        "AC-2 setup: the restored latch must still record command_ended"
    );

    let fired = restored_pane.record_live_osc133_mark(PromptMarkKind::PromptStart);
    assert!(
        fired.is_some(),
        "AC-2: the pending D->A transition must fire an inferred clear after restore"
    );
    assert_eq!(restored_pane.agent_status.lock().unwrap().state, None);

    let fired_again = restored_pane.record_live_osc133_mark(PromptMarkKind::PromptStart);
    assert_eq!(
        fired_again, None,
        "AC-2: a second live A must not re-fire an already-fired latch"
    );
}

/// AC-3: a pane whose latch is disarmed (no `Set` since the last
/// `Clear`) at the moment of a hot-upgrade remains disarmed after the
/// upgrade — no spurious clear and no false arm introduced by the
/// transfer itself.
#[test]
fn disarmed_latch_stays_disarmed_after_restore_with_no_spurious_clear() {
    let (mut mgr, sid, wid, pid) = single_live_pane_manager();
    {
        let pane = mgr
            .get_session_mut(sid)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .panes
            .get_mut(&pid)
            .unwrap();
        pane.apply_agent_status_event(AgentStatusEvent::Set {
            state: AgentState::Working,
            name: None,
        });
        pane.apply_agent_status_event(AgentStatusEvent::Clear);
    }
    let expected_latch = *mgr
        .get_session(sid)
        .unwrap()
        .windows
        .get(&wid)
        .unwrap()
        .panes
        .get(&pid)
        .unwrap()
        .agent_status_exit_latch
        .lock()
        .unwrap();
    let (armed, command_ended, _generation) = expected_latch.state_parts();
    assert!(
        !armed && !command_ended,
        "sanity: Set then Clear leaves the latch disarmed"
    );

    let restored = snapshot_then_restore_live_pane(&mgr);

    let restored_pane = restored
        .get_session(sid)
        .unwrap()
        .windows
        .get(&wid)
        .unwrap()
        .panes
        .get(&pid)
        .unwrap();
    assert!(
        !restored_pane.exited,
        "AC-3 setup: the pane's descriptor must adopt successfully in this test"
    );
    assert_eq!(
        *restored_pane.agent_status_exit_latch.lock().unwrap(),
        expected_latch,
        "AC-3: a disarmed latch must be transferred verbatim, not reset or re-derived"
    );

    let revision_before = restored_pane.agent_status.lock().unwrap().revision;
    let d_result = restored_pane.record_live_osc133_mark(PromptMarkKind::CommandEnd);
    let a_result = restored_pane.record_live_osc133_mark(PromptMarkKind::PromptStart);
    assert_eq!(
        d_result, None,
        "AC-3: no false arm — a live D must not fire while disarmed"
    );
    assert_eq!(
        a_result, None,
        "AC-3: no spurious clear — a live A must not fire while disarmed"
    );
    assert_eq!(
        restored_pane.agent_status.lock().unwrap().revision,
        revision_before,
        "AC-3: neither D nor A may change agent_status while the latch is disarmed"
    );
}

// ── task0006 (review rework, finding 2e6f18b4dc0a7593): refresh closes
// the torn-snapshot window for live agent-status/latch state ──────────

/// Reproduces the finding's exact example: a live D->A transition
/// completing AFTER `snapshot` already wrote the document (simulating
/// the daemon's agent-status task consuming a live mark during
/// `prepare_upgrade`'s post-snapshot client-acknowledgement wait) must
/// be reflected by `refresh_live_agent_state` -- otherwise the
/// successor would restore a latch waiting for an `A` that was already
/// consumed in THIS process and will never arrive again.
#[test]
fn refresh_live_agent_state_pulls_in_a_clear_that_completed_after_snapshot() {
    let (mut mgr, sid, wid, pid) = single_live_pane_manager();
    {
        let pane = mgr
            .get_session_mut(sid)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .panes
            .get_mut(&pid)
            .unwrap();
        pane.apply_agent_status_event(AgentStatusEvent::Set {
            state: AgentState::Working,
            name: Some("claude".to_string()),
        });
        let fired = pane.record_live_osc133_mark(PromptMarkKind::CommandEnd);
        assert_eq!(fired, None, "a lone D must not fire a clear yet");
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let socket_path = dir.path().join("mux-default.sock");
    let listen_file = tempfile::NamedTempFile::new_in(dir.path()).expect("listen fd stand-in");
    let listen_fd = listen_file.as_raw_fd();

    let mut document = snapshot(&mgr, listen_fd, &socket_path).expect("snapshot must succeed");
    {
        let pane_doc = &document.sessions[0].windows[0].panes[0];
        assert!(
            pane_doc.latch_armed && pane_doc.latch_command_ended,
            "the original snapshot must record the pre-clear armed/command_ended state"
        );
        assert_eq!(
            pane_doc.agent_state,
            Some(to_wire_state(AgentState::Working)),
            "the original snapshot must record the pre-clear agent_state"
        );
    }

    // Simulate the daemon's agent-status task consuming a live A DURING
    // prepare_upgrade's post-snapshot acknowledgement wait -- exactly
    // the window the finding describes -- WITHOUT re-snapshotting.
    let fired = {
        let pane = mgr
            .get_session(sid)
            .unwrap()
            .windows
            .get(&wid)
            .unwrap()
            .panes
            .get(&pid)
            .unwrap();
        pane.record_live_osc133_mark(PromptMarkKind::PromptStart)
    };
    assert!(
        fired.is_some(),
        "the pending D->A transition must fire an inferred clear in THIS process"
    );

    refresh_live_agent_state(&mut document, &mgr);

    let refreshed_pane_doc = &document.sessions[0].windows[0].panes[0];
    assert!(
        !refreshed_pane_doc.latch_armed && !refreshed_pane_doc.latch_command_ended,
        "task0006: refresh must pull in the disarm that happened after the original snapshot"
    );
    assert_eq!(
        refreshed_pane_doc.agent_state, None,
        "task0006: refresh must pull in the inferred clear's agent_state update"
    );
}

/// AC-4: a pane that enters the alternate screen AFTER the original
/// snapshot must have its document entry updated by
/// `refresh_live_agent_state` -- main->alt gains flag true + a fresh
/// dump.
#[test]
fn refresh_live_agent_state_pulls_in_alt_screen_entered_after_snapshot() {
    let (mgr, sid, wid, pid) = single_live_pane_manager();

    let dir = tempfile::tempdir().expect("tempdir");
    let socket_path = dir.path().join("mux-default.sock");
    let listen_file = tempfile::NamedTempFile::new_in(dir.path()).expect("listen fd stand-in");
    let listen_fd = listen_file.as_raw_fd();

    let mut document = snapshot(&mgr, listen_fd, &socket_path).expect("snapshot must succeed");
    {
        let pane_doc = &document.sessions[0].windows[0].panes[0];
        assert!(
            !pane_doc.alt_screen,
            "the original snapshot must record the pre-transition main-buffer state"
        );
        assert!(pane_doc.alt_screen_dump.is_empty());
    }

    // Simulate the pane entering the alternate screen DURING
    // prepare_upgrade's post-snapshot window -- WITHOUT re-snapshotting.
    {
        let pane = mgr
            .get_session(sid)
            .unwrap()
            .windows
            .get(&wid)
            .unwrap()
            .panes
            .get(&pid)
            .unwrap();
        pane.shadow_parser.lock().unwrap().process(b"\x1b[?1049h");
        pane.shadow_parser
            .lock()
            .unwrap()
            .process(b"POST-SNAPSHOT-ALT-CONTENT");
    }

    refresh_live_agent_state(&mut document, &mgr);

    let refreshed_pane_doc = &document.sessions[0].windows[0].panes[0];
    assert!(
        refreshed_pane_doc.alt_screen,
        "AC-4: refresh must pull in a main->alt transition that happened after snapshot"
    );
    assert!(
        refreshed_pane_doc
            .alt_screen_dump
            .windows(b"POST-SNAPSHOT-ALT-CONTENT".len())
            .any(|w| w == b"POST-SNAPSHOT-ALT-CONTENT"),
        "AC-4: the refreshed dump must reflect the post-snapshot alt-screen content"
    );
}

/// AC-4 (continued): a pane that LEAVES the alternate screen AFTER the
/// original snapshot must have its document entry updated in the other
/// direction -- alt->main returns to flag false + empty dump.
#[test]
fn refresh_live_agent_state_pulls_in_alt_screen_left_after_snapshot() {
    let (mgr, sid, wid, pid) = single_live_pane_manager();
    {
        let pane = mgr
            .get_session(sid)
            .unwrap()
            .windows
            .get(&wid)
            .unwrap()
            .panes
            .get(&pid)
            .unwrap();
        pane.shadow_parser.lock().unwrap().process(b"\x1b[?1049h");
        pane.shadow_parser
            .lock()
            .unwrap()
            .process(b"PRE-SNAPSHOT-ALT-CONTENT");
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let socket_path = dir.path().join("mux-default.sock");
    let listen_file = tempfile::NamedTempFile::new_in(dir.path()).expect("listen fd stand-in");
    let listen_fd = listen_file.as_raw_fd();

    let mut document = snapshot(&mgr, listen_fd, &socket_path).expect("snapshot must succeed");
    {
        let pane_doc = &document.sessions[0].windows[0].panes[0];
        assert!(
            pane_doc.alt_screen,
            "the original snapshot must record the pre-transition alt-screen state"
        );
        assert!(!pane_doc.alt_screen_dump.is_empty());
    }

    // Simulate the pane leaving the alternate screen DURING
    // prepare_upgrade's post-snapshot window -- WITHOUT re-snapshotting.
    {
        let pane = mgr
            .get_session(sid)
            .unwrap()
            .windows
            .get(&wid)
            .unwrap()
            .panes
            .get(&pid)
            .unwrap();
        pane.shadow_parser.lock().unwrap().process(b"\x1b[?1049l");
    }

    refresh_live_agent_state(&mut document, &mgr);

    let refreshed_pane_doc = &document.sessions[0].windows[0].panes[0];
    assert!(
        !refreshed_pane_doc.alt_screen,
        "AC-4: refresh must pull in an alt->main transition that happened after snapshot"
    );
    assert!(
        refreshed_pane_doc.alt_screen_dump.is_empty(),
        "AC-4: a pane back on the main buffer must have its dump cleared"
    );
}

/// A pane that exits (in `mgr`) after the original snapshot must be left
/// exactly as originally recorded -- refreshing exited-pane state is a
/// separate, pre-existing concern this function does not attempt to fix
/// (see its doc comment).
#[test]
fn refresh_live_agent_state_leaves_a_pane_that_since_exited_untouched() {
    let (mut mgr, sid, wid, pid) = single_live_pane_manager();
    {
        let pane = mgr
            .get_session_mut(sid)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .panes
            .get_mut(&pid)
            .unwrap();
        pane.apply_agent_status_event(AgentStatusEvent::Set {
            state: AgentState::Working,
            name: Some("claude".to_string()),
        });
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let socket_path = dir.path().join("mux-default.sock");
    let listen_file = tempfile::NamedTempFile::new_in(dir.path()).expect("listen fd stand-in");
    let listen_fd = listen_file.as_raw_fd();
    let mut document = snapshot(&mgr, listen_fd, &socket_path).expect("snapshot must succeed");
    let recorded = document.sessions[0].windows[0].panes[0].clone();

    // The pane exits in `mgr`, AND its agent_status is explicitly
    // cleared, AFTER the snapshot above -- proving refresh really SKIPS
    // an exited pane rather than happening to leave the same value.
    mgr.get_session_mut(sid)
        .unwrap()
        .windows
        .get_mut(&wid)
        .unwrap()
        .panes
        .get_mut(&pid)
        .unwrap()
        .mark_exited();
    mgr.get_session(sid)
        .unwrap()
        .windows
        .get(&wid)
        .unwrap()
        .panes
        .get(&pid)
        .unwrap()
        .apply_agent_status_event(AgentStatusEvent::Clear);

    refresh_live_agent_state(&mut document, &mgr);

    assert_eq!(
        document.sessions[0].windows[0].panes[0], recorded,
        "task0006: a pane that has since exited must be left exactly as originally recorded"
    );
}

/// A document pane whose id no longer resolves in `mgr` (e.g. destroyed
/// between snapshot and refresh) must be left untouched, not panic.
#[test]
fn refresh_live_agent_state_leaves_a_pane_no_longer_present_in_the_manager_untouched() {
    let (mgr, _sid, _wid, _pid) = single_live_pane_manager();
    let mut document = HandoffDocument {
        schema_version: HANDOFF_SCHEMA_VERSION,
        incarnation: "deadbeef".to_string(),
        listen_fd: 3,
        next_session_id: 1,
        next_pane_id: 1,
        sessions: vec![HandoffSession {
            id: 1,
            name: "s".to_string(),
            window_order: vec![1],
            active_window_id: Some(1),
            next_window_id: 2,
            windows: vec![HandoffWindow {
                id: 1,
                name: "w".to_string(),
                active_pane_id: Some(1),
                next_pane_id: 2,
                panes: vec![HandoffPane {
                    id: 999, // no such pane in `mgr`
                    cols: 80,
                    rows: 24,
                    cwd: None,
                    title: None,
                    agent_state: Some(to_wire_state(AgentState::Working)),
                    agent_name: Some("claude".to_string()),
                    agent_revision: 3,
                    exited: false,
                    child_pid: Some(1234),
                    master_fd: Some(42),
                    scrollback: Vec::new(),
                    latch_armed: true,
                    latch_command_ended: true,
                    latch_generation: 7,
                    alt_screen: true,
                    alt_screen_dump: b"stale-alt-dump".to_vec(),
                }],
            }],
        }],
    };
    let recorded = document.sessions[0].windows[0].panes[0].clone();

    refresh_live_agent_state(&mut document, &mgr);

    assert_eq!(
        document.sessions[0].windows[0].panes[0], recorded,
        "task0006: a pane no longer present in mgr must be left untouched, not panic"
    );
}

/// [`rewrite_handoff_file`] must replace the SAME handoff file
/// [`write_handoff_file`] already created -- unlike `write_handoff_file`'s
/// `create_new`, a second call must not fail just because the file
/// already exists, and the visible result at `path` must be the new
/// content (regardless of the temp-file-then-rename mechanics used to
/// get there).
#[test]
fn rewrite_handoff_file_replaces_an_already_written_handoff_file_at_the_same_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket_path = dir.path().join("mux-default.sock");
    let path = handoff_file_path(&socket_path);

    let mut document = minimal_document();
    write_handoff_file(&document, &path).expect("initial write must succeed");

    document.next_session_id = 42;
    rewrite_handoff_file(&document, &socket_path)
        .expect("rewrite must succeed over an already-existing handoff file");

    let bytes = std::fs::read(&path).expect("file must still exist at the same path");
    let decoded = decode_handoff_document(&bytes).expect("must decode as a valid document");
    assert_eq!(
        decoded.next_session_id, 42,
        "rewrite must persist the updated content, not the original write"
    );
}

/// The core regression this fix closes (finding `b58e0d47f3c2916a`): if
/// `rewrite_handoff_file` cannot produce the new content, the
/// PREVIOUSLY-WRITTEN handoff document at `path` must survive intact and
/// decodable -- never truncated/torn -- since the caller (`daemon.rs`)
/// only logs a warning on `Err` and continues toward `exec`, handing
/// whatever is on disk to the successor process.
///
/// Forces the failure by pre-occupying the same-directory temp path
/// (`<handoff file>.tmp`) with a directory, so the internal
/// `create_handoff_file(tmp_path)` call fails before any byte of the new
/// content is written anywhere near the real path -- exercising the
/// same "the real path must be untouched by a failed rewrite" property
/// a mid-write failure (e.g. ENOSPC) would.
#[test]
fn rewrite_handoff_file_leaves_the_previous_document_intact_when_the_write_fails() {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket_path = dir.path().join("mux-default.sock");
    let path = handoff_file_path(&socket_path);

    let document = minimal_document();
    write_handoff_file(&document, &path).expect("initial write must succeed");
    let original_bytes = std::fs::read(&path).expect("initial file must exist");

    let mut tmp_path = path.clone();
    let mut tmp_name = path.file_name().unwrap().to_os_string();
    tmp_name.push(".tmp");
    tmp_path.set_file_name(tmp_name);
    std::fs::create_dir(&tmp_path).expect("occupy the temp path with a directory");

    let mut broken_document = document.clone();
    broken_document.next_session_id = 42;
    let result = rewrite_handoff_file(&broken_document, &socket_path);
    assert!(
        result.is_err(),
        "rewrite must fail when it cannot create its temp file"
    );

    let bytes_after = std::fs::read(&path).expect("original handoff file must still exist");
    assert_eq!(
        bytes_after, original_bytes,
        "a failed rewrite must leave the previously written handoff document byte-for-byte \
         intact, not truncated or partially overwritten"
    );
    let decoded =
        decode_handoff_document(&bytes_after).expect("surviving document must still decode");
    assert_eq!(
        decoded.next_session_id, document.next_session_id,
        "surviving document must be the ORIGINAL content, not the failed rewrite's"
    );
}
