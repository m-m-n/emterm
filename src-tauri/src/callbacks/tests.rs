use super::*;
use crate::render::theme::{CursorStyle, Rgb};

// ── Test infrastructure ─────────────────────────────────────────────

/// Capturing `NotificationSink` for unit tests.
#[derive(Default)]
struct TestSink {
    calls: Mutex<Vec<(String, String)>>,
}

impl TestSink {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
    fn calls(&self) -> Vec<(String, String)> {
        self.calls.lock().clone()
    }
}

impl NotificationSink for TestSink {
    fn send(&self, title: &str, body: &str) {
        self.calls
            .lock()
            .push((title.to_string(), body.to_string()));
    }
}

/// Bag of test handles so tests can poke at every shared piece without
/// re-wiring constructors.
struct Harness {
    cb: NativeCallbacks,
    state: Arc<Mutex<NativeCallbackState>>,
    theme: Arc<Mutex<Theme>>,
    #[allow(dead_code)]
    sink: Arc<TestSink>,
    #[allow(dead_code)]
    clock: Arc<Mutex<Instant>>,
}

fn harness(settings: Settings) -> Harness {
    let state = Arc::new(Mutex::new(NativeCallbackState::default()));
    let theme = Arc::new(Mutex::new(Theme::default()));
    let sink: Arc<TestSink> = TestSink::new();
    let clock = Arc::new(Mutex::new(Instant::now()));
    let clk = clock.clone();
    let rl = Arc::new(NotificationRateLimiter::new(
        Duration::from_secs(1),
        Box::new(move || *clk.lock()),
    ));
    let cb = NativeCallbacks::with_sink(
        state.clone(),
        theme.clone(),
        Arc::new(settings),
        sink.clone() as Arc<dyn NotificationSink>,
        rl,
    );
    Harness {
        cb,
        state,
        theme,
        sink,
        clock,
    }
}

fn default_harness() -> Harness {
    harness(Settings::default())
}

// ── Per-action_type dispatch tests ──────────────────────────────────

#[test]
fn osc_0_sets_title_and_icon() {
    let h = default_harness();
    h.cb.on_osc(OSC_SET_TITLE_AND_ICON, "hello");
    let s = h.state.lock();
    assert_eq!(s.title.as_deref(), Some("hello"));
    assert_eq!(s.icon_name.as_deref(), Some("hello"));
}

#[test]
fn osc_1_sets_icon_name_only() {
    let h = default_harness();
    h.cb.on_osc(OSC_SET_ICON_NAME, "icon");
    let s = h.state.lock();
    assert_eq!(s.icon_name.as_deref(), Some("icon"));
    assert!(s.title.is_none());
}

#[test]
fn osc_2_sets_title_only() {
    let h = default_harness();
    h.cb.on_osc(OSC_SET_TITLE, "win-title");
    let s = h.state.lock();
    assert_eq!(s.title.as_deref(), Some("win-title"));
    assert!(s.icon_name.is_none());
}

#[test]
fn osc_4_sets_palette_and_marks_theme_dirty() {
    let h = default_harness();
    h.cb.on_osc(OSC_SET_COLOR_PALETTE, "5;rgb:11/22/33");
    assert_eq!(h.theme.lock().palette256[5], Some(Rgb(0x11, 0x22, 0x33)));
    assert!(h.cb.take_theme_dirty());
    // Second drain returns false (latch behavior).
    assert!(!h.cb.take_theme_dirty());
}

#[test]
fn osc_7_sets_cwd() {
    let h = default_harness();
    h.cb.on_osc(OSC_SET_WORKING_DIRECTORY, "file:///home/me");
    assert_eq!(h.state.lock().cwd.as_deref(), Some("file:///home/me"));
}

#[test]
fn osc_8_is_logged_only() {
    let h = default_harness();
    h.cb.on_osc(OSC_HYPERLINK, "id=42;https://example.com");
    // No state mutation expected.
    let s = h.state.lock();
    assert!(s.title.is_none());
    assert!(s.icon_name.is_none());
}

#[test]
fn osc_9_emits_notification() {
    let h = default_harness();
    h.cb.on_osc(OSC_NOTIFICATION, "Build done;all green");
    let calls = h.sink.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "Build done");
    assert_eq!(calls[0].1, "all green");
    assert_eq!(h.state.lock().pending_notifications.len(), 1);
}

