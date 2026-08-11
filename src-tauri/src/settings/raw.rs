//! `settings.json` deserialize layer: the `Raw*` mirror structs and
//! [`RawSettings::merge_into`], which folds a parsed file into
//! [`Settings`], plus the parse-or-warn helpers only this layer uses.

use super::Settings;
use super::types::{
    AmbiguousWidthMode, BellAction, CursorStyle, CustomCommand, DEFAULT_FONT_SIZE_PT, FontEngine,
    ScrollbarMode, ShiftEnterBehavior, UiTheme, UiThemePreset, UserColorScheme,
    default_mux_action_chord, is_legacy_mux_action, parse_mux_action_chord,
};
use crate::settings_core::Language;

// ============================================================
// settings.json schema (deserialize side)
// ============================================================
//
// `RawSettings` mirrors the on-disk JSON layout. Top-level keys match
// the legacy Tauri WebView build (`src-tauri/.../config/settings.rs`'s
// `AppSettings`) so a single `settings.json` works for both binaries.
// native-poc-specific fields live under a dedicated `native_poc:` block
// to avoid colliding with future src-tauri additions.
//
// Every field is `Option<_>` with `#[serde(default)]` on the struct, so:
// - missing keys leave the corresponding `Settings` field untouched
// - explicit `null` is treated as "absent" (Option = None)
// - unknown keys are ignored (forward compatibility with newer
//   src-tauri settings written by the legacy build)
//
// Exception: `shift_enter_behavior` is `Option<Option<String>>` because
// its precedence over the legacy `shift_enter_as_alt_enter` boolean
// depends on distinguishing "key absent" from "key present with null"
// (FR5 / AC-3) — see its field doc comment.

/// Deserializes a JSON field into `Option<Option<T>>`, distinguishing an
/// absent key (`#[serde(default)]` on the container yields `None`; this
/// function is never invoked for a missing key) from a key present with
/// `null` (`Some(None)`) or a concrete value (`Some(Some(v))`). Used
/// where the difference between "key absent" and "key explicitly null"
/// changes precedence against a legacy fallback key — see
/// `RawSettings::shift_enter_behavior` / FR5.
fn deserialize_present_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    <Option<T> as serde::Deserialize>::deserialize(deserializer).map(Some)
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
pub(in crate::settings) struct RawSettings {
    // ── src-tauri / WebView build compatible (flat keys) ──
    scrollback_lines: Option<u32>,
    clipboard_read_osc52: Option<bool>,
    clipboard_max_size_osc52: Option<u32>,
    font_family_primary: Option<String>,
    font_family_secondary: Option<String>,
    font_size: Option<f32>,
    padding: Option<u32>,
    cursor_style: Option<String>,
    cursor_blink: Option<bool>,
    mux: Option<RawMux>,
    keybinds: Option<RawKeybinds>,

    statusbar_enabled: Option<bool>,
    statusbar_app_line1_left: Option<String>,
    statusbar_app_line1_right: Option<String>,
    statusbar_app_line2_left: Option<String>,
    statusbar_app_line2_right: Option<String>,
    statusbar_time_format: Option<String>,
    statusbar_font_size: Option<f32>,
    statusbar_custom_commands: Option<std::collections::HashMap<String, RawCustomCommand>>,
    statusbar_refresh_rates: Option<std::collections::HashMap<String, u64>>,

    // ── UI chrome ──
    ui_theme: Option<String>,
    ui_theme_preset: Option<String>,
    ui_font_family: Option<String>,
    terminal_color_scheme: Option<String>,
    custom_color_schemes: Option<Vec<RawUserColorScheme>>,
    show_scrollbar: Option<String>,
    show_tab_bar: Option<bool>,
    bold_brightens_ansi_colors: Option<bool>,

