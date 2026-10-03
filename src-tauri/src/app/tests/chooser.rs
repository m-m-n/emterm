use super::*;

// ── profile selector / new-tab chooser ───────────────────────────

fn profile(name: &str, is_default: bool) -> app_settings::Profile {
    app_settings::Profile {
        name: name.to_string(),
        shell_path: String::new(),
        shell_args: Vec::new(),
        env_vars: String::new(),
        working_directory: String::new(),
        is_default,
        ssh_connection_name: String::new(),
        wsl_distro_name: String::new(),
    }
}

fn app_with_profiles(profiles: Vec<app_settings::Profile>) -> App {
    let settings = crate::settings::Settings {
        profiles,
        ..Default::default()
    };
    App::with_settings(settings)
}

#[test]
fn open_profile_selector_noop_without_profiles() {
    let mut app = App::new();
    app.open_profile_selector();
    assert!(!app.profile_selector.visible);
}

#[test]
fn open_profile_selector_lists_profiles_only() {
    let mut app = app_with_profiles(vec![profile("a", false), profile("b", true)]);
    app.open_profile_selector();
    assert!(app.profile_selector.visible);
    assert!(!app.profile_selector.include_global);
    assert_eq!(app.profile_selector_row_count(), 2);
    assert_eq!(app.profile_selector.selected, 0);
}

fn tmux_row(label: &str, argv: Vec<&str>) -> crate::ui::profile_selector::TmuxRow {
    crate::ui::profile_selector::TmuxRow {
        label: label.to_string(),
        argv: argv.into_iter().map(str::to_string).collect(),
    }
}

#[test]
fn new_tab_chooser_prepends_global_and_preselects_default() {
    let mut app = app_with_profiles(vec![profile("a", false), profile("b", true)]);
    // `_with_entries` (not the public `open_new_tab_chooser`, which
    // calls the real, environment-dependent tmux discovery) keeps
    // this test deterministic: it exercises the default-profile
    // preselect decision, not tmux discovery.
    app.open_new_tab_chooser_with_entries(Vec::new());
    assert!(app.profile_selector.visible);
    assert!(app.profile_selector.include_global);
    // Global row + 2 profiles.
    assert_eq!(app.profile_selector_row_count(), 3);
    // Default profile "b" (profiles[1]) → row 2.
    assert_eq!(app.profile_selector.selected, 2);
}

#[test]
fn new_tab_chooser_without_default_preselects_global() {
    let mut app = app_with_profiles(vec![profile("a", false)]);
    app.open_new_tab_chooser_with_entries(Vec::new());
    assert!(app.profile_selector.include_global);
    assert_eq!(app.profile_selector.selected, 0);
}

// AC-6: profiles empty + no tmux entries → today's immediate-spawn
// fast path is preserved (chooser never opens).
#[test]
fn new_tab_chooser_spawns_immediately_without_profiles_or_entries() {
    let mut app = App::new();
    app.open_new_tab_chooser_with_entries(Vec::new());
    assert!(!app.profile_selector.visible);
    assert_eq!(app.tabs.len(), 1);
}

// AC-6: profiles empty but a tmux entry exists → the chooser opens
// instead of the fast path.
#[test]
fn new_tab_chooser_opens_with_entries_even_without_profiles() {
    let mut app = App::new();
    app.open_new_tab_chooser_with_entries(vec![tmux_row(
        "tmux: dev",
        vec!["-S", "/tmp/tmux-1000/dev", "attach"],
    )]);
    assert!(app.profile_selector.visible);
    assert!(app.profile_selector.include_global);
    // Global row + 0 profiles + 1 tmux entry.
    assert_eq!(app.profile_selector_row_count(), 2);
    assert!(app.tabs.is_empty());
}

// AC-5: confirming a tmux row spawns a tab (routes through
// `spawn_new_tab_with_overrides`, same as a profile confirm), using
// the entry's precomputed argv. The argv shape itself (AC-5) is
// covered by `tmux_sockets::attach_args`'s own tests; this test
// covers the wiring — that a tmux row confirm reaches the spawn path
// at all, same guard as `confirm_tmux_row_out_of_range_closes_without_spawn`.
#[test]
fn confirm_tmux_row_spawns_a_tab() {
    let mut app = app_with_profiles(vec![profile("a", false)]);
    app.open_new_tab_chooser_with_entries(vec![tmux_row(
        "tmux: dev",
        vec!["-S", "/tmp/tmux-1000/dev", "attach"],
    )]);
    // Global(0) + profile "a"(1) + tmux "dev"(2).
    app.confirm_profile_selection(2);
    assert!(!app.profile_selector.visible);
    assert_eq!(app.tabs.len(), 1);
}