#[test]
fn osc_9_no_separator_uses_fallback_title() {
    let h = default_harness();
    // Title pre-populated by an earlier OSC 2.
    h.cb.on_osc(OSC_SET_TITLE, "the-title");
    h.cb.on_osc(OSC_NOTIFICATION, "body only");
    let calls = h.sink.calls();
    assert_eq!(calls[0].0, "the-title");
    assert_eq!(calls[0].1, "body only");
}

#[test]
fn osc_10_sets_fg_and_marks_theme_dirty() {
    let h = default_harness();
    h.cb.on_osc(OSC_SET_FG, "rgb:11/22/33");
    assert_eq!(h.theme.lock().fg, Rgb(0x11, 0x22, 0x33));
    assert!(h.cb.take_theme_dirty());
}

#[test]
fn osc_11_sets_bg_and_marks_theme_dirty() {
    let h = default_harness();
    h.cb.on_osc(OSC_SET_BG, "#445566");
    assert_eq!(h.theme.lock().bg, Rgb(0x44, 0x55, 0x66));
    assert!(h.cb.take_theme_dirty());
}

#[test]
fn osc_12_sets_cursor_fg_and_marks_theme_dirty() {
    let h = default_harness();
    h.cb.on_osc(OSC_SET_CURSOR_FG, "rgb:aa/bb/cc");
    assert_eq!(h.theme.lock().cursor_fg, Rgb(0xaa, 0xbb, 0xcc));
    assert!(h.cb.take_theme_dirty());
}

#[test]
fn osc_22_updates_cursor_style() {
    let h = default_harness();
    h.cb.on_osc(OSC_CURSOR_STYLE, "underline");
    assert_eq!(h.theme.lock().cursor_style, CursorStyle::Underline);
    assert!(h.cb.take_theme_dirty());
}

#[test]
fn osc_52_write_default_allows_within_quota() {
    let h = default_harness();
    // "hi" -> base64 "aGk="
    h.cb.on_osc(OSC_CLIPBOARD, "c;aGk=");
    let s = h.state.lock();
    assert_eq!(s.pending_clipboard_writes.len(), 1);
    assert_eq!(s.pending_clipboard_writes[0].0, "c");
    assert_eq!(s.pending_clipboard_writes[0].1, "hi");
}

#[test]
fn osc_52_query_default_allows_read() {
    let h = default_harness();
    h.cb.on_osc(OSC_CLIPBOARD, "p;?");
    let s = h.state.lock();
    assert_eq!(s.pending_clipboard_reads, vec!["p".to_string()]);
}

#[test]
fn osc_52_query_denied_when_read_disabled() {
    let mut settings = Settings::default();
    settings.clipboard_read_osc52 = false;
    let h = harness(settings);
    h.cb.on_osc(OSC_CLIPBOARD, "c;?");
    assert!(h.state.lock().pending_clipboard_reads.is_empty());
}

#[test]
fn osc_52_write_denied_when_over_quota() {
    let mut settings = Settings::default();
    // 3 bytes max → "hello" (5 bytes) must be rejected.
    settings.clipboard_max_size_osc52 = 3;
    let h = harness(settings);
    // "hello" -> base64 "aGVsbG8="
    h.cb.on_osc(OSC_CLIPBOARD, "c;aGVsbG8=");
    assert!(h.state.lock().pending_clipboard_writes.is_empty());
}

#[test]
fn osc_52_clear_pushes_empty_write() {
    let h = default_harness();
    h.cb.on_osc(OSC_CLIPBOARD, "c;");
    let s = h.state.lock();
    assert_eq!(s.pending_clipboard_writes.len(), 1);
    assert_eq!(s.pending_clipboard_writes[0].1, "");
}

#[test]
fn osc_104_resets_palette() {
    let h = default_harness();
    h.theme.lock().palette256[5] = Some(Rgb(1, 2, 3));
    h.cb.on_osc(OSC_RESET_COLOR_PALETTE, "");
    assert!(h.theme.lock().palette256.iter().all(|e| e.is_none()));
    assert!(h.cb.take_theme_dirty());
}