    // ── Terminal behavior ──
    shell_path: Option<String>,
    shell_args: Option<Vec<String>>,
    scroll_speed: Option<u32>,
    alternate_scroll_enabled: Option<bool>,
    copy_on_select: Option<bool>,
    middle_click_paste: Option<bool>,
    /// New wire key (task0001/task0003). Wins over the legacy
    /// `shift_enter_as_alt_enter` boolean below whenever this key was
    /// PRESENT in the source JSON — including when it is explicitly
    /// `null` (FR5 / AC-3). `Option<Option<String>>` (rather than the
    /// plain `Option<String>` every other field in this struct uses)
    /// distinguishes "key absent" (`None`, the struct-level
    /// `#[serde(default)]`) from "key present with `null`"
    /// (`Some(None)`, produced by [`deserialize_present_option`]) so
    /// `merge_into` can resolve that precedence correctly; a plain
    /// `Option<String>` would conflate the two (both deserialize to
    /// `None`).
    #[serde(deserialize_with = "deserialize_present_option")]
    shift_enter_behavior: Option<Option<String>>,
    /// Legacy boolean, migration input only (FR5): `true` -> `AltEnter`,
    /// `false` -> `None`. Never written back to `settings.json`.
    shift_enter_as_alt_enter: Option<bool>,
    skk_mode: Option<bool>,
    bell_action: Option<String>,
    url_detection: Option<bool>,
    file_path_detection: Option<bool>,
    fold_enabled: Option<bool>,
    editor_command: Option<String>,
    /// src-tauri-flat ambiguous-width toggle. `true` ≡ `Wide`, `false` ≡
    /// `Narrow`. `native_poc.ambiguous_width_mode` (string form) wins
    /// over this when both are present.
    ambiguous_width: Option<bool>,

    // ── Notifications ──
    notification_enabled: Option<bool>,
    tab_activity_indicator: Option<bool>,
    notify_on_process_exit: Option<bool>,
    notify_on_output: Option<bool>,
    notify_on_bell: Option<bool>,
    agent_status_notifications: Option<bool>,
    agent_notify_on_done: Option<bool>,
    agent_notify_on_blocked: Option<bool>,
    agent_notify_visible_pane: Option<bool>,

    // ── Language / logging ──
    language: Option<String>,
    log_recording_enabled: Option<bool>,

    // ── Markdown viewer (flat keys, src-tauri compatible) ──
    markdown_theme_follow_ui: Option<bool>,
    markdown_theme: Option<String>,
    markdown_theme_preset: Option<String>,
    markdown_body_font_family: Option<String>,
    markdown_code_font_family: Option<String>,
    markdown_font_size: Option<u32>,

    // ── Profiles / SSH / SFTP (src-tauri compatible). The entry types
    // come from the shared `app_settings` crate so per-field defaults
    // and null-handling match the legacy build exactly. ──
    profiles: Option<Vec<app_settings::Profile>>,
    ssh_command_path: Option<String>,
    ssh_connections: Option<Vec<app_settings::SshConnection>>,
    sftp_max_concurrent_uploads: Option<u16>,

    // ── native-poc-specific (nested) ──
    native_poc: Option<RawNativePoc>,
}

#[derive(Debug, serde::Deserialize)]
struct RawUserColorScheme {
    name: String,
    #[serde(default)]
    foreground: String,
    #[serde(default)]
    background: String,
    #[serde(default)]
    cursor: String,
    #[serde(default)]
    selection: String,
    #[serde(default)]
    ansi_colors: Vec<String>,
}

/// `statusbar` (the retired `mux.statusbar.*` object, formerly
/// deserialized into `RawMuxStatusbar`) is deliberately NOT a field here
/// (mux-status-bar-removal task0001, FR4/FR8b): with no field to name it,
/// serde's default unknown-field handling (no `deny_unknown_fields`
/// anywhere in this loader) leaves a stale `settings.json` still carrying
/// that key loading successfully, silently ignoring it.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct RawMux {
    prefix: Option<String>,
    tab_always_expand: Option<bool>,
    window_sidebar_overlay: Option<bool>,
    keybinds: Option<std::collections::HashMap<String, String>>,
}

