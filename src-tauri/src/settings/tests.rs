use super::*;

#[test]
fn mux_action_names_match_prefix_ssot() {
    // `MUX_ACTION_NAMES` is a second in-Rust list of the mux action names
    // whose authority is `crate::mux::prefix::DEFAULT_ACTION_BINDINGS`.
    // Assert they stay identical (same names, same order) so adding/removing
    // an action in the SSOT without updating this list fails CI instead of
    // silently dropping a default-seed.
    let ssot: Vec<&str> = crate::mux::prefix::DEFAULT_ACTION_BINDINGS
        .iter()
        .map(|(name, _)| *name)
        .collect();
    assert_eq!(
        MUX_ACTION_NAMES.as_slice(),
        ssot.as_slice(),
        "MUX_ACTION_NAMES drifted from prefix::DEFAULT_ACTION_BINDINGS"
    );
}

#[test]
fn default_scrollback_lines_is_ten_thousand() {
    let s = Settings::new();
    assert_eq!(s.scrollback_lines, 10_000);
}

#[test]
fn default_image_memory_quota_is_320_mb() {
    let s = Settings::new();
    assert_eq!(s.image_memory_quota_mb, 320);
}

#[test]
fn default_clipboard_read_osc52_is_true() {
    let s = Settings::new();
    assert!(s.clipboard_read_osc52);
}

#[test]
fn default_clipboard_max_size_osc52_is_10_mib() {
    let s = Settings::new();
    assert_eq!(s.clipboard_max_size_osc52, 10 * 1024 * 1024);
}

// ── TS-settings-1: mux.prefix_key default ────────────────────────────

#[test]
fn default_mux_prefix_key_is_ctrl_z() {
    let s = Settings::new();
    assert_eq!(s.mux_prefix_key, "Ctrl+Z");
}

#[test]
fn default_mux_prefix_key_parses_to_default_chord() {
    // Cross-check: the default spec must parse cleanly under the
    // prefix-key parser introduced in Phase 4-C.
    let s = Settings::new();
    let chord =
        crate::mux::prefix::parse_prefix_key(&s.mux_prefix_key).expect("parse default chord");
    assert_eq!(chord, crate::mux::prefix::PrefixChord::default());
}

// ── TS-settings-1: statusbar defaults + position fallback ───────────

#[test]
fn default_statusbar_is_enabled() {
    let s = Settings::new();
    assert!(
        s.statusbar.enabled,
        "statusbar.enabled must default to true"
    );
}

#[test]
fn statusbar_settings_default_round_trip() {
    // The Default impl on StatusBarSettings must match the value seeded
    // into the parent Settings struct so callers can compare safely.
    let s = Settings::new();
    assert_eq!(s.statusbar, StatusBarSettings::default());
}

// ── TS-20: status-bar settings extension defaults ─────────────────

#[test]
fn default_status_bar_app_line1_templates() {
    let s = Settings::new();
    assert_eq!(s.statusbar.app_line1_left, "{time}");
    assert_eq!(s.statusbar.app_line1_right, "{cwd}");
}

#[test]
fn default_status_bar_app_line2_templates_are_empty() {
    let s = Settings::new();
    assert!(s.statusbar.app_line2_left.is_empty());
    assert!(s.statusbar.app_line2_right.is_empty());
}

#[test]
fn default_status_bar_time_format_is_hhmmss() {
    let s = Settings::new();
    assert_eq!(s.statusbar.time_format, "HH:mm:ss");
}

#[test]
fn default_status_bar_font_size_is_none() {
    let s = Settings::new();
    assert!(s.statusbar.font_size.is_none());
}

#[test]
fn default_status_bar_custom_commands_is_empty() {
    let s = Settings::new();
    assert!(s.statusbar.custom_commands.is_empty());
}

#[test]
fn default_status_bar_refresh_rates_is_empty() {
    let s = Settings::new();
    assert!(s.statusbar.refresh_rates.is_empty());
}

#[test]
fn custom_command_default_interval_is_1000ms() {
    let c = CustomCommand::default();
    assert_eq!(c.interval_ms, 1000);
    assert!(c.executable.is_empty());
}

// ── TS-settings-1: ime.native_integration defaults to true ─────

/// `Settings::default().ime.native_integration` must default to `true`.
/// Phase 7 (JSON loader) will rely on this default when a settings file
/// omits the `ime` block or the `native_integration` key. Pinning the
/// shape here keeps the Phase 4-G factory's "opt-out only" contract.
#[test]
fn default_ime_native_integration_is_true() {
    let s = Settings::new();
    assert!(s.ime.native_integration);
}

#[test]
fn ime_settings_default_round_trip() {
    let s = Settings::new();
    assert_eq!(s.ime, ImeSettings::default());
}

#[test]
fn ime_settings_default_is_native_integration_true() {
    let ime = ImeSettings::default();
    assert!(ime.native_integration);
}

// ── font-swash-migration: FontEngine + font-related Settings ────────

/// TS-font-1: `FontEngine::default()` is `Swash`.
#[test]
fn font_engine_default_is_swash() {
    assert_eq!(FontEngine::default(), FontEngine::Swash);
}

/// TS-font-2: parse `"ab_glyph"` succeeds; unknown values warn-log
/// and fall back to Swash.
#[test]
fn font_engine_parses_known_values() {
    assert_eq!(FontEngine::parse_or_warn("swash"), FontEngine::Swash);
    assert_eq!(FontEngine::parse_or_warn("ab_glyph"), FontEngine::AbGlyph);
    assert_eq!(FontEngine::parse_or_warn("AbGlyph"), FontEngine::AbGlyph);
    assert_eq!(FontEngine::parse_or_warn("  swash  "), FontEngine::Swash);
}