#[test]
fn osc_110_resets_fg() {
    let h = default_harness();
    h.theme.lock().fg = Rgb(1, 2, 3);
    h.cb.on_osc(OSC_RESET_FG, "");
    assert_eq!(h.theme.lock().fg, crate::render::theme::DEFAULT_TERMINAL_FG);
    assert!(h.cb.take_theme_dirty());
}

#[test]
fn osc_111_resets_bg() {
    let h = default_harness();
    h.theme.lock().bg = Rgb(1, 2, 3);
    h.cb.on_osc(OSC_RESET_BG, "");
    assert_eq!(h.theme.lock().bg, Rgb::BLACK);
    assert!(h.cb.take_theme_dirty());
}

#[test]
fn osc_112_resets_cursor_fg_to_active_scheme_color() {
    // task0003 AC-3: OSC 112 restores the ACTIVE SCHEME's cursor
    // color, not a hard-coded preset. `scheme_cursor_fg` stands in
    // for a non-default scheme's cursor color (as `apply_color_scheme`
    // would seed it); `cursor_fg` stands in for an OSC 12 override.
    let h = default_harness();
    {
        let mut theme = h.theme.lock();
        theme.scheme_cursor_fg = Rgb(9, 8, 7);
        theme.cursor_fg = Rgb(1, 2, 3);
    }
    h.cb.on_osc(OSC_RESET_CURSOR_FG, "");
    assert_eq!(h.theme.lock().cursor_fg, Rgb(9, 8, 7));
    assert_ne!(h.theme.lock().cursor_fg, Theme::DEFAULT_CURSOR_FG);
    assert!(h.cb.take_theme_dirty());
}

// ── task0004 AC-5: on_reset restores an active OSC 12 override ────

#[test]
fn on_reset_restores_active_cursor_override_to_active_scheme_color() {
    let h = default_harness();
    h.theme.lock().scheme_cursor_fg = Rgb(9, 8, 7);
    h.cb.on_osc(OSC_SET_CURSOR_FG, "rgb:01/02/03");
    assert!(h.cb.take_theme_dirty(), "OSC 12 itself marked dirty");
    assert!(h.theme.lock().cursor_fg_override_active);

    h.cb.on_reset();

    let theme = h.theme.lock();
    assert_eq!(theme.cursor_fg, Rgb(9, 8, 7));
    assert!(!theme.cursor_fg_override_active);
    drop(theme);
    assert!(
        h.cb.take_theme_dirty(),
        "on_reset marks dirty when it changed cursor_fg"
    );
}

#[test]
fn on_reset_is_a_noop_without_an_active_override() {
    let h = default_harness();
    h.cb.on_reset();
    assert!(!h.cb.take_theme_dirty());
}

#[test]
fn osc_133_callback_is_a_noop_for_native_state() {
    // OSC 133 marks are now captured in `term_core`
    // (`push_pending_prompt_mark`) and drained by the tab via
    // `take_prompt_marks`. The callback retains its dispatch arm only to
    // keep the wasm/WebView `on_osc(133, …)` contract; it must not mutate
    // any `NativeCallbackState`.
    let h = default_harness();
    h.cb.on_osc(OSC_SEMANTIC_PROMPT, "A");
    h.cb.on_osc(OSC_SEMANTIC_PROMPT, "D;42");
    let s = h.state.lock();
    assert!(s.title.is_none());
    assert!(s.osc_queue.is_empty());
    assert!(s.pending_notifications.is_empty());
}

#[test]
fn osc_100_emterm_extension_pushes_to_queue() {
    let h = default_harness();
    h.cb.on_osc(OSC_EMTERM_EXTENSION, "markdown;hello");
    let s = h.state.lock();
    assert_eq!(s.osc_queue.len(), 1);
    assert_eq!(s.osc_queue[0].payload, "markdown;hello");
}

#[test]
fn osc_100_strips_emterm_namespace_token_from_real_wire_form() {
    // The real CLI wire form is `OSC 777;emterm;<kind>;…` and term_core
    // delivers the `emterm;` prefix intact. The extension arm must strip
    // it once so the viewer sees the post-namespace `<kind>;<verb>;…`.
    let h = default_harness();
    h.cb.on_osc(
        OSC_EMTERM_EXTENSION,
        "emterm;markdown;begin;id=x;format=gfm",
    );
    let s = h.state.lock();
    assert_eq!(s.osc_queue.len(), 1);
    assert_eq!(s.osc_queue[0].payload, "markdown;begin;id=x;format=gfm");
}