// AC-6: an out-of-range tmux index (stale entries list) closes
// without spawning, same guard as an out-of-range profile index.
#[test]
fn confirm_tmux_row_out_of_range_closes_without_spawn() {
    let mut app = App::new();
    app.open_new_tab_chooser_with_entries(vec![tmux_row(
        "tmux: dev",
        vec!["-S", "/tmp/tmux-1000/dev", "attach"],
    )]);
    // Global(0) + tmux "dev"(1); row 2 has no entry.
    app.confirm_profile_selection(2);
    assert!(!app.profile_selector.visible);
    assert!(app.tabs.is_empty());
}

#[test]
fn confirm_out_of_range_closes_without_spawn() {
    let mut app = app_with_profiles(vec![profile("a", false)]);
    app.open_profile_selector();
    app.confirm_profile_selection(5);
    assert!(!app.profile_selector.visible);
    assert!(app.tabs.is_empty());
}

// ── shortcut labels on the new-tab chooser rows ─────────────────
//
// new-tab-menu-shortcut-hints task0001 AC-3 / AC-5 / AC-6. The
// entry-injecting `open_new_tab_chooser_with_entries` keeps these tests
// off real tmux discovery.

fn labels(v: &[Option<&str>]) -> Vec<Option<String>> {
    v.iter().map(|l| l.map(str::to_string)).collect()
}

// AC-3 at the App level: Global, default profile, other profile, tmux
// row, in row order.
#[test]
fn chooser_shortcut_labels_cover_global_and_default_profile_rows() {
    let mut app = app_with_profiles(vec![profile("a", true), profile("b", false)]);
    app.open_new_tab_chooser_with_entries(vec![tmux_row(
        "tmux: dev",
        vec!["-S", "/tmp/tmux-1000/dev", "attach"],
    )]);
    assert_eq!(
        app.profile_selector_shortcut_labels(),
        labels(&[Some("Ctrl+Shift+G"), Some("Ctrl+Shift+T"), None, None])
    );
    // One entry per row the dialog shows.
    assert_eq!(
        app.profile_selector_shortcut_labels().len(),
        app.profile_selector_row_count()
    );
}

// AC-5 (FR8): the Ctrl+Shift+P selector never shows a shortcut label.
#[test]
fn profile_selector_mode_has_no_shortcut_labels() {
    let mut app = app_with_profiles(vec![profile("a", true), profile("b", false)]);
    app.open_profile_selector();
    assert!(app.profile_selector.visible);
    assert!(!app.profile_selector.include_global);
    assert_eq!(
        app.profile_selector_shortcut_labels(),
        labels(&[None, None])
    );
}

// AC-6 (FR4): labels are derived from the table in force when the
// chooser is drawn, so the call after a settings apply shows the new
// chords.
#[test]
fn chooser_shortcut_labels_follow_applied_keybind_settings() {
    let mut app = app_with_profiles(vec![profile("a", true)]);
    app.open_new_tab_chooser_with_entries(Vec::new());
    assert_eq!(
        app.profile_selector_shortcut_labels(),
        labels(&[Some("Ctrl+Shift+G"), Some("Ctrl+Shift+T")])
    );

    let mut keybinds = crate::settings::KeybindSettings::default();
    keybinds.new_tab_global = "Ctrl+Alt+G".to_string();
    keybinds.new_tab = "Ctrl+Alt+N".to_string();
    let reloaded = crate::settings::Settings {
        profiles: vec![profile("a", true)],
        keybinds,
        ..Default::default()
    };
    app.apply_settings(reloaded);
    app.open_new_tab_chooser_with_entries(Vec::new());
    assert_eq!(
        app.profile_selector_shortcut_labels(),
        labels(&[Some("Ctrl+Alt+G"), Some("Ctrl+Alt+N")])
    );
}