#[test]
fn font_engine_unknown_falls_back_to_swash() {
    assert_eq!(FontEngine::parse_or_warn("blink"), FontEngine::Swash);
    assert_eq!(FontEngine::parse_or_warn(""), FontEngine::Swash);
}

/// FR9 / Settings schema additions: the new font-related fields exist
/// on Settings and carry sensible defaults.
#[test]
fn settings_carry_font_engine_default_swash() {
    let s = Settings::new();
    assert_eq!(s.font_engine, FontEngine::Swash);
}

#[test]
fn settings_font_family_fallback_default_empty() {
    let s = Settings::new();
    assert!(s.font_family_fallback.is_empty());
}

#[test]
fn settings_variable_font_axes_default_empty() {
    let s = Settings::new();
    assert!(s.variable_font_axes.is_empty());
}

// ── settings.json loader (Phase 7) ─────────────────────────────────

fn load_json(s: &str) -> Settings {
    let raw: RawSettings = serde_json::from_str(s).expect("parse RawSettings");
    let mut base = Settings::default();
    raw.merge_into(&mut base);
    base
}

#[test]
fn loader_empty_object_keeps_all_defaults() {
    let s = load_json("{}");
    let d = Settings::default();
    assert_eq!(s.scrollback_lines, d.scrollback_lines);
    assert_eq!(s.image_memory_quota_mb, d.image_memory_quota_mb);
    assert_eq!(s.clipboard_read_osc52, d.clipboard_read_osc52);
    assert_eq!(s.clipboard_max_size_osc52, d.clipboard_max_size_osc52);
    assert_eq!(s.mux_prefix_key, d.mux_prefix_key);
    assert_eq!(s.ambiguous_width_mode, d.ambiguous_width_mode);
    assert_eq!(s.font_engine, d.font_engine);
    assert_eq!(s.statusbar, d.statusbar);
    assert_eq!(s.ime, d.ime);
}

#[test]
fn loader_unknown_keys_are_ignored() {
    // Forward compat: src-tauri may add keys native-poc does not yet
    // consume. They must not break the loader.
    let s = load_json(
        r#"{"some_future_key": 42, "another": {"nested": true}, "scrollback_lines": 1234}"#,
    );
    assert_eq!(s.scrollback_lines, 1234);
}

#[test]
fn loader_explicit_null_falls_back_to_default() {
    let s = load_json(r#"{"scrollback_lines": null, "clipboard_read_osc52": null}"#);
    assert_eq!(s.scrollback_lines, DEFAULT_SCROLLBACK_LINES);
    assert!(s.clipboard_read_osc52);
}

#[test]
fn loader_flat_keys_are_applied() {
    let s = load_json(
        r#"{
            "scrollback_lines": 50000,
            "clipboard_read_osc52": false,
            "clipboard_max_size_osc52": 1024
        }"#,
    );
    assert_eq!(s.scrollback_lines, 50_000);
    assert!(!s.clipboard_read_osc52);
    assert_eq!(s.clipboard_max_size_osc52, 1024);
}