// ── task0005: OSC 777 agent-status routing ────────────────────────

#[test]
fn osc_100_agent_status_set_routes_to_pending_agent_status_not_osc_queue() {
    let h = default_harness();
    h.cb.on_osc(
        OSC_EMTERM_EXTENSION,
        "emterm;agent-status;v=1;state=working;name=claude",
    );
    let s = h.state.lock();
    assert_eq!(
        s.pending_agent_status,
        vec![crate::agent_status::AgentStatusEvent::Set {
            state: crate::agent_status::AgentState::Working,
            name: Some("claude".to_string()),
        }]
    );
    assert!(s.osc_queue.is_empty());
}

#[test]
fn osc_100_agent_status_clear_routes_to_pending_agent_status() {
    let h = default_harness();
    h.cb.on_osc(OSC_EMTERM_EXTENSION, "emterm;agent-status;clear");
    let s = h.state.lock();
    assert_eq!(
        s.pending_agent_status,
        vec![crate::agent_status::AgentStatusEvent::Clear]
    );
}

#[test]
fn osc_100_agent_status_invalid_payload_falls_through_to_osc_queue() {
    // A malformed agent-status payload (missing `state`) is rejected by
    // `crate::agent_status::parse`, so the extension arm falls through
    // to the legacy viewer queue exactly as any other unrecognized OSC
    // 777 payload would — it is not silently dropped.
    let h = default_harness();
    h.cb.on_osc(OSC_EMTERM_EXTENSION, "emterm;agent-status;v=1");
    let s = h.state.lock();
    assert!(s.pending_agent_status.is_empty());
    assert_eq!(s.osc_queue.len(), 1);
}

// ── agent-exit-after-icon (task0002): pending_latch_feed ordering ──

#[test]
fn osc_100_agent_status_set_pushes_set_marker_to_latch_feed() {
    let h = default_harness();
    h.cb.on_osc(
        OSC_EMTERM_EXTENSION,
        "emterm;agent-status;v=1;state=working",
    );
    assert_eq!(h.state.lock().pending_latch_feed, vec![LatchFeedEvent::Set]);
}

#[test]
fn osc_100_agent_status_clear_pushes_clear_marker_to_latch_feed() {
    let h = default_harness();
    h.cb.on_osc(OSC_EMTERM_EXTENSION, "emterm;agent-status;clear");
    assert_eq!(
        h.state.lock().pending_latch_feed,
        vec![LatchFeedEvent::Clear]
    );
}

#[test]
fn osc_100_invalid_agent_status_payload_does_not_push_to_latch_feed() {
    let h = default_harness();
    h.cb.on_osc(OSC_EMTERM_EXTENSION, "emterm;agent-status;v=1");
    assert!(h.state.lock().pending_latch_feed.is_empty());
}

#[test]
fn osc_133_a_and_d_push_prompt_mark_candidates_to_latch_feed() {
    let h = default_harness();
    h.cb.on_osc(OSC_SEMANTIC_PROMPT, "D;0");
    h.cb.on_osc(OSC_SEMANTIC_PROMPT, "A");
    assert_eq!(
        h.state.lock().pending_latch_feed,
        vec![
            LatchFeedEvent::PromptMark(crate::prompts::PromptMarkKind::CommandEnd),
            LatchFeedEvent::PromptMark(crate::prompts::PromptMarkKind::PromptStart),
        ]
    );
}

#[test]
fn osc_133_unrecognized_kind_does_not_push_to_latch_feed() {
    let h = default_harness();
    h.cb.on_osc(OSC_SEMANTIC_PROMPT, "Z");
    assert!(h.state.lock().pending_latch_feed.is_empty());
}