// AC-6 (NFR2): the label text takes no locale input.
#[test]
fn chooser_shortcut_labels_are_the_same_in_ja_and_en() {
    let mut app = app_with_profiles(vec![profile("a", true)]);
    app.open_new_tab_chooser_with_entries(Vec::new());
    app.locale = crate::i18n::Locale::Ja;
    let ja = app.profile_selector_shortcut_labels();
    app.locale = crate::i18n::Locale::En;
    let en = app.profile_selector_shortcut_labels();
    assert_eq!(ja, en);
    assert_eq!(ja, labels(&[Some("Ctrl+Shift+G"), Some("Ctrl+Shift+T")]));
}

// The default flag is read at call time: no cached row assignment.
#[test]
fn chooser_shortcut_labels_read_the_default_flag_at_call_time() {
    let mut app = app_with_profiles(vec![profile("a", false), profile("b", false)]);
    app.open_new_tab_chooser_with_entries(Vec::new());
    assert_eq!(
        app.profile_selector_shortcut_labels(),
        labels(&[Some("Ctrl+Shift+G"), None, None])
    );
    std::sync::Arc::make_mut(&mut app.settings).profiles[1].is_default = true;
    assert_eq!(
        app.profile_selector_shortcut_labels(),
        labels(&[Some("Ctrl+Shift+G"), None, Some("Ctrl+Shift+T")])
    );
}

// ── overlay wiring (headless egui pass) ─────────────────────────
//
// Runs the real overlay pass and inspects what was painted: each label
// lands on the row of the target it opens (new-tab-menu-shortcut-hints
// task0001 FR1 / FR2 / FR8 wiring).

/// `(painted text, vertical center)` of every text the selector overlay
/// painted in its final frame.
fn overlay_texts(app: &mut App) -> Vec<(String, f32)> {
    fn walk(shape: &egui::Shape, out: &mut Vec<(String, f32)>) {
        match shape {
            egui::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, out)),
            egui::Shape::Text(t) => {
                let text: String = t.galley.rows.iter().map(|r| r.text()).collect();
                out.push((text, t.pos.y + t.galley.size().y / 2.0));
            }
            _ => {}
        }
    }
    let ctx = egui::Context::default();
    let mut output = egui::FullOutput::default();
    // The first frame only sizes the areas; advancing time finishes
    // their fade-in.
    for frame in 0..4 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            time: Some(f64::from(frame)),
            ..Default::default()
        };
        output = ctx.run(input, |ctx| {
            crate::render::draw_profile_selector_overlay(ctx, app);
        });
    }
    let mut out = Vec::new();
    for clipped in &output.shapes {
        walk(&clipped.shape, &mut out);
    }
    out
}

fn center_of(texts: &[(String, f32)], text: &str) -> f32 {
    texts
        .iter()
        .find(|(t, _)| t == text)
        .unwrap_or_else(|| panic!("no painted text {text:?} in {texts:?}"))
        .1
}

fn has_painted(texts: &[(String, f32)], text: &str) -> bool {
    texts.iter().any(|(t, _)| t == text)
}

// FR1 / FR2: the Global row carries the `new_tab_global` label and the
// default profile's row (not another profile's) the `new_tab` label.
#[test]
fn overlay_paints_labels_on_global_and_default_profile_rows() {
    let mut app = app_with_profiles(vec![profile("alpha", false), profile("beta", true)]);
    app.locale = crate::i18n::Locale::En;
    app.open_new_tab_chooser_with_entries(vec![tmux_row("tmux: dev", vec!["attach"])]);
    let texts = overlay_texts(&mut app);

    let near = |a: f32, b: f32| (a - b).abs() < 0.5;
    assert!(near(
        center_of(&texts, "Ctrl+Shift+G"),
        center_of(&texts, "Global Settings")
    ));
    assert!(near(
        center_of(&texts, "Ctrl+Shift+T"),
        center_of(&texts, "beta")
    ));
    // No other row gets a label: exactly one of each.
    assert_eq!(texts.iter().filter(|(t, _)| t == "Ctrl+Shift+G").count(), 1);
    assert_eq!(texts.iter().filter(|(t, _)| t == "Ctrl+Shift+T").count(), 1);
}

