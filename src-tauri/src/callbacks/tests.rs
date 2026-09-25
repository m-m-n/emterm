use super::*;
use crate::render::theme::{CursorStyle, Rgb};
use term_core::{OscResponder, OscTerminator};

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

impl Harness {
    /// SC-2 responder wired to this harness's shared `theme` / `state`
    /// Arcs, exactly as `Tab::build` wires it in production
    /// (osc-color-query-response D7). OSC 4/10/11/12 are handled
    /// EXCLUSIVELY through this responder now, not through
    /// `self.cb.on_osc` (see `ThemeColorResponder`'s doc).
    fn responder(&self) -> ThemeColorResponder {
        ThemeColorResponder::new(self.theme.clone(), self.state.clone())
    }
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
    // OSC 4/10/11/12 are handled through the SC-2 `ThemeColorResponder`,
    // not through `NativeCallbacks::on_osc` (osc-color-query-response D7) —
    // see `Harness::responder`'s doc.
    let h = default_harness();
    h.responder().respond(
        OSC_SET_COLOR_PALETTE as u16,
        "5;rgb:11/22/33",
        OscTerminator::Bel,
    );
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
    h.responder()
        .respond(OSC_SET_FG as u16, "rgb:11/22/33", OscTerminator::Bel);
    assert_eq!(h.theme.lock().fg, Rgb(0x11, 0x22, 0x33));
    assert!(h.cb.take_theme_dirty());
}

#[test]
fn osc_11_sets_bg_and_marks_theme_dirty() {
    let h = default_harness();
    h.responder()
        .respond(OSC_SET_BG as u16, "#445566", OscTerminator::Bel);
    assert_eq!(h.theme.lock().bg, Rgb(0x44, 0x55, 0x66));
    assert!(h.cb.take_theme_dirty());
}