#[test]
fn set_and_live_133_marks_preserve_true_relative_order_in_latch_feed() {
    // FR4: OSC 777 Set/Clear and OSC 133 D/A candidates share ONE
    // ordered log (`pending_latch_feed`), reflecting the true
    // synchronous `on_osc` call order — not two independently
    // populated queues that a caller would have to re-interleave.
    let h = default_harness();
    h.cb.on_osc(
        OSC_EMTERM_EXTENSION,
        "emterm;agent-status;v=1;state=working",
    );
    h.cb.on_osc(OSC_SEMANTIC_PROMPT, "D;0");
    h.cb.on_osc(OSC_SEMANTIC_PROMPT, "A");
    h.cb.on_osc(OSC_EMTERM_EXTENSION, "emterm;agent-status;clear");
    assert_eq!(
        h.state.lock().pending_latch_feed,
        vec![
            LatchFeedEvent::Set,
            LatchFeedEvent::PromptMark(crate::prompts::PromptMarkKind::CommandEnd),
            LatchFeedEvent::PromptMark(crate::prompts::PromptMarkKind::PromptStart),
            LatchFeedEvent::Clear,
        ]
    );
}

// ── Phase D: OSC 777 statusbar routing ────────────────────────────

#[test]
fn osc_100_with_statusbar_prefix_routes_to_dispatcher() {
    let mut h = default_harness();
    let dispatcher = Arc::new(StatusBarOscDispatcher::new());
    h.cb.set_statusbar_dispatcher(dispatcher.clone());
    h.cb.on_osc(OSC_EMTERM_EXTENSION, "statusbar;set;left;hi");
    // Dispatched: state updated.
    assert_eq!(dispatcher.snapshot().left, "hi");
    // NOT pushed to osc_queue.
    assert!(h.state.lock().osc_queue.is_empty());
}

#[test]
fn osc_100_without_statusbar_prefix_still_pushes_to_queue_when_dispatcher_present() {
    let mut h = default_harness();
    let dispatcher = Arc::new(StatusBarOscDispatcher::new());
    h.cb.set_statusbar_dispatcher(dispatcher.clone());
    h.cb.on_osc(OSC_EMTERM_EXTENSION, "markdown;hello");
    assert_eq!(h.state.lock().osc_queue.len(), 1);
    assert_eq!(h.state.lock().osc_queue[0].payload, "markdown;hello");
    // Dispatcher untouched.
    assert!(dispatcher.snapshot().left.is_empty());
}

#[test]
fn osc_101_iterm2_is_logged_only() {
    let h = default_harness();
    h.cb.on_osc(OSC_ITERM2, "File=name=foo:");
    let s = h.state.lock();
    assert!(s.title.is_none());
    assert!(s.osc_queue.is_empty());
}

#[test]
fn osc_255_unknown_is_logged_only() {
    let h = default_harness();
    h.cb.on_osc(OSC_UNKNOWN, "something");
    let s = h.state.lock();
    assert!(s.title.is_none());
    assert!(s.osc_queue.is_empty());
}

// ── TS-13: pre-mux OSC 9999 emterm-mux Welcome reaches the mux APC sink ─
#[test]
fn osc_9999_emterm_mux_inband_routed_to_pending_apc() {
    // A pre-mux Windows-ConPTY Welcome arrives as an OSC 9999
    // `emterm-mux;<base64>` frame. `term_core` no longer special-cases it
    // (NFR5): it now reaches the app via `on_osc(OSC_MUX_INBAND, …)`. The
    // app layer recognizes the `emterm-mux;` prefix and routes the full
    // frame string into the same `pending_apc` sink `on_apc` feeds, so
    // `partition_apc_for_mux` can establish mux.
    let h = default_harness();
    let frame = "emterm-mux;V2VsY29tZQ==";
    h.cb.on_osc(OSC_MUX_INBAND, frame);
    let s = h.state.lock();
    assert_eq!(s.pending_apc.len(), 1, "frame buffered into pending_apc");
    assert_eq!(s.pending_apc[0], frame.as_bytes().to_vec());
    // It must NOT leak into the OSC viewer queue or set a title.
    assert!(s.osc_queue.is_empty());
    assert!(s.title.is_none());
}

#[test]
fn osc_9999_non_mux_prefix_is_dropped() {
    // OSC 9999 whose data lacks the `emterm-mux;` prefix is not a mux
    // frame and must be dropped (parity with the old term_core guard,
    // now enforced in the app layer).
    let h = default_harness();
    h.cb.on_osc(OSC_MUX_INBAND, "something-else;data");
    let s = h.state.lock();
    assert!(s.pending_apc.is_empty());
    assert!(s.osc_queue.is_empty());
}