#[test]
fn loader_mux_prefix_overrides_default() {
    let s = load_json(r#"{"mux": {"prefix": "Ctrl+A"}}"#);
    assert_eq!(s.mux_prefix_key, "Ctrl+A");
}

// ── TS-4: mux settings loader (tab_always_expand / keybinds) ─────────

#[test]
fn default_mux_settings_match_webview() {
    let s = Settings::new();
    assert!(!s.mux.tab_always_expand);
    // `Ctrl`-modified default action chords.
    let chord = |spec: &str| parse_mux_action_chord(spec).unwrap();
    assert_eq!(s.mux.keybinds.get("detach"), Some(&chord("Ctrl+D")));
    assert_eq!(s.mux.keybinds.get("new-window"), Some(&chord("Ctrl+C")));
    assert_eq!(s.mux.keybinds.get("next-window"), Some(&chord("Ctrl+N")));
    assert_eq!(s.mux.keybinds.get("prev-window"), Some(&chord("Ctrl+P")));
    assert_eq!(s.mux.keybinds.get("rename-window"), Some(&chord("Ctrl+R")));
    assert_eq!(s.mux.keybinds.get("move-window"), Some(&chord("Ctrl+T")));
    // mux-agent-tab-cycle task0001 AC-1: default present when unset.
    assert_eq!(
        s.mux.keybinds.get("next-agent-window"),
        Some(&chord("Ctrl+A"))
    );
}

/// mux-agent-tab-cycle task0001 AC-1: a user override of
/// `settings.mux.keybinds["next-agent-window"]` wins over the Ctrl+A
/// default.
#[test]
fn loader_mux_keybinds_next_agent_window_override() {
    let s = load_json(r#"{"mux": {"keybinds": {"next-agent-window": "g"}}}"#);
    let chord = |spec: &str| parse_mux_action_chord(spec).unwrap();
    assert_eq!(s.mux.keybinds.get("next-agent-window"), Some(&chord("g")));
    // Untouched actions keep their defaults.
    assert_eq!(s.mux.keybinds.get("next-window"), Some(&chord("Ctrl+N")));
}

#[test]
fn loader_mux_tab_always_expand() {
    let s = load_json(r#"{"mux": {"tab_always_expand": true}}"#);
    assert!(s.mux.tab_always_expand);
}

// ── window_sidebar_overlay (task0001 AC-1/AC-2/AC-3/AC-4) ────────────

#[test]
fn loader_mux_window_sidebar_overlay_missing_defaults_overlay() {
    // AC-1: a settings JSON without the field resolves to the overlay
    // display mode (`true`).
    let s = load_json(r#"{"mux": {"prefix": "Ctrl+A"}}"#);
    assert!(s.mux.window_sidebar_overlay);
}

#[test]
fn loader_mux_window_sidebar_overlay_null_resolves_overlay() {
    // AC-2: a settings JSON with the field `null` resolves to the
    // overlay display mode (the loader treats null as "not
    // specified", matching the missing-key case).
    let s = load_json(r#"{"mux": {"window_sidebar_overlay": null}}"#);
    assert!(s.mux.window_sidebar_overlay);
}

#[test]
fn loader_mux_window_sidebar_overlay_explicit_false_is_persistent() {
    // AC-3: an explicit `false` resolves to the persistent display
    // mode in the runtime settings the GUI reads, overriding the
    // overlay default (compatibility guarantee: a saved persistent
    // choice is never changed under the user).
    let s = load_json(r#"{"mux": {"window_sidebar_overlay": false}}"#);
    assert!(!s.mux.window_sidebar_overlay);
}

#[test]
fn loader_mux_window_sidebar_overlay_true() {
    // AC-4: `true` resolves to `true` in the runtime settings the GUI
    // reads.
    let s = load_json(r#"{"mux": {"window_sidebar_overlay": true}}"#);
    assert!(s.mux.window_sidebar_overlay);
}

#[test]
fn loader_mux_keybinds_override_valid() {
    let s = load_json(r#"{"mux": {"keybinds": {"next-window": "j", "prev-window": "k"}}}"#);
    let chord = |spec: &str| parse_mux_action_chord(spec).unwrap();
    assert_eq!(s.mux.keybinds.get("next-window"), Some(&chord("j")));
    assert_eq!(s.mux.keybinds.get("prev-window"), Some(&chord("k")));
    // Untouched actions keep their defaults.
    assert_eq!(s.mux.keybinds.get("new-window"), Some(&chord("Ctrl+C")));
}

#[test]
fn loader_mux_keybinds_modifier_chord_accepted() {
    // Regression: modifier-bearing chords (`Ctrl+D`, `Alt+M`, …) are now
    // first-class follow-ups, matching the WebView's
    // `matchActionBinding`. tmux.conf import writes these back when the
    // user binds, e.g., `bind C-d detach-client`.
    let s = load_json(r#"{"mux": {"keybinds": {"detach": "Ctrl+D", "next-window": "Alt+N"}}}"#);
    let ctrl_d = parse_mux_action_chord("Ctrl+D").unwrap();
    let alt_n = parse_mux_action_chord("Alt+N").unwrap();
    assert_eq!(s.mux.keybinds.get("detach"), Some(&ctrl_d));
    assert_eq!(s.mux.keybinds.get("next-window"), Some(&alt_n));
}

#[test]
fn loader_mux_keybinds_unparseable_keeps_default() {
    // Garbage spec is still rejected and keeps the default chord.
    let s = load_json(r#"{"mux": {"keybinds": {"next-window": "+++"}}}"#);
    let default_n = parse_mux_action_chord("Ctrl+N").unwrap();
    assert_eq!(s.mux.keybinds.get("next-window"), Some(&default_n));
}

#[test]
fn loader_mux_keybinds_empty_keeps_default() {
    let s = load_json(r#"{"mux": {"keybinds": {"next-window": ""}}}"#);
    let default_n = parse_mux_action_chord("Ctrl+N").unwrap();
    assert_eq!(s.mux.keybinds.get("next-window"), Some(&default_n));
}

#[test]
fn loader_mux_keybinds_unknown_action_ignored() {
    let s = load_json(r#"{"mux": {"keybinds": {"frobnicate": "z"}}}"#);
    assert!(!s.mux.keybinds.contains_key("frobnicate"));
}

#[test]
fn loader_mux_keybinds_legacy_actions_dropped_silently() {
    // Pre-cleanup `mux.keybinds` entries that the WebView build still
    // emits must be dropped (no map entry) without a `warn!` storm.
    // We can't easily assert "no warn fired" from here, but the
    // dropped-from-the-map invariant is the user-observable half.
    let s = load_json(
        r#"{
            "mux": {
                "keybinds": {
                    "next-pane": "o",
                    "copy-mode": "Ctrl+[",
                    "paste": "Ctrl+]"
                }
            }
        }"#,
    );
    assert!(!s.mux.keybinds.contains_key("next-pane"));
    assert!(!s.mux.keybinds.contains_key("copy-mode"));
    assert!(!s.mux.keybinds.contains_key("paste"));
}

/// FR4/FR8b (mux-status-bar-removal task0001, TS3): a `settings.json`
/// written by an older eMterm build may still contain the retired
/// `mux.statusbar` object (the removed mux status-bar settings
/// schema). Loading it must not fail -- `RawMux` no longer names the
/// key, so it is silently ignored, exactly like any other
/// unrecognized JSON field.
#[test]
fn loader_tolerates_stale_mux_statusbar_key() {
    let s = load_json(
        r#"{
            "mux": {
                "prefix": "Ctrl+A",
                "statusbar": {
                    "enabled": true,
                    "left": "L",
                    "right": "R",
                    "commands": {
                        "branch": {"executable": "/usr/bin/git-branch", "interval_ms": 7500}
                    }
                }
            }
        }"#,
    );
    assert_eq!(s.mux_prefix_key, "Ctrl+A");
}

// ── Notification settings ───────────────────────────────────────────

#[test]
fn default_notification_settings_match_webview_build() {
    // Mirrors src-tauri's AppSettings defaults: everything on except
    // notify_on_output (opt-in, mirrors the WebView build).
    let s = Settings::new();
    assert!(s.notification_enabled);
    assert!(s.tab_activity_indicator);
    assert!(s.notify_on_process_exit);
    assert!(!s.notify_on_output);
    assert!(s.notify_on_bell);
}

#[test]
fn loader_notification_flat_keys_are_applied() {
    let s = load_json(
        r#"{
            "notification_enabled": false,
            "tab_activity_indicator": false,
            "notify_on_process_exit": false,
            "notify_on_output": true,
            "notify_on_bell": false
        }"#,
    );
    assert!(!s.notification_enabled);
    assert!(!s.tab_activity_indicator);
    assert!(!s.notify_on_process_exit);
    assert!(s.notify_on_output);
    assert!(!s.notify_on_bell);
}

#[test]
fn loader_notification_null_keys_keep_defaults() {
    let s = load_json(
        r#"{
            "notification_enabled": null,
            "tab_activity_indicator": null,
            "notify_on_process_exit": null,
            "notify_on_output": null,
            "notify_on_bell": null
        }"#,
    );
    let d = Settings::default();
    assert_eq!(s.notification_enabled, d.notification_enabled);
    assert_eq!(s.tab_activity_indicator, d.tab_activity_indicator);
    assert_eq!(s.notify_on_process_exit, d.notify_on_process_exit);
    assert_eq!(s.notify_on_output, d.notify_on_output);
    assert_eq!(s.notify_on_bell, d.notify_on_bell);
}

// ── agent_status_notifications (task0007 AC-5) ───────────────────────

#[test]
fn default_agent_status_notifications_is_true() {
    assert!(Settings::new().agent_status_notifications);
}

#[test]
fn loader_agent_status_notifications_flat_key_is_applied() {
    let s = load_json(r#"{"agent_status_notifications": false}"#);
    assert!(!s.agent_status_notifications);
}

#[test]
fn loader_agent_status_notifications_null_keeps_default() {
    let s = load_json(r#"{"agent_status_notifications": null}"#);
    assert_eq!(
        s.agent_status_notifications,
        Settings::default().agent_status_notifications
    );
}

// ── agent_notify_on_done / agent_notify_on_blocked (task0001 AC-1) ──

#[test]
fn default_agent_notify_on_done_is_true() {
    assert!(Settings::new().agent_notify_on_done);
}

#[test]
fn loader_agent_notify_on_done_flat_key_is_applied() {
    let s = load_json(r#"{"agent_notify_on_done": false}"#);
    assert!(!s.agent_notify_on_done);
}

#[test]
fn loader_agent_notify_on_done_null_keeps_default() {
    let s = load_json(r#"{"agent_notify_on_done": null}"#);
    assert_eq!(
        s.agent_notify_on_done,
        Settings::default().agent_notify_on_done
    );
}

#[test]
fn default_agent_notify_on_blocked_is_true() {
    assert!(Settings::new().agent_notify_on_blocked);
}

#[test]
fn loader_agent_notify_on_blocked_flat_key_is_applied() {
    let s = load_json(r#"{"agent_notify_on_blocked": false}"#);
    assert!(!s.agent_notify_on_blocked);
}

#[test]
fn loader_agent_notify_on_blocked_null_keeps_default() {
    let s = load_json(r#"{"agent_notify_on_blocked": null}"#);
    assert_eq!(
        s.agent_notify_on_blocked,
        Settings::default().agent_notify_on_blocked
    );
}

// ── language / log recording / skk_mode ─────────────────────────

#[test]
fn default_language_is_auto() {
    assert_eq!(Settings::new().language, Language::Auto);
}

#[test]
fn default_log_recording_is_disabled() {
    assert!(!Settings::new().log_recording_enabled);
}

#[test]
fn default_skk_mode_is_enabled() {
    assert!(Settings::new().skk_mode);
}

#[test]
fn loader_language_log_recording_skk_mode_flat_keys_are_applied() {
    let s = load_json(
        r#"{
            "language": "ja",
            "log_recording_enabled": true,
            "skk_mode": false
        }"#,
    );
    assert_eq!(s.language, Language::Ja);
    assert!(s.log_recording_enabled);
    assert!(!s.skk_mode);
}

#[test]
fn loader_language_parses_all_supported_values() {
    assert_eq!(
        load_json(r#"{"language": "auto"}"#).language,
        Language::Auto
    );
    assert_eq!(load_json(r#"{"language": "en"}"#).language, Language::En);
    assert_eq!(load_json(r#"{"language": "ja"}"#).language, Language::Ja);
    // Unknown values warn and fall back to auto.
    assert_eq!(load_json(r#"{"language": "fr"}"#).language, Language::Auto);
}

#[test]
fn loader_language_log_recording_skk_mode_null_keys_keep_defaults() {
    let s = load_json(
        r#"{
            "language": null,
            "log_recording_enabled": null,
            "skk_mode": null
        }"#,
    );
    let d = Settings::default();
    assert_eq!(s.language, d.language);
    assert_eq!(s.log_recording_enabled, d.log_recording_enabled);
    assert_eq!(s.skk_mode, d.skk_mode);
}

#[test]
fn loader_empty_mux_prefix_keeps_default() {
    let s = load_json(r#"{"mux": {"prefix": ""}}"#);
    assert_eq!(s.mux_prefix_key, DEFAULT_MUX_PREFIX_KEY);
}

#[test]
fn loader_flat_statusbar_keys_map_to_nested() {
    let s = load_json(
        r#"{
            "statusbar_enabled": false,
            "statusbar_app_line1_left": "{hostname}",
            "statusbar_app_line1_right": "{git_branch}",
            "statusbar_app_line2_left": "L2L",
            "statusbar_app_line2_right": "L2R",
            "statusbar_time_format": "HH:mm",
            "statusbar_font_size": 18.5,
            "statusbar_refresh_rates": {"time": 2000, "git_branch": 10000}
        }"#,
    );
    assert!(!s.statusbar.enabled);
    assert_eq!(s.statusbar.app_line1_left, "{hostname}");
    assert_eq!(s.statusbar.app_line1_right, "{git_branch}");
    assert_eq!(s.statusbar.app_line2_left, "L2L");
    assert_eq!(s.statusbar.app_line2_right, "L2R");
    assert_eq!(s.statusbar.time_format, "HH:mm");
    assert_eq!(s.statusbar.font_size, Some(18.5));
    assert_eq!(s.statusbar.refresh_rates.get("time"), Some(&2000));
    assert_eq!(s.statusbar.refresh_rates.get("git_branch"), Some(&10000));
}

#[test]
fn loader_statusbar_custom_commands_default_interval_when_omitted() {
    let s = load_json(
        r#"{
            "statusbar_custom_commands": {
                "weather": {"executable": "/usr/bin/curl"}
            }
        }"#,
    );
    let c = s.statusbar.custom_commands.get("weather").unwrap();
    assert_eq!(c.executable, "/usr/bin/curl");
    assert_eq!(c.interval_ms, 1000);
}

#[test]
fn loader_statusbar_custom_commands_explicit_interval_kept() {
    let s = load_json(
        r#"{
            "statusbar_custom_commands": {
                "weather": {"executable": "x", "interval_ms": 30000}
            }
        }"#,
    );
    assert_eq!(
        s.statusbar
            .custom_commands
            .get("weather")
            .unwrap()
            .interval_ms,
        30_000
    );
}

#[test]
fn loader_font_family_primary_secondary_populate_fallback() {
    let s = load_json(
        r#"{"font_family_primary": "JetBrains Mono", "font_family_secondary": "Noto Sans JP"}"#,
    );
    assert_eq!(
        s.font_family_fallback,
        vec!["JetBrains Mono".to_string(), "Noto Sans JP".to_string()]
    );
}

#[test]
fn loader_blank_font_family_strings_are_dropped() {
    let s = load_json(r#"{"font_family_primary": "  ", "font_family_secondary": ""}"#);
    // Blank entries must not be pushed; the field stays empty
    // (matching Settings::default()).
    assert!(s.font_family_fallback.is_empty());
}

#[test]
fn loader_native_poc_font_engine_overrides() {
    let s = load_json(r#"{"native_poc": {"font_engine": "ab_glyph"}}"#);
    assert_eq!(s.font_engine, FontEngine::AbGlyph);
}

#[test]
fn loader_native_poc_ambiguous_width_wide() {
    let s = load_json(r#"{"native_poc": {"ambiguous_width_mode": "wide"}}"#);
    assert_eq!(s.ambiguous_width_mode, AmbiguousWidthMode::Wide);
}

#[test]
fn loader_native_poc_ambiguous_width_unknown_falls_back_to_narrow() {
    let s = load_json(r#"{"native_poc": {"ambiguous_width_mode": "huge"}}"#);
    assert_eq!(s.ambiguous_width_mode, AmbiguousWidthMode::Narrow);
}

#[test]
fn loader_native_poc_image_memory_quota_overrides() {
    let s = load_json(r#"{"native_poc": {"image_memory_quota_mb": 128}}"#);
    assert_eq!(s.image_memory_quota_mb, 128);
}

#[test]
fn loader_native_poc_ime_native_integration_overrides() {
    let s = load_json(r#"{"native_poc": {"ime": {"native_integration": false}}}"#);
    assert!(!s.ime.native_integration);
}

#[test]
fn loader_native_poc_font_family_fallback_overrides_flat_keys() {
    let s = load_json(
        r#"{
            "font_family_primary": "A",
            "native_poc": {"font_family_fallback": ["X", "Y", "Z"]}
        }"#,
    );
    assert_eq!(
        s.font_family_fallback,
        vec!["X".to_string(), "Y".to_string(), "Z".to_string()]
    );
}

#[test]
fn loader_native_poc_variable_font_axes_overrides() {
    let s = load_json(r#"{"native_poc": {"variable_font_axes": {"wght": 700.0}}}"#);
    assert_eq!(s.variable_font_axes.get("wght").copied(), Some(700.0));
}

#[test]
fn load_from_missing_file_returns_defaults() {
    let p = std::path::PathBuf::from("/tmp/__nonexistent_emterm_settings_xyz_998877.json");
    // Defensive: ensure the path really does not exist.
    let _ = std::fs::remove_file(&p);
    let s = Settings::load_from(&p);
    // Spot-check a couple of fields against Default.
    assert_eq!(s.scrollback_lines, DEFAULT_SCROLLBACK_LINES);
    assert_eq!(s.mux_prefix_key, DEFAULT_MUX_PREFIX_KEY);
}

#[test]
fn load_from_invalid_json_returns_defaults() {
    let dir = std::env::temp_dir();
    let p = dir.join(format!(
        "emterm_settings_invalid_{}.json",
        std::process::id()
    ));
    std::fs::write(&p, b"{ not json").expect("write tmp settings");
    let s = Settings::load_from(&p);
    let _ = std::fs::remove_file(&p);
    assert_eq!(s.scrollback_lines, DEFAULT_SCROLLBACK_LINES);
}

// ── font_size / padding / cursor_style / cursor_blink loader ──────

#[test]
fn default_font_size_is_13() {
    let s = Settings::new();
    assert!((s.font_size - DEFAULT_FONT_SIZE_PT).abs() < f32::EPSILON);
}

#[test]
fn default_padding_is_4() {
    let s = Settings::new();
    assert_eq!(s.padding, DEFAULT_PADDING_PX);
}

#[test]
fn default_cursor_style_is_block() {
    let s = Settings::new();
    assert_eq!(s.cursor_style, CursorStyle::Block);
}

#[test]
fn default_cursor_blink_is_true() {
    let s = Settings::new();
    assert!(s.cursor_blink);
}

#[test]
fn cursor_style_parses_known_values() {
    assert_eq!(CursorStyle::parse_or_warn("block"), CursorStyle::Block);
    assert_eq!(
        CursorStyle::parse_or_warn("Underline"),
        CursorStyle::Underline
    );
    assert_eq!(CursorStyle::parse_or_warn("BAR"), CursorStyle::Bar);
    assert_eq!(CursorStyle::parse_or_warn("beam"), CursorStyle::Bar);
    assert_eq!(CursorStyle::parse_or_warn("  block "), CursorStyle::Block);
}

#[test]
fn cursor_style_unknown_falls_back_to_block() {
    assert_eq!(CursorStyle::parse_or_warn("rectangle"), CursorStyle::Block);
    assert_eq!(CursorStyle::parse_or_warn(""), CursorStyle::Block);
}

#[test]
fn cursor_style_as_cursor_shape_u8_maps_block_underline_bar() {
    // AC-1: block -> 0, underline -> 1, bar -> 2.
    assert_eq!(CursorStyle::Block.as_cursor_shape_u8(), 0);
    assert_eq!(CursorStyle::Underline.as_cursor_shape_u8(), 1);
    assert_eq!(CursorStyle::Bar.as_cursor_shape_u8(), 2);
}

#[test]
fn loader_font_size_overrides_default() {
    let s = load_json(r#"{"font_size": 15.5}"#);
    assert!((s.font_size - 15.5).abs() < f32::EPSILON);
}

#[test]
fn loader_font_size_zero_or_negative_keeps_default() {
    let s_zero = load_json(r#"{"font_size": 0}"#);
    assert!((s_zero.font_size - DEFAULT_FONT_SIZE_PT).abs() < f32::EPSILON);
    let s_neg = load_json(r#"{"font_size": -3}"#);
    assert!((s_neg.font_size - DEFAULT_FONT_SIZE_PT).abs() < f32::EPSILON);
}

#[test]
fn loader_padding_overrides_default() {
    let s = load_json(r#"{"padding": 12}"#);
    assert_eq!(s.padding, 12);
}

#[test]
fn loader_padding_zero_is_accepted() {
    let s = load_json(r#"{"padding": 0}"#);
    assert_eq!(s.padding, 0);
}

#[test]
fn loader_cursor_style_overrides_default() {
    let s = load_json(r#"{"cursor_style": "bar"}"#);
    assert_eq!(s.cursor_style, CursorStyle::Bar);
}

#[test]
fn loader_cursor_style_empty_keeps_default() {
    let s = load_json(r#"{"cursor_style": ""}"#);
    assert_eq!(s.cursor_style, CursorStyle::Block);
}

#[test]
fn loader_cursor_blink_can_be_disabled() {
    let s = load_json(r#"{"cursor_blink": false}"#);
    assert!(!s.cursor_blink);
}

// ── shift_enter_behavior loader (task0001 AC-1 / AC-2) ─────────────

#[test]
fn default_shift_enter_behavior_is_alt_enter() {
    // AC-1: the default is `alt_enter`.
    let s = Settings::new();
    assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::AltEnter);
}

#[test]
fn shift_enter_behavior_parses_each_wire_value() {
    assert_eq!(
        ShiftEnterBehavior::parse_or_warn("none"),
        ShiftEnterBehavior::None
    );
    assert_eq!(
        ShiftEnterBehavior::parse_or_warn("alt_enter"),
        ShiftEnterBehavior::AltEnter
    );
    assert_eq!(
        ShiftEnterBehavior::parse_or_warn("kitty_csi_u"),
        ShiftEnterBehavior::KittyCsiU
    );
    assert_eq!(
        ShiftEnterBehavior::parse_or_warn("lf"),
        ShiftEnterBehavior::Lf
    );
}

#[test]
fn loader_shift_enter_behavior_new_key_overrides_default_for_each_value() {
    // AC-2: new key present (each value) -> that value.
    let s = load_json(r#"{"shift_enter_behavior": "none"}"#);
    assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::None);
    let s = load_json(r#"{"shift_enter_behavior": "alt_enter"}"#);
    assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::AltEnter);
    let s = load_json(r#"{"shift_enter_behavior": "kitty_csi_u"}"#);
    assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::KittyCsiU);
    let s = load_json(r#"{"shift_enter_behavior": "lf"}"#);
    assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::Lf);
}

#[test]
fn loader_shift_enter_behavior_null_keeps_default() {
    // AC-2: new key null -> default.
    let s = load_json(r#"{"shift_enter_behavior": null}"#);
    assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::AltEnter);
}

#[test]
fn loader_shift_enter_behavior_unknown_string_falls_back_to_default() {
    // AC-2: new key unknown string -> default.
    let s = load_json(r#"{"shift_enter_behavior": "bogus"}"#);
    assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::AltEnter);
}

#[test]
fn loader_shift_enter_behavior_legacy_key_only_migrates_true_to_alt_enter() {
    // AC-2 / FR5: legacy key only, true -> alt_enter.
    let s = load_json(r#"{"shift_enter_as_alt_enter": true}"#);
    assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::AltEnter);
}

#[test]
fn loader_shift_enter_behavior_legacy_key_only_migrates_false_to_none() {
    // AC-2 / FR5: legacy key only, false -> none.
    let s = load_json(r#"{"shift_enter_as_alt_enter": false}"#);
    assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::None);
}

#[test]
fn loader_shift_enter_behavior_both_keys_new_key_wins() {
    // AC-2: both keys present -> new key wins over the legacy value
    // (here the legacy value alone would resolve to `alt_enter`, but
    // the new key explicitly says `none`).
    let s = load_json(r#"{"shift_enter_behavior": "none", "shift_enter_as_alt_enter": true}"#);
    assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::None);
}

#[test]
fn loader_shift_enter_behavior_neither_key_keeps_default() {
    // AC-2: neither key present -> default.
    let s = load_json(r#"{}"#);
    assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::AltEnter);
}

#[test]
fn loader_shift_enter_behavior_explicit_null_wins_over_legacy_true() {
    // AC-3: the new key present-but-null must be distinguished from
    // the new key being absent — present null resolves to the
    // default and wins over the legacy key, even though here the
    // legacy value alone would ALSO resolve to `alt_enter` (so this
    // case alone would not catch a regression to the old
    // "null == absent" behavior; see the `_false` case below).
    let s = load_json(r#"{"shift_enter_behavior": null, "shift_enter_as_alt_enter": true}"#);
    assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::AltEnter);
}

#[test]
fn loader_shift_enter_behavior_explicit_null_wins_over_legacy_false() {
    // AC-3: present null -> default (`alt_enter`), NOT `none` (which
    // the legacy `false` value alone would produce). This is the
    // regression case: conflating "null" with "absent" would
    // incorrectly fall through to the legacy boolean here.
    let s = load_json(r#"{"shift_enter_behavior": null, "shift_enter_as_alt_enter": false}"#);
    assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::AltEnter);
}

#[test]
fn load_from_valid_file_applies_overrides() {
    let dir = std::env::temp_dir();
    let p = dir.join(format!("emterm_settings_valid_{}.json", std::process::id()));
    std::fs::write(
        &p,
        br#"{
            "scrollback_lines": 7,
            "native_poc": {"ambiguous_width_mode": "wide", "font_engine": "ab_glyph"}
        }"#,
    )
    .expect("write tmp settings");
    let s = Settings::load_from(&p);
    let _ = std::fs::remove_file(&p);
    assert_eq!(s.scrollback_lines, 7);
    assert_eq!(s.ambiguous_width_mode, AmbiguousWidthMode::Wide);
    assert_eq!(s.font_engine, FontEngine::AbGlyph);
}

// ── keybinds defaults + loader ─────────────────────────────────────

#[test]
fn default_keybinds_match_src_tauri() {
    let kb = Settings::new().keybinds;
    assert_eq!(kb.copy, "Ctrl+Shift+C");
    assert_eq!(kb.paste, "Ctrl+Shift+V");
    assert_eq!(kb.select_all, "Ctrl+Shift+A");
    assert_eq!(kb.search, "Ctrl+Shift+F");
    assert_eq!(kb.new_tab, "Ctrl+Shift+T");
    assert_eq!(kb.new_tab_global, "Ctrl+Shift+G");
    assert_eq!(kb.close_tab, "Ctrl+Shift+W");
    assert_eq!(kb.next_tab, "Ctrl+PageDown");
    assert_eq!(kb.prev_tab, "Ctrl+PageUp");
    assert_eq!(kb.zoom_in, "Ctrl+Plus");
    assert_eq!(kb.zoom_out, "Ctrl+Minus");
    assert_eq!(kb.zoom_reset, "Ctrl+0");
    assert_eq!(kb.toggle_fullscreen, "F11");
    assert_eq!(kb.open_settings, "Ctrl+,");
    assert_eq!(kb.toggle_tab_bar, "Ctrl+Shift+B");
    assert_eq!(kb.jump_to_prev_prompt, "Ctrl+Shift+ArrowUp");
    assert_eq!(kb.jump_to_next_prompt, "Ctrl+Shift+ArrowDown");
    assert_eq!(kb.profile_selector, "Ctrl+Shift+P");
}

#[test]
fn keybinds_default_round_trip() {
    let s = Settings::new();
    assert_eq!(s.keybinds, KeybindSettings::default());
}

#[test]
fn loader_keybinds_override_only_specified_keys() {
    let s = load_json(r#"{"keybinds": {"new_tab": "Ctrl+Shift+N", "copy": "Ctrl+Insert"}}"#);
    // Overridden keys take the new spec.
    assert_eq!(s.keybinds.new_tab, "Ctrl+Shift+N");
    assert_eq!(s.keybinds.copy, "Ctrl+Insert");
    // Everything else stays at the default.
    let d = KeybindSettings::default();
    assert_eq!(s.keybinds.paste, d.paste);
    assert_eq!(s.keybinds.close_tab, d.close_tab);
    assert_eq!(s.keybinds.next_tab, d.next_tab);
    assert_eq!(s.keybinds.prev_tab, d.prev_tab);
}

#[test]
fn loader_keybinds_blank_specs_keep_defaults() {
    let s = load_json(r#"{"keybinds": {"copy": "", "paste": "   ", "new_tab": "Ctrl+T"}}"#);
    let d = KeybindSettings::default();
    // Blank / whitespace-only specs are dropped.
    assert_eq!(s.keybinds.copy, d.copy);
    assert_eq!(s.keybinds.paste, d.paste);
    // A non-blank spec still applies.
    assert_eq!(s.keybinds.new_tab, "Ctrl+T");
}

#[test]
fn loader_keybinds_null_keeps_defaults() {
    let s = load_json(r#"{"keybinds": null}"#);
    assert_eq!(s.keybinds, KeybindSettings::default());
}

#[test]
fn loader_keybinds_unknown_keys_do_not_panic() {
    let s = load_json(r#"{"keybinds": {"some_future_action": "Ctrl+Z", "copy": "Ctrl+Insert"}}"#);
    // Unknown keys are ignored; known keys still apply.
    assert_eq!(s.keybinds.copy, "Ctrl+Insert");
    assert_eq!(s.keybinds.paste, KeybindSettings::default().paste);
}

// ── Markdown viewer settings (Phase 1 / TS-9, TS-10) ────────────────

#[test]
fn markdown_settings_defaults_match_spec() {
    // SPEC §Settings: follow_ui=true, theme=System, preset=Purple,
    // fonts empty, size 14.
    let s = Settings::new();
    assert!(s.markdown_theme_follow_ui);
    assert_eq!(s.markdown_theme, UiTheme::System);
    assert_eq!(s.markdown_theme_preset, UiThemePreset::Purple);
    assert_eq!(s.markdown_body_font_family, "");
    assert_eq!(s.markdown_code_font_family, "");
    assert_eq!(s.markdown_font_size, 14);
}

#[test]
fn loader_markdown_flat_keys_are_applied() {
    let s = load_json(
        r#"{
            "markdown_theme_follow_ui": false,
            "markdown_theme": "light",
            "markdown_theme_preset": "green",
            "markdown_body_font_family": "Noto Sans",
            "markdown_code_font_family": "Fira Code",
            "markdown_font_size": 16
        }"#,
    );
    assert!(!s.markdown_theme_follow_ui);
    assert_eq!(s.markdown_theme, UiTheme::Light);
    assert_eq!(s.markdown_theme_preset, UiThemePreset::Green);
    assert_eq!(s.markdown_body_font_family, "Noto Sans");
    assert_eq!(s.markdown_code_font_family, "Fira Code");
    assert_eq!(s.markdown_font_size, 16);
}

#[test]
fn loader_markdown_null_keys_keep_defaults() {
    let s = load_json(
        r#"{
            "markdown_theme_follow_ui": null,
            "markdown_theme": null,
            "markdown_theme_preset": null,
            "markdown_body_font_family": null,
            "markdown_code_font_family": null,
            "markdown_font_size": null
        }"#,
    );
    let d = Settings::default();
    assert_eq!(s.markdown_theme_follow_ui, d.markdown_theme_follow_ui);
    assert_eq!(s.markdown_theme, d.markdown_theme);
    assert_eq!(s.markdown_theme_preset, d.markdown_theme_preset);
    assert_eq!(s.markdown_body_font_family, d.markdown_body_font_family);
    assert_eq!(s.markdown_code_font_family, d.markdown_code_font_family);
    assert_eq!(s.markdown_font_size, d.markdown_font_size);
}

#[test]
fn loader_markdown_unknown_theme_falls_back() {
    let s = load_json(r#"{"markdown_theme": "chartreuse", "markdown_theme_preset": "cyan"}"#);
    // Unknown enum values coerce to documented defaults.
    assert_eq!(s.markdown_theme, UiTheme::System);
    assert_eq!(s.markdown_theme_preset, UiThemePreset::Purple);
}

// ── Profiles / SSH / SFTP ────────────────────────────────────────

#[test]
fn loader_profiles_and_ssh_defaults() {
    let s = load_json("{}");
    assert!(s.profiles.is_empty());
    assert!(s.ssh_connections.is_empty());
    assert_eq!(s.ssh_command_path, "");
    assert_eq!(
        s.sftp_max_concurrent_uploads,
        DEFAULT_SFTP_MAX_CONCURRENT_UPLOADS
    );
}

#[test]
fn loader_profiles_and_ssh_parse_src_tauri_shape() {
    let s = load_json(
        r#"{
            "profiles": [
                {
                    "name": "dev",
                    "shell_path": "/bin/zsh",
                    "shell_args": ["-l"],
                    "env_vars": "FOO=bar",
                    "working_directory": "/tmp",
                    "is_default": true,
                    "ssh_connection_name": "",
                    "wsl_distro_name": ""
                },
                { "name": "minimal" }
            ],
            "ssh_command_path": "/usr/bin/ssh",
            "ssh_connections": [
                {
                    "name": "work",
                    "hostname": "example.com",
                    "port": 2222,
                    "username": "user",
                    "identity_file": "~/.ssh/id_rsa",
                    "ssh_options": [
                        { "key": "ServerAliveInterval", "value": "60" }
                    ]
                },
                { "name": "bare", "hostname": "h", "port": null }
            ],
            "sftp_max_concurrent_uploads": 8
        }"#,
    );
    assert_eq!(s.profiles.len(), 2);
    assert_eq!(s.profiles[0].name, "dev");
    assert!(s.profiles[0].is_default);
    assert_eq!(s.profiles[0].shell_args, vec!["-l".to_string()]);
    // Partial entries fill the app_settings per-field defaults.
    assert_eq!(s.profiles[1].name, "minimal");
    assert!(!s.profiles[1].is_default);
    assert_eq!(s.ssh_command_path, "/usr/bin/ssh");
    assert_eq!(s.ssh_connections.len(), 2);
    assert_eq!(s.ssh_connections[0].port, 2222);
    assert_eq!(s.ssh_connections[0].ssh_options.len(), 1);
    // `null` port falls back to 22 (src-tauri deserializer parity).
    assert_eq!(s.ssh_connections[1].port, 22);
    assert_eq!(s.sftp_max_concurrent_uploads, 8);
}

#[test]
fn appearance_follow_ui_true_uses_ui_theme_source() {
    let s = load_json(
        r#"{
            "ui_theme": "dark",
            "ui_theme_preset": "blue",
            "markdown_theme_follow_ui": true,
            "markdown_theme": "light",
            "markdown_theme_preset": "green",
            "markdown_body_font_family": "Body",
            "markdown_code_font_family": "Code",
            "markdown_font_size": 20
        }"#,
    );
    let a = s.markdown_appearance();
    // follow_ui = true -> theme/preset come from the UI chrome source.
    assert_eq!(a.theme, UiTheme::Dark);
    assert_eq!(a.preset, UiThemePreset::Blue);
    // Fonts and size always come from the markdown_* keys.
    assert_eq!(a.body_font_family, "Body");
    assert_eq!(a.code_font_family, "Code");
    assert_eq!(a.font_size, 20);
}

#[test]
fn appearance_follow_ui_false_uses_markdown_theme_source() {
    let s = load_json(
        r#"{
            "ui_theme": "dark",
            "ui_theme_preset": "blue",
            "markdown_theme_follow_ui": false,
            "markdown_theme": "light",
            "markdown_theme_preset": "green"
        }"#,
    );
    let a = s.markdown_appearance();
    // follow_ui = false -> theme/preset come from the markdown_* source.
    assert_eq!(a.theme, UiTheme::Light);
    assert_eq!(a.preset, UiThemePreset::Green);
}