/// Deserialize side of the nested `"keybinds"` block. Mirrors
/// `src-tauri`'s `AppSettings.keybinds` (snake_case string values).
/// Every field is `Option<String>` with `#[serde(default)]` so missing
/// keys, explicit `null`, and unknown keys all leave the corresponding
/// [`KeybindSettings`] field at its default.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct RawKeybinds {
    copy: Option<String>,
    paste: Option<String>,
    select_all: Option<String>,
    search: Option<String>,
    new_tab: Option<String>,
    new_tab_global: Option<String>,
    close_tab: Option<String>,
    next_tab: Option<String>,
    prev_tab: Option<String>,
    zoom_in: Option<String>,
    zoom_out: Option<String>,
    zoom_reset: Option<String>,
    toggle_fullscreen: Option<String>,
    open_settings: Option<String>,
    toggle_tab_bar: Option<String>,
    jump_to_prev_prompt: Option<String>,
    jump_to_next_prompt: Option<String>,
    profile_selector: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct RawCustomCommand {
    executable: String,
    interval_ms: Option<u64>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct RawNativePoc {
    ambiguous_width_mode: Option<String>,
    font_engine: Option<String>,
    image_memory_quota_mb: Option<u32>,
    ime: Option<RawIme>,
    font_family_fallback: Option<Vec<String>>,
    variable_font_axes: Option<std::collections::HashMap<String, f32>>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct RawIme {
    native_integration: Option<bool>,
}

impl RawSettings {
    /// Apply every present field onto `dst`. Absent fields leave the
    /// corresponding `Settings` field at its prior value (typically the
    /// default that `dst` was seeded with).
    pub(in crate::settings) fn merge_into(self, dst: &mut Settings) {
        // ── flat keys (src-tauri compatible) ──
        if let Some(v) = self.scrollback_lines {
            dst.scrollback_lines = v;
        }
        if let Some(v) = self.clipboard_read_osc52 {
            dst.clipboard_read_osc52 = v;
        }
        if let Some(v) = self.clipboard_max_size_osc52 {
            dst.clipboard_max_size_osc52 = v;
        }
        if let Some(v) = self.font_size {
            // `font_size` from `settings.json` is a logical-point value;
            // sanitize against absurd inputs that would render an
            // unusable grid (and would also break the cell-metrics
            // computation). The legacy WebView build does the same
            // clamp in `applySettings`.
            if v.is_finite() && v > 0.0 {
                dst.font_size = v;
            } else {
                log::warn!(
                    "settings.font_size: invalid value {:?}, keeping default {}",
                    v,
                    DEFAULT_FONT_SIZE_PT
                );
            }
        }
        if let Some(v) = self.padding {
            dst.padding = v;
        }
        if let Some(v) = self.cursor_style.filter(|s| !s.trim().is_empty()) {
            dst.cursor_style = CursorStyle::parse_or_warn(&v);
        }
        if let Some(v) = self.cursor_blink {
            dst.cursor_blink = v;
        }

        // Font fallback derived from the src-tauri-compatible flat keys.
        // primary -> secondary becomes `font_family_fallback`; the bundled
        // CJK / emoji fonts continue to live at the resolver layer. If
        // `native_poc.font_family_fallback` is also present it overrides
        // this further down (explicit wins).
        let mut fb = Vec::new();
        if let Some(v) = self.font_family_primary.filter(|s| !s.trim().is_empty()) {
            fb.push(v);
        }
        if let Some(v) = self.font_family_secondary.filter(|s| !s.trim().is_empty()) {
            fb.push(v);
        }
        if !fb.is_empty() {
            dst.font_family_fallback = fb;
        }

        if let Some(mux) = self.mux {
            if let Some(v) = mux.prefix.filter(|s| !s.trim().is_empty()) {
                dst.mux_prefix_key = v;
            }
            if let Some(v) = mux.tab_always_expand {
                dst.mux.tab_always_expand = v;
            }
            if let Some(v) = mux.window_sidebar_overlay {
                dst.mux.window_sidebar_overlay = v;
            }
            if let Some(kb) = mux.keybinds {
                for (action, spec) in kb {
                    // Unknown action names are ignored (forward compat).
                    if default_mux_action_chord(&action).is_none() {
                        // Pre-SPEC-mux-feature-cleanup actions can still
                        // be present in `settings.json` files written by
                        // the WebView build (or older native builds).
                        // They are dead bindings the WebView frontend
                        // ignores; silently drop them here so a normal
                        // launch doesn't repeat the same warn for every
                        // legacy entry.
                        if is_legacy_mux_action(&action) {
                            log::debug!(
                                "settings.mux.keybinds: legacy action {:?}, dropped",
                                action
                            );
                        } else {
                            log::warn!(
                                "settings.mux.keybinds: unknown action {:?}, ignored",
                                action
                            );
                        }
                        continue;
                    }
                    // Blank entries keep the default (matching mux.prefix).
                    if spec.trim().is_empty() {
                        continue;
                    }
                    match parse_mux_action_chord(&spec) {
                        Some(c) => {
                            dst.mux.keybinds.insert(action, c);
                        }
                        None => {
                            log::warn!(
                                "settings.mux.keybinds.{}: invalid chord {:?}, keeping default",
                                action,
                                spec
                            );
                        }
                    }
                }
            }
            // `mux.statusbar.*` (retired, mux-status-bar-removal task0001)
            // is deliberately not merged here — see `RawMux`'s doc comment
            // for the FR4/FR8b tolerance contract.
        }

        // keybinds (nested, snake_case). Blank / whitespace-only specs
        // are dropped so a `"copy": ""` entry keeps the default chord
        // (matching the `mux.prefix` treatment above).
        if let Some(kb) = self.keybinds {
            if let Some(v) = kb.copy.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.copy = v;
            }
            if let Some(v) = kb.paste.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.paste = v;
            }
            if let Some(v) = kb.select_all.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.select_all = v;
            }
            if let Some(v) = kb.search.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.search = v;
            }
            if let Some(v) = kb.new_tab.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.new_tab = v;
            }
            if let Some(v) = kb.new_tab_global.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.new_tab_global = v;
            }
            if let Some(v) = kb.close_tab.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.close_tab = v;
            }
            if let Some(v) = kb.next_tab.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.next_tab = v;
            }
            if let Some(v) = kb.prev_tab.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.prev_tab = v;
            }
            if let Some(v) = kb.zoom_in.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.zoom_in = v;
            }
            if let Some(v) = kb.zoom_out.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.zoom_out = v;
            }
            if let Some(v) = kb.zoom_reset.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.zoom_reset = v;
            }
            if let Some(v) = kb.toggle_fullscreen.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.toggle_fullscreen = v;
            }
            if let Some(v) = kb.open_settings.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.open_settings = v;
            }
            if let Some(v) = kb.toggle_tab_bar.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.toggle_tab_bar = v;
            }
            if let Some(v) = kb.jump_to_prev_prompt.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.jump_to_prev_prompt = v;
            }
            if let Some(v) = kb.jump_to_next_prompt.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.jump_to_next_prompt = v;
            }
            if let Some(v) = kb.profile_selector.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.profile_selector = v;
            }
        }

        // statusbar (flat src-tauri shape -> nested native-poc shape)
        if let Some(v) = self.statusbar_enabled {
            dst.statusbar.enabled = v;
        }
        if let Some(v) = self.statusbar_app_line1_left {
            dst.statusbar.app_line1_left = v;
        }
        if let Some(v) = self.statusbar_app_line1_right {
            dst.statusbar.app_line1_right = v;
        }
        if let Some(v) = self.statusbar_app_line2_left {
            dst.statusbar.app_line2_left = v;
        }
        if let Some(v) = self.statusbar_app_line2_right {
            dst.statusbar.app_line2_right = v;
        }
        if let Some(v) = self.statusbar_time_format {
            dst.statusbar.time_format = v;
        }
        if let Some(v) = self.statusbar_font_size {
            dst.statusbar.font_size = Some(v);
        }
        if let Some(v) = self.statusbar_custom_commands {
            dst.statusbar.custom_commands = v
                .into_iter()
                .map(|(k, c)| {
                    (
                        k,
                        CustomCommand {
                            executable: c.executable,
                            interval_ms: c.interval_ms.unwrap_or(1000),
                        },
                    )
                })
                .collect();
        }
        if let Some(v) = self.statusbar_refresh_rates {
            dst.statusbar.refresh_rates = v;
        }

        // ── UI chrome ──
        if let Some(v) = self.ui_theme {
            dst.ui_theme = UiTheme::parse_or_warn(&v);
        }
        if let Some(v) = self.ui_theme_preset {
            dst.ui_theme_preset = UiThemePreset::parse_or_warn(&v);
        }
        if let Some(v) = self.ui_font_family.filter(|s| !s.trim().is_empty()) {
            dst.ui_font_family = v;
        }
        if let Some(v) = self.terminal_color_scheme {
            dst.terminal_color_scheme = v;
        }
        if let Some(v) = self.custom_color_schemes {
            dst.custom_color_schemes = v
                .into_iter()
                .map(|r| UserColorScheme {
                    name: r.name,
                    foreground: r.foreground,
                    background: r.background,
                    cursor: r.cursor,
                    selection: r.selection,
                    ansi_colors: r.ansi_colors,
                })
                .collect();
        }
        if let Some(v) = self.show_scrollbar {
            dst.show_scrollbar = ScrollbarMode::parse_or_warn(&v);
        }
        if let Some(v) = self.show_tab_bar {
            dst.show_tab_bar = v;
        }
        if let Some(v) = self.bold_brightens_ansi_colors {
            dst.bold_brightens_ansi_colors = v;
        }

        // ── Terminal behavior ──
        if let Some(v) = self.shell_path.filter(|s| !s.trim().is_empty()) {
            dst.shell_path = v;
        }
        if let Some(v) = self.shell_args {
            dst.shell_args = v;
        }
        if let Some(v) = self.scroll_speed {
            // Mirrors src-tauri's MIN/MAX (1..=10). Out-of-range values
            // are clamped so a typo can't lock the wheel at 0 or fly the
            // viewport at e.g. 1000 lines/notch.
            dst.scroll_speed = v.clamp(1, 10);
        }
        if let Some(v) = self.alternate_scroll_enabled {
            dst.alternate_scroll_enabled = v;
        }
        if let Some(v) = self.copy_on_select {
            dst.copy_on_select = v;
        }
        if let Some(v) = self.middle_click_paste {
            dst.middle_click_paste = v;
        }
        // FR5 migration: the new key wins whenever it was PRESENT in the
        // source JSON — including an explicit `null`, which resolves to
        // the default (AC-3) rather than falling through to the legacy
        // boolean. Only when the new key is fully ABSENT does the legacy
        // boolean apply (true -> AltEnter, false -> None). Neither
        // present leaves `dst` at the default seeded by
        // `Settings::default()` (AltEnter). The legacy key is read-only
        // here and is never written back to settings.json.
        match self.shift_enter_behavior {
            Some(Some(v)) => dst.shift_enter_behavior = ShiftEnterBehavior::parse_or_warn(&v),
            Some(None) => dst.shift_enter_behavior = ShiftEnterBehavior::default(),
            None => {
                if let Some(legacy) = self.shift_enter_as_alt_enter {
                    dst.shift_enter_behavior = if legacy {
                        ShiftEnterBehavior::AltEnter
                    } else {
                        ShiftEnterBehavior::None
                    };
                }
            }
        }
        if let Some(v) = self.skk_mode {
            dst.skk_mode = v;
        }
        if let Some(v) = self.bell_action {
            dst.bell_action = BellAction::parse_or_warn(&v);
        }
        if let Some(v) = self.url_detection {
            dst.url_detection = v;
        }
        if let Some(v) = self.file_path_detection {
            dst.file_path_detection = v;
        }
        if let Some(v) = self.fold_enabled {
            dst.fold_enabled = v;
        }
        if let Some(v) = self.editor_command.filter(|s| !s.trim().is_empty()) {
            dst.editor_command = v;
        }
        if let Some(v) = self.notification_enabled {
            dst.notification_enabled = v;
        }
        if let Some(v) = self.tab_activity_indicator {
            dst.tab_activity_indicator = v;
        }
        if let Some(v) = self.notify_on_process_exit {
            dst.notify_on_process_exit = v;
        }
        if let Some(v) = self.notify_on_output {
            dst.notify_on_output = v;
        }
        if let Some(v) = self.notify_on_bell {
            dst.notify_on_bell = v;
        }
        if let Some(v) = self.agent_status_notifications {
            dst.agent_status_notifications = v;
        }
        if let Some(v) = self.agent_notify_on_done {
            dst.agent_notify_on_done = v;
        }
        if let Some(v) = self.agent_notify_on_blocked {
            dst.agent_notify_on_blocked = v;
        }
        if let Some(v) = self.agent_notify_visible_pane {
            dst.agent_notify_visible_pane = v;
        }

        // ── Language / logging ──
        if let Some(v) = self.language {
            dst.language = Language::parse_or_warn(&v);
        }
        if let Some(v) = self.log_recording_enabled {
            dst.log_recording_enabled = v;
        }

        // ── Markdown viewer ──
        if let Some(v) = self.markdown_theme_follow_ui {
            dst.markdown_theme_follow_ui = v;
        }
        if let Some(v) = self.markdown_theme {
            dst.markdown_theme = UiTheme::parse_or_warn(&v);
        }
        if let Some(v) = self.markdown_theme_preset {
            dst.markdown_theme_preset = UiThemePreset::parse_or_warn(&v);
        }
        // Fonts: empty string is a valid value (→ CSS fallback chain), so
        // unlike `ui_font_family` we do not drop blanks here.
        if let Some(v) = self.markdown_body_font_family {
            dst.markdown_body_font_family = v;
        }
        if let Some(v) = self.markdown_code_font_family {
            dst.markdown_code_font_family = v;
        }
        if let Some(v) = self.markdown_font_size {
            dst.markdown_font_size = v;
        }

        // ── Profiles / SSH / SFTP ──
        if let Some(v) = self.profiles {
            dst.profiles = v;
        }
        if let Some(v) = self.ssh_command_path.filter(|s| !s.trim().is_empty()) {
            dst.ssh_command_path = v;
        }
        if let Some(v) = self.ssh_connections {
            dst.ssh_connections = v;
        }
        if let Some(v) = self.sftp_max_concurrent_uploads {
            dst.sftp_max_concurrent_uploads = v;
        }

        // Flat-key bridge: only seed the native-poc enum when the user
        // hasn't already set the more explicit `native_poc.ambiguous_width_mode`
        // string. The native_poc block runs *after* this, so if both are
        // present the string form wins, matching the "explicit overrides
        // flat keys" pattern used elsewhere in this loader.
        if let Some(v) = self.ambiguous_width {
            dst.ambiguous_width_mode = if v {
                AmbiguousWidthMode::Wide
            } else {
                AmbiguousWidthMode::Narrow
            };
        }

        // ── native_poc.* (explicit overrides win over flat keys) ──
        if let Some(np) = self.native_poc {
            if let Some(v) = np.ambiguous_width_mode {
                dst.ambiguous_width_mode = parse_ambiguous_width_or_warn(&v);
            }
            if let Some(v) = np.font_engine {
                dst.font_engine = FontEngine::parse_or_warn(&v);
            }
            if let Some(v) = np.image_memory_quota_mb {
                dst.image_memory_quota_mb = v;
            }
            if let Some(ime) = np.ime {
                if let Some(b) = ime.native_integration {
                    dst.ime.native_integration = b;
                }
            }
            if let Some(v) = np.font_family_fallback {
                dst.font_family_fallback = v;
            }
            if let Some(v) = np.variable_font_axes {
                dst.variable_font_axes = v;
            }
        }
    }
}

fn parse_ambiguous_width_or_warn(spec: &str) -> AmbiguousWidthMode {
    match spec.trim().to_ascii_lowercase().as_str() {
        "wide" => AmbiguousWidthMode::Wide,
        "narrow" => AmbiguousWidthMode::Narrow,
        other => {
            warn_unknown_ambiguous_width_once(other);
            AmbiguousWidthMode::Narrow
        }
    }
}

fn warn_unknown_ambiguous_width_once(seen: &str) {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    let owned = seen.to_string();
    ONCE.call_once(move || {
        log::warn!(
            "settings.native_poc.ambiguous_width_mode: unknown value {:?}, falling back to \"narrow\"",
            owned
        );
    });
}