// ── Rate limiter behavior ───────────────────────────────────────────

#[test]
fn rate_limiter_dedupes_identical_pair_within_window() {
    let h = default_harness();
    h.cb.on_osc(OSC_NOTIFICATION, "title;body");
    h.cb.on_osc(OSC_NOTIFICATION, "title;body");
    // Only the first call reaches the sink.
    assert_eq!(h.sink.calls().len(), 1);
}

#[test]
fn rate_limiter_allows_after_window_elapsed() {
    let h = default_harness();
    h.cb.on_osc(OSC_NOTIFICATION, "title;body");
    // Advance the injected clock past the dedupe window.
    {
        let mut clk = h.clock.lock();
        *clk += Duration::from_secs(2);
    }
    h.cb.on_osc(OSC_NOTIFICATION, "title;body");
    assert_eq!(h.sink.calls().len(), 2);
}

#[test]
fn rate_limiter_distinct_pairs_not_deduped() {
    let h = default_harness();
    h.cb.on_osc(OSC_NOTIFICATION, "A;1");
    h.cb.on_osc(OSC_NOTIFICATION, "A;2");
    h.cb.on_osc(OSC_NOTIFICATION, "B;1");
    assert_eq!(h.sink.calls().len(), 3);
}

// ── Existing-behavior regression coverage ──────────────────────────

#[test]
fn on_apc_buffers_payload_into_pending_apc() {
    let h = default_harness();
    h.cb.on_apc(b"Ga=q;");
    let st = h.state.lock();
    assert_eq!(st.pending_apc.len(), 1);
    assert_eq!(st.pending_apc[0], b"Ga=q;".to_vec());
    assert!(st.pending_dcs.is_empty());
}

#[test]
fn on_dcs_buffers_payload_into_pending_dcs() {
    let h = default_harness();
    h.cb.on_dcs(b"0;0;0q");
    let st = h.state.lock();
    assert_eq!(st.pending_dcs.len(), 1);
    assert_eq!(st.pending_dcs[0], b"0;0;0q".to_vec());
    assert!(st.pending_apc.is_empty());
}

#[test]
fn on_apc_appends_in_order_across_multiple_calls() {
    let h = default_harness();
    h.cb.on_apc(b"a");
    h.cb.on_apc(b"b");
    h.cb.on_apc(b"c");
    let st = h.state.lock();
    assert_eq!(
        st.pending_apc,
        vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]
    );
}

#[test]
fn on_bell_increments_counter() {
    let h = default_harness();
    h.cb.on_bell();
    h.cb.on_bell();
    assert_eq!(h.state.lock().bell_count, 2);
}

// ── Parser micro-tests ──────────────────────────────────────────────

#[test]
fn parse_osc52_write() {
    assert_eq!(
        parse_osc52("c;aGk="),
        Some(Osc52Action::Write {
            target: "c".into(),
            payload: "aGk=".into(),
        })
    );
}

#[test]
fn parse_osc52_query() {
    assert_eq!(
        parse_osc52("p;?"),
        Some(Osc52Action::Query { target: "p".into() })
    );
}

#[test]
fn parse_osc52_clear() {
    assert_eq!(
        parse_osc52("c;"),
        Some(Osc52Action::Clear { target: "c".into() })
    );
}

#[test]
fn parse_osc52_missing_separator_returns_none() {
    assert_eq!(parse_osc52("garbage"), None);
    assert_eq!(parse_osc52(""), None);
}

#[test]
fn parse_osc9_with_separator() {
    let (t, b) = parse_osc9("Title;Body", None);
    assert_eq!(t, "Title");
    assert_eq!(b, "Body");
}

#[test]
fn parse_osc9_no_separator_uses_fallback() {
    let (t, b) = parse_osc9("just body", Some("fallback"));
    assert_eq!(t, "fallback");
    assert_eq!(b, "just body");
}

#[test]
fn parse_osc9_empty_title_uses_fallback() {
    let (t, b) = parse_osc9(";body", Some("fb"));
    assert_eq!(t, "fb");
    assert_eq!(b, ";body");
}