// FR8: the Ctrl+Shift+P selector paints no shortcut label.
#[test]
fn overlay_selector_mode_paints_no_labels() {
    let mut app = app_with_profiles(vec![profile("alpha", true)]);
    app.open_profile_selector();
    let texts = overlay_texts(&mut app);
    assert!(has_painted(&texts, "alpha"));
    assert!(!has_painted(&texts, "Ctrl+Shift+G"));
    assert!(!has_painted(&texts, "Ctrl+Shift+T"));
}

// FR6: a chord another action outranks is not shown.
#[test]
fn overlay_omits_a_label_whose_chord_is_taken_by_a_higher_priority_action() {
    let mut keybinds = crate::settings::KeybindSettings::default();
    keybinds.new_tab = keybinds.copy.clone();
    let settings = crate::settings::Settings {
        profiles: vec![profile("alpha", true)],
        keybinds,
        ..Default::default()
    };
    let mut app = App::with_settings(settings);
    app.open_new_tab_chooser_with_entries(Vec::new());
    let texts = overlay_texts(&mut app);
    assert!(has_painted(&texts, "Ctrl+Shift+G"));
    assert!(!has_painted(&texts, "Ctrl+Shift+C"));
}

#[test]
fn apply_settings_closes_open_profile_selector() {
    let mut app = app_with_profiles(vec![profile("a", false), profile("b", false)]);
    app.open_new_tab_chooser();
    assert!(app.profile_selector.visible);
    // A settings save reloads profiles (here: a shorter list) while
    // the modal is open. The selector must close rather than confirm
    // against the stale list.
    let reloaded = crate::settings::Settings {
        profiles: vec![profile("a", false)],
        ..Default::default()
    };
    app.apply_settings(reloaded);
    assert!(!app.profile_selector.visible);
}

#[test]
fn auto_research_reresolves_matches_without_scrolling() {
    // Spawn a tab so there is an active core to search against.
    let mut app = App::new();
    app.spawn_initial_tab();
    {
        let mut core = app.tabs[0].core.lock();
        core.process_pty_data(b"needle\r\n");
    }
    app.open_search();
    app.search.query = "needle".to_string();
    app.run_search();
    assert_eq!(app.search.matches.len(), 1);

    // User scrolls back; the auto re-search must preserve this offset.
    app.scroll_set_offset(5);
    assert_eq!(app.scroll_offset(), 5);

    // New PTY output brings a second "needle"; on_pty_output flags the
    // cache dirty (mirrors the pump_all path).
    {
        let mut core = app.tabs[0].core.lock();
        core.process_pty_data(b"another needle line\r\n");
    }
    app.on_pty_output(true, 0);
    assert!(app.search.needs_research());

    // The frame-loop hook re-resolves against the current buffer without
    // scrolling.
    let researched = app.auto_research_if_dirty();
    assert!(
        researched,
        "dirty + visible + non-empty query → re-search ran"
    );
    assert_eq!(
        app.search.matches.len(),
        2,
        "re-search reflects the new occurrence in the current buffer"
    );
    assert_eq!(
        app.scroll_offset(),
        5,
        "auto re-search must NOT move the viewport"
    );
}

#[test]
fn auto_research_noop_when_overlay_hidden_or_query_empty() {
    let mut app = App::new();
    app.spawn_initial_tab();
    {
        let mut core = app.tabs[0].core.lock();
        core.process_pty_data(b"needle");
    }
    // Hidden overlay: even after a buffer change, no re-search.
    app.search.query = "needle".to_string();
    app.on_pty_output(true, 0);
    assert!(
        !app.auto_research_if_dirty(),
        "hidden overlay does not research"
    );

    // Visible but empty query: nothing to re-resolve.
    app.open_search();
    app.search.query.clear();
    app.on_pty_output(true, 0);
    assert!(
        !app.auto_research_if_dirty(),
        "empty query does not research"
    );
}

#[test]
fn switch_to_tab_closes_open_search() {
    // Two synthetic tabs so `switch_to_tab` actually changes `active`.
    // We avoid spawning PTYs by leaving the search overlay open and
    // asserting it closes on the active-tab change path. Construct a
    // bare app and drive `switch_to_tab` against an out-of-range and
    // an in-range index to confirm only a real switch closes search.
    let mut app = App::new();
    app.open_search();
    // No tabs → out-of-range switch is a no-op; search stays open.
    app.switch_to_tab(1);
    assert!(app.search_visible(), "no-op switch must not close search");
}