#[test]
fn osc_12_sets_cursor_fg_and_marks_theme_dirty() {
    let h = default_harness();
    h.responder()
        .respond(OSC_SET_CURSOR_FG as u16, "rgb:aa/bb/cc", OscTerminator::Bel);
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
    h.responder()
        .respond(OSC_SET_CURSOR_FG as u16, "rgb:01/02/03", OscTerminator::Bel);
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
    drop(s);
    assert!(h.sink.calls().is_empty());
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

// ── task0001: body-markup escape (Unix only — notify-rust's
// `get_capabilities()` is an XDG-only export, see `NotifyRustSink::send`) ─

#[cfg(unix)]
mod body_markup_escape {
    use super::*;

    // AC-1 (TS1): a body containing an HTML anchor tag becomes an
    // escaped literal — no raw `<`/`>` survive.
    #[test]
    fn escape_body_markup_neutralizes_html_tags() {
        let input = r#"<a href="https://attacker.invalid">Sign in</a>"#;
        let out = escape_body_markup(input);
        assert!(!out.contains('<'), "raw '<' survived: {out}");
        assert!(!out.contains('>'), "raw '>' survived: {out}");
        assert_eq!(
            out,
            r#"&lt;a href="https://attacker.invalid"&gt;Sign in&lt;/a&gt;"#
        );
    }

    // AC-2 (TS2): `&` is replaced first, so a pre-existing `&amp;` becomes
    // `&amp;amp;` rather than staying `&amp;` (which would leave the
    // ambiguity the ordering rule exists to remove).
    #[test]
    fn escape_body_markup_double_escapes_a_preexisting_entity() {
        assert_eq!(
            escape_body_markup("Tom & Jerry &amp; co"),
            "Tom &amp; Jerry &amp;amp; co"
        );
    }

    // AC-3 (TS3): a title with a meta-character right at the
    // `sanitize_title` 100-char truncation boundary, escaped AFTER
    // truncation, produces a complete trailing entity reference — the
    // escape step never sees a boundary to split.
    #[test]
    fn escape_body_markup_after_sanitize_title_truncation_keeps_entity_intact() {
        let mut title = "a".repeat(99);
        title.push('<'); // 100th character — sanitize_title's cutoff.
        title.push_str("TRAILING-SHOULD-BE-TRUNCATED>");

        let sanitized = crate::notifications::sanitize_title(&title);
        assert_eq!(sanitized.chars().count(), 100);
        assert!(sanitized.ends_with('<'));

        let escaped = escape_body_markup(&sanitized);
        assert!(
            escaped.ends_with("&lt;"),
            "entity reference was split or missing: {escaped}"
        );
        assert!(!escaped.contains("TRUNCATED"));
    }

    // AC-1/AC-2 (TS1/TS2, notification-markup-fail-closed FR1/FR2): the
    // fail-closed predicate asks the inverted question — is the ABSENCE of
    // `body-markup` explicitly confirmed? Only a successful query whose
    // list omits it answers yes; a list containing it, or a fetch failure,
    // both answer no, and callers escape on no (fail-closed).
    #[test]
    fn body_markup_absence_confirmed_is_true_only_for_a_successful_list_without_it() {
        let absent: Result<Vec<String>, ()> = Ok(vec!["actions".to_string()]);
        assert!(body_markup_absence_confirmed(&absent));
    }

    #[test]
    fn body_markup_absence_confirmed_is_false_when_present_in_a_successful_list() {
        let present: Result<Vec<String>, ()> =
            Ok(vec!["actions".to_string(), "body-markup".to_string()]);
        assert!(!body_markup_absence_confirmed(&present));
    }

    // AC-1 (TS1, FR1/FR3): a failed fetch does NOT confirm absence — this
    // is the fail-closed side; callers escape on `false` (closes PR #35
    // review finding eade9e7f97a29a29: a failed `GetCapabilities()` no
    // longer lands on the unescaped side).
    #[test]
    fn body_markup_absence_confirmed_is_false_on_fetch_failure() {
        let failed: Result<Vec<String>, ()> = Err(());
        assert!(!body_markup_absence_confirmed(&failed));
    }

    // AC-2 (TS2, composed): when body-markup absence is explicitly
    // confirmed (a successful list omitting it), the same escape/no-escape
    // decision `NotifyRustSink::send` makes leaves the body byte-for-byte
    // unchanged — a literal `&amp;` in the input must not become visible
    // as anything else (no partial/accidental transform).
    #[test]
    fn unconfirmed_capabilities_leave_the_body_unchanged() {
        let body = r#"Tom & Jerry &amp; <b>bold</b>"#;
        let absence_confirmed: Result<Vec<String>, ()> = Ok(vec!["actions".to_string()]);
        let out = if body_markup_absence_confirmed(&absence_confirmed) {
            body.to_string()
        } else {
            escape_body_markup(body)
        };
        assert_eq!(out, body);
    }

    // AC-3 (TS3): both notification-body producers — tab-activity
    // (`sanitize_title` + `notification_body`) and agent-status
    // (`agent_notification_body`) — are covered by the same sink-side
    // escape decision when the capability query confirms `body-markup`
    // support (absence NOT confirmed). Proves the single choke point
    // covers both pipelines (IMPLEMENTATION.md D1 NFR2).
    #[test]
    fn tab_activity_and_agent_bodies_are_both_escaped_when_confirmed() {
        let confirmed: Result<Vec<String>, ()> = Ok(vec!["body-markup".to_string()]);

        // Tab-activity path.
        let sanitized = crate::notifications::sanitize_title(r#"<img src=x onerror=alert(1)>"#);
        let tab_body = crate::notifications::notification_body(
            &sanitized,
            crate::notifications::ActivityKind::Output,
            crate::i18n::Locale::En,
        );
        assert!(
            tab_body.contains('<'),
            "fixture lost its markup: {tab_body}"
        );
        let tab_escaped = if body_markup_absence_confirmed(&confirmed) {
            tab_body.clone()
        } else {
            escape_body_markup(&tab_body)
        };
        assert!(!tab_escaped.contains('<'));
        assert!(!tab_escaped.contains('>'));

        // Agent-status path.
        let transition = crate::notifications::AgentTransition {
            old_state: None,
            new_state: crate::notifications::AgentState::Blocked,
            name: Some("<script>evil</script>".to_string()),
        };
        let agent_body = crate::notifications::agent_notification_body(
            &transition,
            "my-tab",
            crate::i18n::Locale::En,
        );
        assert!(
            agent_body.contains('<'),
            "fixture lost its markup: {agent_body}"
        );
        let agent_escaped = if body_markup_absence_confirmed(&confirmed) {
            agent_body.clone()
        } else {
            escape_body_markup(&agent_body)
        };
        assert!(!agent_escaped.contains('<'));
        assert!(!agent_escaped.contains('>'));
    }
}

// ── task0001: summary-markup escape (mirrors `body_markup_escape` above;
// same pinned components, applied to the title/summary side of
// `NotifyRustSink::send` under the single per-send gate — D2). Exercises
// the private `escape_for_send` composition unit since the sink itself
// cannot be invoked against a real D-Bus connection (Test Notes). ────────

#[cfg(unix)]
mod summary_markup_escape {
    use super::*;

    // AC-1 (TS1): with a confirmed capability list, a tag-bearing title is
    // converted to entity references — same ordering (`&` first) as the
    // body path produces for the same input.
    #[test]
    fn confirmed_capabilities_escape_a_tag_bearing_title_like_the_body_path() {
        let title = r#"<a href="https://attacker.invalid">Sign in</a>"#;
        let confirmed: Result<Vec<String>, ()> = Ok(vec!["body-markup".to_string()]);
        let (escaped_title, _escaped_body) = escape_for_send(title, "body", &confirmed);
        assert!(
            !escaped_title.contains('<'),
            "raw '<' survived: {escaped_title}"
        );
        assert!(
            !escaped_title.contains('>'),
            "raw '>' survived: {escaped_title}"
        );
        assert_eq!(
            escaped_title,
            r#"&lt;a href="https://attacker.invalid"&gt;Sign in&lt;/a&gt;"#
        );
    }

    // AC-1: `&` is replaced first in the title too, so a pre-existing
    // `&amp;` double-escapes — mirrors the pinned body behavior.
    #[test]
    fn confirmed_capabilities_double_escape_a_preexisting_entity_in_a_title() {
        let confirmed: Result<Vec<String>, ()> = Ok(vec!["body-markup".to_string()]);
        let (escaped_title, _) = escape_for_send("Tom & Jerry &amp; co", "body", &confirmed);
        assert_eq!(escaped_title, "Tom &amp; Jerry &amp;amp; co");
    }

    // AC-2 (TS2): a successful capability query whose list omits
    // `body-markup` — explicit absence confirmed — leaves the title
    // byte-for-byte unchanged (FR2, unchanged from before this feature).
    #[test]
    fn unconfirmed_capability_list_leaves_the_title_unchanged() {
        let title = r#"Tom & Jerry &amp; <b>bold</b>"#;
        let absence_confirmed: Result<Vec<String>, ()> = Ok(vec!["actions".to_string()]);
        let (escaped_title, _) = escape_for_send(title, "body", &absence_confirmed);
        assert_eq!(escaped_title, title);
    }

    // AC-1 (TS1, FR1/FR3, notification-markup-fail-closed): fail-closed —
    // a failed capability fetch escapes BOTH the title and the body from
    // the single per-send evaluation (no raw `<`, `>`, `&` survive in
    // either field). Closes PR #35 review finding eade9e7f97a29a29: a
    // failed `GetCapabilities()` no longer lands on the unescaped side.
    #[test]
    fn failed_capability_fetch_escapes_both_title_and_body_in_the_same_call() {
        let title = r#"<script>alert(1)</script>"#;
        let body = r#"<a href="https://attacker.invalid">Sign in</a> & regards"#;
        let failed: Result<Vec<String>, ()> = Err(());
        let (escaped_title, escaped_body) = escape_for_send(title, body, &failed);

        assert!(
            !escaped_title.contains('<') && !escaped_title.contains('>'),
            "raw markup survived in title: {escaped_title}"
        );
        assert_eq!(escaped_title, "&lt;script&gt;alert(1)&lt;/script&gt;");

        assert!(
            !escaped_body.contains('<') && !escaped_body.contains('>'),
            "raw markup survived in body: {escaped_body}"
        );
        assert_eq!(
            escaped_body,
            r#"&lt;a href="https://attacker.invalid"&gt;Sign in&lt;/a&gt; &amp; regards"#
        );
    }

    // AC-2 / D2: a single gate evaluation drives BOTH fields — when
    // absence is confirmed (successful list without `body-markup`), body
    // is unchanged too, in the same call.
    #[test]
    fn unconfirmed_capability_list_leaves_the_body_unchanged_in_the_same_call() {
        let absence_confirmed: Result<Vec<String>, ()> = Ok(vec!["actions".to_string()]);
        let (_escaped_title, escaped_body) =
            escape_for_send("title", "Tom & Jerry &amp; <b>bold</b>", &absence_confirmed);
        assert_eq!(escaped_body, "Tom & Jerry &amp; <b>bold</b>");
    }

    // AC-3 (TS3): a title truncated by `sanitize_title` to exactly 100
    // characters, ending in `<`, escapes to a complete trailing entity
    // reference — the escape happens strictly after truncation, so the
    // entity is never split.
    #[test]
    fn confirmed_capabilities_escape_a_sanitize_title_truncated_title_intact() {
        let mut title = "a".repeat(99);
        title.push('<'); // 100th character — sanitize_title's cutoff.
        title.push_str("TRAILING-SHOULD-BE-TRUNCATED>");

        let sanitized = crate::notifications::sanitize_title(&title);
        assert_eq!(sanitized.chars().count(), 100);
        assert!(sanitized.ends_with('<'));

        let confirmed: Result<Vec<String>, ()> = Ok(vec!["body-markup".to_string()]);
        let (escaped_title, _) = escape_for_send(&sanitized, "body", &confirmed);
        assert!(
            escaped_title.ends_with("&lt;"),
            "entity reference was split or missing: {escaped_title}"
        );
        assert!(!escaped_title.contains("TRUNCATED"));
    }

    // AC-4 (TS4): the OSC 9 fallback-title branch (empty title segment →
    // current tab title, or "emterm" when there is none) produces a title
    // that flows through the same escaped summary decision as an explicit
    // title.
    #[test]
    fn osc9_fallback_title_flows_through_the_same_escaped_decision() {
        let confirmed: Result<Vec<String>, ()> = Ok(vec!["body-markup".to_string()]);

        // Fallback to the current tab title (untrusted OSC 0/2 payload).
        let (title, _body) = parse_osc9(";all green", Some("<tab title>"));
        assert_eq!(title, "<tab title>");
        let (escaped_title, _) = escape_for_send(&title, "all green", &confirmed);
        assert_eq!(escaped_title, "&lt;tab title&gt;");

        // Fallback to "emterm" when there's no tab title either — no meta
        // characters, so the escape is an identity transform (must not
        // corrupt a plain title).
        let (title, _body) = parse_osc9(";all green", None);
        assert_eq!(title, "emterm");
        let (escaped_title, _) = escape_for_send(&title, "all green", &confirmed);
        assert_eq!(escaped_title, "emterm");
    }

    // Edge case: a title consisting only of meta characters.
    #[test]
    fn confirmed_capabilities_escape_a_title_of_only_meta_characters() {
        let confirmed: Result<Vec<String>, ()> = Ok(vec!["body-markup".to_string()]);
        let (escaped_title, _) = escape_for_send("<>&", "body", &confirmed);
        assert_eq!(escaped_title, "&lt;&gt;&amp;");
    }
}

// ── notification-capability-query-skip task0001: on-demand capability
// query gate. Mirrors the escape-gate test modules above; pins the
// short-circuit's output-equivalence to `escape_for_send`, the "exactly
// one call when needed" property, and the no-caching property
// (IMPLEMENTATION.md D1-D3). ──────────────────────────────────────────

#[cfg(unix)]
mod capability_query_skip_gate {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    /// Build a `FnOnce` supplier that increments `calls` when invoked (at
    /// most once, since `escape_for_send_on_demand` consumes it) and
    /// returns a clone of `outcome`.
    fn counting_supplier(
        calls: Rc<Cell<u32>>,
        outcome: Result<Vec<String>, ()>,
    ) -> impl FnOnce() -> Result<Vec<String>, ()> {
        move || {
            calls.set(calls.get() + 1);
            outcome
        }
    }

    fn all_outcomes() -> Vec<Result<Vec<String>, ()>> {
        vec![
            Err(()),
            Ok(vec!["body-markup".to_string()]),
            Ok(vec!["actions".to_string()]),
        ]
    }

    // AC-1 (FR1, FR3, FR4; TS-1/TS-5): no metacharacter in title or body —
    // ASCII, non-ASCII, empty title, empty body, the fallback title
    // "emterm", and text with quotes/other symbols. The supplier is never
    // invoked, and the result is output-equivalent to what
    // `escape_for_send` would produce for each of the three outcomes
    // (D2: `escape_body_markup` is the identity on text without a
    // metacharacter).
    #[test]
    fn no_metacharacter_skips_the_query_and_returns_inputs_unchanged() {
        let cases: &[(&str, &str)] = &[
            ("hello", "world"),
            ("こんにちは", "世界"),
            ("", "body only"),
            ("title only", ""),
            ("emterm", "fallback title body"),
            (
                r#"quote " apos ' symbols !@#$%^*()"#,
                r#"more "quoted" 'text'"#,
            ),
        ];

        for (title, body) in cases.iter().copied() {
            for outcome in all_outcomes() {
                let calls = Rc::new(Cell::new(0));
                let supplier = counting_supplier(calls.clone(), outcome.clone());
                let (out_title, out_body) = escape_for_send_on_demand(title, body, supplier);
                assert_eq!(calls.get(), 0, "supplier called for {title:?}/{body:?}");
                assert_eq!(out_title, title);
                assert_eq!(out_body, body);

                // Output-equivalent to the previous always-query behavior.
                let expected = escape_for_send(title, body, &outcome);
                assert_eq!((out_title, out_body), expected);
            }
        }
    }

    // AC-2 (FR2, FR3, FR4; TS-2/TS-3/TS-4): a metacharacter in the title
    // only (including the OSC 9 fallback title `<tab title>`), the body
    // only (including a lone `&` and a pre-existing `&amp;`), or both
    // fields — under each of the three outcomes the supplier is called
    // exactly once and the returned pair equals `escape_for_send` for the
    // same title, body and outcome.
    #[test]
    fn metacharacter_in_title_or_body_queries_exactly_once_and_matches_escape_for_send() {
        let cases: &[(&str, &str)] = &[
            ("<tab title>", "all green"),
            ("plain title", "a & b"),
            ("plain title", "already &amp; escaped"),
            ("<script>", "<b>bold</b> & co"),
        ];

        for (title, body) in cases.iter().copied() {
            for outcome in all_outcomes() {
                let calls = Rc::new(Cell::new(0));
                let supplier = counting_supplier(calls.clone(), outcome.clone());
                let (out_title, out_body) = escape_for_send_on_demand(title, body, supplier);
                assert_eq!(
                    calls.get(),
                    1,
                    "supplier not called exactly once for {title:?}/{body:?}"
                );

                let expected = escape_for_send(title, body, &outcome);
                assert_eq!((out_title, out_body), expected);
            }
        }
    }

    // AC-2 spot check: a hard-coded expectation independent of
    // `escape_for_send`, for the OSC 9 fallback title under a failed
    // query (fail-closed).
    #[test]
    fn tab_title_fallback_escapes_under_a_failed_query() {
        let calls = Rc::new(Cell::new(0));
        let supplier = counting_supplier(calls.clone(), Err(()));
        let (title, _body) = escape_for_send_on_demand("<tab title>", "all green", supplier);
        assert_eq!(calls.get(), 1);
        assert_eq!(title, "&lt;tab title&gt;");
    }

    // AC-3 (NFR1; TS-7): two consecutive calls with metacharacter input,
    // each given its own counting stub with a different outcome, each
    // calls its own stub exactly once and follows its own outcome — no
    // result is carried over between calls.
    #[test]
    fn consecutive_calls_never_share_a_query_result() {
        let calls_a = Rc::new(Cell::new(0));
        let supplier_a = counting_supplier(calls_a.clone(), Err(()));
        let (title_a, body_a) = escape_for_send_on_demand("<a>", "body", supplier_a);
        assert_eq!(calls_a.get(), 1);
        assert_eq!((title_a.as_str(), body_a.as_str()), ("&lt;a&gt;", "body"));

        let calls_b = Rc::new(Cell::new(0));
        let supplier_b = counting_supplier(calls_b.clone(), Ok(vec!["actions".to_string()]));
        let (title_b, body_b) = escape_for_send_on_demand("<b>", "body", supplier_b);
        assert_eq!(calls_b.get(), 1);
        assert_eq!((title_b.as_str(), body_b.as_str()), ("<b>", "body"));

        // The first call's outcome did not leak into the second call.
        assert_eq!(calls_a.get(), 1);
    }

    // AC-4 (FR3, NFR2; TS-8): the identity property the short-circuit
    // relies on — `escape_body_markup` is a no-op on every printable
    // ASCII character outside the metacharacter set, and on a sample of
    // non-ASCII characters, but changes each of `&`, `<`, `>`. The
    // metacharacter constant contains exactly those three characters.
    #[test]
    fn escape_body_markup_is_identity_outside_the_metacharacter_set() {
        for c in ' '..='~' {
            let s = c.to_string();
            if MARKUP_METACHARACTERS.contains(&c) {
                assert_ne!(
                    escape_body_markup(&s),
                    s,
                    "{c:?} should be escaped but was not"
                );
            } else {
                assert_eq!(
                    escape_body_markup(&s),
                    s,
                    "{c:?} should be left unchanged but was not"
                );
            }
        }

        for c in ['こ', '世', 'é', '🎉', 'あ'] {
            let s = c.to_string();
            assert_eq!(
                escape_body_markup(&s),
                s,
                "non-ASCII {c:?} should be left unchanged"
            );
        }

        assert_eq!(MARKUP_METACHARACTERS.len(), 3);
        assert!(MARKUP_METACHARACTERS.contains(&'&'));
        assert!(MARKUP_METACHARACTERS.contains(&'<'));
        assert!(MARKUP_METACHARACTERS.contains(&'>'));
    }
}

// ── task0001: notification log redaction (IMPLEMENTATION.md "Redaction
// renderer" / "Diagnostic ID" contracts, "Redacted record format"). NOT
// Unix-gated (D4) — both log sites this feeds (the rate-limit warn record
// and the dispatch-success debug record) exist on every supported
// platform, unlike the escape helpers above. ────────────────────────────

mod notification_redaction {
    use super::*;

    // AC-1 (TS1): none of a URL, a fixture standing in for a token-like
    // string, or a command line survives in the rendering. Whole-token
    // assertions only (Test Notes): a length digit or a hex character of
    // the diagnostic ID can coincidentally match a single input
    // character, so per-character absence would be flaky for reasons
    // unrelated to redaction.
    #[test]
    fn redaction_contains_no_sensitive_substrings() {
        let title = "https://example.com/reset?auth=SENSITIVE-FIXTURE-VALUE";
        let body = "run: rm -rf /var/data --force";
        let out = redact_notification(title, body);
        assert!(!out.contains("https://example.com/reset?auth=SENSITIVE-FIXTURE-VALUE"));
        assert!(!out.contains("SENSITIVE-FIXTURE-VALUE"));
        assert!(!out.contains("rm -rf /var/data --force"));
    }

    // AC-2 (TS2): fixed field order, `name=value` shape, UTF-8 byte
    // counts (not char counts — "café" is 5 bytes / 4 chars, so this
    // pair distinguishes the two units), and exactly 3 fields.
    #[test]
    fn redaction_carries_byte_lengths_and_id_in_fixed_order_and_shape() {
        let title = "café";
        let body = "ok";
        assert_eq!(title.len(), 5);
        assert_eq!(title.chars().count(), 4);
        assert_eq!(body.len(), 2);

        let out = redact_notification(title, body);
        let fields: Vec<&str> = out.split(' ').collect();
        assert_eq!(fields.len(), 3, "expected exactly 3 fields: {out}");
        assert_eq!(fields[0], "title_len_bytes=5");
        assert_eq!(fields[1], "body_len_bytes=2");
        let id = fields[2]
            .strip_prefix("diag_id=")
            .unwrap_or_else(|| panic!("missing diag_id field: {out}"));
        assert_eq!(id.len(), 16);
        assert!(
            id.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "diag_id not 16 lowercase hex chars: {id}"
        );
    }

    // AC-2 (D5 totality): an empty (title, body) pair is not a failure
    // branch — both lengths render as zero.
    #[test]
    fn redaction_handles_empty_title_and_body() {
        let out = redact_notification("", "");
        assert!(out.contains("title_len_bytes=0"), "{out}");
        assert!(out.contains("body_len_bytes=0"), "{out}");
    }

    // AC-3 (TS3): 16 lowercase hex characters, fixed width.
    #[test]
    fn diagnostic_id_is_16_lowercase_hex_chars() {
        let id = notification_diagnostic_id("t", "b");
        assert_eq!(id.len(), 16);
        assert!(
            id.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
    }

    // AC-3 (TS3): equal for two renderings of the same pair within one
    // process run.
    #[test]
    fn diagnostic_id_is_stable_for_the_same_pair_within_one_run() {
        let a = notification_diagnostic_id("Build done", "all green");
        let b = notification_diagnostic_id("Build done", "all green");
        assert_eq!(a, b);
    }

    // AC-3 (TS3): differs for a pair that differs only in the body.
    #[test]
    fn diagnostic_id_differs_when_only_the_body_differs() {
        let a = notification_diagnostic_id("Build done", "all green");
        let b = notification_diagnostic_id("Build done", "one failure");
        assert_ne!(a, b);
    }

    // TS7 (AC-4): the rate-limit marker constant's value is unchanged by
    // this feature — operational log greps keyed on it keep working.
    #[test]
    fn rate_limit_marker_constant_value_is_unchanged() {
        assert_eq!(LOG_NOTIFY_RATE_LIMIT, "LOG_NOTIFY_RATE_LIMIT");
    }

    // TS10 (AC-5, D2 premise): the escape gate's output differs from its
    // raw input for a body containing markup meta-characters, so
    // rendering before vs. after the gate would yield different
    // diagnostic IDs for what is logically the same notification — this
    // pins *why* the dispatch-success site must capture pre-escape (the
    // capture point itself is a review check, per Test Notes, not proven
    // by this test alone). Unix-gated: `escape_body_markup` (the escape
    // gate under test here) is Unix-only, same as the module's other
    // escape-gate tests.
    #[cfg(unix)]
    #[test]
    fn raw_and_escape_gate_output_render_differently() {
        let raw_body = "<b>bold</b> & co";
        let escaped_body = escape_body_markup(raw_body);
        assert_ne!(raw_body, escaped_body.as_str());
        assert_ne!(
            redact_notification("t", raw_body),
            redact_notification("t", &escaped_body)
        );
    }
}

// ── task0001 (notification-worker-thread): submission queue + worker
// shutdown (IMPLEMENTATION.md D1/D2, task0001.md AC-1/AC-3/AC-4/AC-6). The
// submission component is exercised directly (`NotifyQueue`), never
// through notify-rust, per Test Notes / NFR4 — AC-3/AC-4 hold the
// receiver open without draining it so a full queue stays full. AC-1/AC-6
// exercise the real `NotifyRustSink` and assert on elapsed wall-clock
// time, deliberately generous relative to `NOTIFY_WORKER_JOIN_TIMEOUT` so
// CI variance never makes these flaky (Test Notes).
mod worker_thread {
    use super::*;

    // AC-3 (SPEC AC4 / FR5 / NFR3): capacity NOTIFY_QUEUE_CAPACITY fills
    // without a single drop; the very next submission is dropped, not
    // blocked, and does not grow the queue. The receiver is held but
    // never drained so the 9th item is genuinely the first to find the
    // queue full (Test Notes: draining would shift the drop boundary by
    // one and make the test flaky).
    #[test]
    fn queue_drops_the_ninth_submission_without_blocking_when_receiver_is_idle() {
        let (queue, _rx) = NotifyQueue::new(NOTIFY_QUEUE_CAPACITY);
        for i in 0..NOTIFY_QUEUE_CAPACITY {
            assert_eq!(
                queue.try_submit(&format!("t{i}"), "b"),
                SubmitOutcome::Submitted,
                "submission {i} should succeed while under capacity"
            );
        }
        assert_eq!(
            queue.try_submit("overflow-title", "overflow-body"),
            SubmitOutcome::DroppedEpisodeStart
        );
    }

    // AC-4 (SPEC AC5 / FR6, D1): the first drop of a saturation episode
    // warns, later drops in the same episode are silent, and a
    // successful submission ends the episode — the very next drop after
    // that starts a NEW episode and warns again. Judged purely from
    // `try_submit`'s return value (Test Notes), not log capture.
    #[test]
    fn saturation_episode_warns_exactly_once_then_rearms_after_a_successful_submission() {
        let (queue, rx) = NotifyQueue::new(NOTIFY_QUEUE_CAPACITY);
        for i in 0..NOTIFY_QUEUE_CAPACITY {
            assert_eq!(
                queue.try_submit(&format!("t{i}"), "b"),
                SubmitOutcome::Submitted
            );
        }

        // Saturation episode: first drop warns, later drops are silent.
        assert_eq!(
            queue.try_submit("drop-1", "b"),
            SubmitOutcome::DroppedEpisodeStart
        );
        assert_eq!(
            queue.try_submit("drop-2", "b"),
            SubmitOutcome::DroppedAlreadyWarned
        );
        assert_eq!(
            queue.try_submit("drop-3", "b"),
            SubmitOutcome::DroppedAlreadyWarned
        );

        // Draining one slot and submitting again ends the episode and
        // re-arms the warning (D1 step 4).
        rx.recv().expect("a queued item should be waiting");
        assert_eq!(
            queue.try_submit("after-drain", "b"),
            SubmitOutcome::Submitted
        );

        // The queue is full again (the drained slot was refilled) — the
        // very next drop starts a NEW episode and warns once more.
        assert_eq!(
            queue.try_submit("drop-again", "b"),
            SubmitOutcome::DroppedEpisodeStart
        );
    }

    // AC-4 (D1): "複数プロデューサが同時にドロップしても、エピソード先頭の
    // 警告はちょうど 1 件" — many threads racing to drop against the same
    // saturated queue still produce exactly one `DroppedEpisodeStart`,
    // because the armed-check-and-clear is a single atomic swap.
    #[test]
    fn concurrent_saturated_drops_produce_exactly_one_episode_start_warning() {
        let (queue, _rx) = NotifyQueue::new(NOTIFY_QUEUE_CAPACITY);
        for i in 0..NOTIFY_QUEUE_CAPACITY {
            assert_eq!(
                queue.try_submit(&format!("t{i}"), "b"),
                SubmitOutcome::Submitted
            );
        }

        let queue = Arc::new(queue);
        let threads: Vec<_> = (0..16)
            .map(|i| {
                let queue = queue.clone();
                std::thread::spawn(move || queue.try_submit(&format!("racer{i}"), "b"))
            })
            .collect();
        let outcomes: Vec<SubmitOutcome> = threads.into_iter().map(|t| t.join().unwrap()).collect();

        let warn_count = outcomes
            .iter()
            .filter(|o| **o == SubmitOutcome::DroppedEpisodeStart)
            .count();
        assert_eq!(
            warn_count, 1,
            "expected exactly one episode-start warning, got {warn_count}: {outcomes:?}"
        );
    }

    // AC-1 (SPEC AC1 / FR1 / NFR1): `NotificationSink::send` on the real
    // production sink only enqueues — it never performs the capability
    // query or the notify-rust dispatch itself, so a burst of sends well
    // past queue capacity returns to the caller almost instantly
    // regardless of how busy (or slow) the worker is. A synchronous
    // implementation performing D-Bus round-trips inline would take
    // orders of magnitude longer for the same burst.
    #[test]
    fn sink_send_never_blocks_the_caller_even_past_queue_capacity() {
        let sink = NotifyRustSink::new();
        let started = Instant::now();
        for i in 0..(NOTIFY_QUEUE_CAPACITY * 4) {
            sink.send(&format!("t{i}"), "b");
        }
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_millis(100),
            "send() appears to block the caller thread: {elapsed:?}"
        );
    }

    // AC-6 (SPEC AC8 / FR7 / FR8 / NFR2, D2): dropping the sink after
    // sending a few notifications disconnects the queue, the worker's
    // receive loop exits, and the bounded join returns well within a
    // generous CI-safe bound (deliberately far above
    // `NOTIFY_WORKER_JOIN_TIMEOUT` itself — Test Notes — so this never
    // flakes on a loaded CI box).
    #[test]
    fn dropping_the_sink_after_sending_returns_within_the_shutdown_deadline() {
        let sink = NotifyRustSink::new();
        for i in 0..3 {
            sink.send(&format!("t{i}"), "b");
        }
        let started = Instant::now();
        drop(sink);
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(2),
            "drop() exceeded the generous CI-safe shutdown bound: {elapsed:?}"
        );
    }

    // AC-6 (SPEC A9, D5): a sink dropped immediately after construction,
    // with no notification ever sent, also returns within the deadline —
    // this is exactly the path every `App`-constructing test exercises
    // when it tears down the production sink it never used.
    #[test]
    fn dropping_a_freshly_constructed_sink_with_no_notifications_returns_within_the_shutdown_deadline()
     {
        let sink = NotifyRustSink::new();
        let started = Instant::now();
        drop(sink);
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(2),
            "drop() exceeded the generous CI-safe shutdown bound: {elapsed:?}"
        );
    }
}
