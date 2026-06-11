//! Settings persistence for the in-app settings panel.
//!
//! The WebView build's `save_settings` Tauri command serializes its
//! complete `AppSettings` struct, which carries every key (profiles,
//! SSH connections, mux sub-keys, …). native-poc's [`crate::settings::
//! Settings`] only models a subset of those keys, so a whole-struct
//! rewrite would silently delete everything it does not know about.
//!
//! Instead, saves go through a **read-modify-write patch**: the on-disk
//! JSON is parsed as a raw `serde_json` object, only the keys the
//! settings panel actually manages are overwritten, and the result is
//! written back atomically (temp file + rename). Keys the native build
//! has never heard of round-trip untouched.

use std::path::Path;

use serde_json::{json, Map, Value};

use crate::settings::Settings;

/// Build the flat-key patch for every setting the native settings panel
/// manages today (UI appearance + terminal appearance + terminal
/// behavior categories). Keys/values use the WebView build's
/// `AppSettings` spelling so the same `settings.json` keeps working for
/// both binaries.
pub fn panel_patch(s: &Settings) -> Map<String, Value> {
    let mut m = Map::new();

    // ── UI appearance ──
    m.insert("language".into(), json!(s.language.as_str()));
    m.insert("ui_theme".into(), json!(s.ui_theme.as_str()));
    m.insert("ui_theme_preset".into(), json!(s.ui_theme_preset.as_str()));
    m.insert("ui_font_family".into(), json!(s.ui_font_family));

    // ── Terminal appearance ──
    m.insert("font_size".into(), json!(s.font_size));
    // The native model folds the flat primary/secondary keys into
    // `font_family_fallback` at load time; project them back out the
    // same way. An empty slot is written as "" which the loaders on
    // both sides treat as "keep the built-in default".
    m.insert(
        "font_family_primary".into(),
        json!(s.font_family_fallback.first().cloned().unwrap_or_default()),
    );
    m.insert(
        "font_family_secondary".into(),
        json!(s.font_family_fallback.get(1).cloned().unwrap_or_default()),
    );
    m.insert(
        "font_family_emoji".into(),
        json!(s.emoji_font.clone().unwrap_or_default()),
    );
    m.insert(
        "terminal_color_scheme".into(),
        json!(s.terminal_color_scheme),
    );
    m.insert(
        "bold_brightens_ansi_colors".into(),
        json!(s.bold_brightens_ansi_colors),
    );
    m.insert("padding".into(), json!(s.padding));
    m.insert("scrollback_lines".into(), json!(s.scrollback_lines));
    m.insert("show_scrollbar".into(), json!(s.show_scrollbar.as_str()));

    // ── Terminal behavior ──
    m.insert("cursor_style".into(), json!(s.cursor_style.as_str()));
    m.insert("cursor_blink".into(), json!(s.cursor_blink));
    m.insert("shell_path".into(), json!(s.shell_path));
    m.insert("shell_args".into(), json!(s.shell_args));
    m.insert("scroll_speed".into(), json!(s.scroll_speed));
    m.insert("bell_action".into(), json!(s.bell_action.as_str()));
    m.insert("url_detection".into(), json!(s.url_detection));
    m.insert("file_path_detection".into(), json!(s.file_path_detection));
    m.insert("editor_command".into(), json!(s.editor_command));
    m.insert("copy_on_select".into(), json!(s.copy_on_select));
    m.insert("middle_click_paste".into(), json!(s.middle_click_paste));
    m.insert(
        "shift_enter_as_alt_enter".into(),
        json!(s.shift_enter_as_alt_enter),
    );
    m.insert("skk_mode".into(), json!(s.skk_mode));
    m.insert("fold_enabled".into(), json!(s.fold_enabled));
    m.insert("clipboard_read_osc52".into(), json!(s.clipboard_read_osc52));
    m.insert(
        "clipboard_max_size_osc52".into(),
        json!(s.clipboard_max_size_osc52),
    );

    m
}

/// Clamp the numeric panel-managed fields to the WebView build's
/// `validate_settings` ranges so a value that egui's widgets failed to
/// constrain (or a programmatic caller passed) never lands on disk
/// outside what the other binary accepts.
pub fn clamp_for_save(s: &mut Settings) {
    s.font_size = s.font_size.clamp(8.0, 32.0);
    s.padding = s.padding.min(32);
    s.scrollback_lines = s.scrollback_lines.min(100_000);
    s.scroll_speed = s.scroll_speed.clamp(1, 10);
    s.clipboard_max_size_osc52 = s
        .clipboard_max_size_osc52
        .clamp(1024 * 1024, 50 * 1024 * 1024);
}

/// Save `settings` to the platform `settings.json` (the same path
/// [`crate::settings::Settings::load_or_default`] reads). Only the
/// panel-managed keys are touched; everything else on disk survives.
pub fn save(settings: &Settings) -> Result<(), String> {
    let path = crate::settings::settings_path()
        .ok_or_else(|| "settings: unable to resolve config dir".to_string())?;
    save_patch_to(&path, panel_patch(settings))
}

/// Read-modify-write `path` with `patch` applied over the existing
/// top-level keys. Missing file / unparseable JSON degrades to a fresh
/// object (the unparseable original is preserved as `settings.json.bak`
/// so a hand-edit gone wrong is recoverable). The write is atomic:
/// temp file in the same directory, then rename over the target.
pub fn save_patch_to(path: &Path, patch: Map<String, Value>) -> Result<(), String> {
    let mut root: Map<String, Value> = match std::fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
            Ok(Value::Object(map)) => map,
            Ok(_) | Err(_) => {
                // Corrupt / non-object settings.json: keep a backup so the
                // user's hand-written content is not silently destroyed,
                // then start from an empty object.
                let bak = path.with_extension("json.bak");
                if let Err(e) = std::fs::write(&bak, &bytes) {
                    log::warn!(
                        "settings: failed to back up unparseable {} to {}: {e}",
                        path.display(),
                        bak.display()
                    );
                } else {
                    log::warn!(
                        "settings: {} was not a JSON object; backed up to {}",
                        path.display(),
                        bak.display()
                    );
                }
                Map::new()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Map::new(),
        Err(e) => return Err(format!("settings: failed to read {}: {e}", path.display())),
    };

    for (k, v) in patch {
        root.insert(k, v);
    }

    let parent = path
        .parent()
        .ok_or_else(|| format!("settings: no parent dir for {}", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("settings: failed to create {}: {e}", parent.display()))?;

    let body = serde_json::to_vec_pretty(&Value::Object(root))
        .map_err(|e| format!("settings: serialize failed: {e}"))?;

    // Atomic replace: write a sibling temp file, then rename over the
    // target so a crash mid-write never leaves a truncated settings.json.
    // The temp name includes the pid so two concurrent emterm processes
    // saving at once do not clobber each other's staging file.
    let tmp = parent.join(format!(".settings.json.tmp.{}", std::process::id()));
    std::fs::write(&tmp, &body)
        .map_err(|e| format!("settings: failed to write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        // Best-effort cleanup of the orphaned temp file.
        let _ = std::fs::remove_file(&tmp);
        format!("settings: failed to replace {}: {e}", path.display())
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "emterm-settings-store-test-{}-{name}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("settings.json")
    }

    #[test]
    fn save_patch_preserves_unknown_keys() {
        let path = tmp_path("preserve");
        std::fs::write(
            &path,
            r#"{"profiles": [{"name": "p1"}], "ssh_command_path": "/usr/bin/ssh", "font_size": 11.0}"#,
        )
        .unwrap();

        let mut patch = Map::new();
        patch.insert("font_size".into(), json!(15.0));
        save_patch_to(&path, patch).unwrap();

        let v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(v["font_size"], json!(15.0));
        assert_eq!(v["profiles"][0]["name"], json!("p1"));
        assert_eq!(v["ssh_command_path"], json!("/usr/bin/ssh"));
    }

    #[test]
    fn save_patch_creates_missing_file() {
        let path = tmp_path("create");
        let _ = std::fs::remove_file(&path);

        let mut patch = Map::new();
        patch.insert("padding".into(), json!(8));
        save_patch_to(&path, patch).unwrap();

        let v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(v["padding"], json!(8));
    }

    #[test]
    fn save_patch_backs_up_corrupt_file() {
        let path = tmp_path("corrupt");
        std::fs::write(&path, b"{ not json !!").unwrap();

        let mut patch = Map::new();
        patch.insert("padding".into(), json!(4));
        save_patch_to(&path, patch).unwrap();

        let v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(v["padding"], json!(4));
        let bak = path.with_extension("json.bak");
        assert_eq!(std::fs::read(&bak).unwrap(), b"{ not json !!");
    }

    #[test]
    fn panel_patch_round_trips_through_loader() {
        // A Settings mutated by the panel, projected to flat keys, must
        // load back to the same values through the normal loader.
        let mut s = Settings::default();
        s.language = crate::settings::Language::Ja;
        s.ui_theme = crate::settings::UiTheme::Light;
        s.ui_theme_preset = crate::settings::UiThemePreset::Green;
        s.ui_font_family = "Noto Sans JP".into();
        s.font_size = 15.5;
        s.font_family_fallback = vec!["JetBrains Mono".into(), "Noto Sans JP".into()];
        s.emoji_font = Some("Twemoji".into());
        s.terminal_color_scheme = "dracula".into();
        s.bold_brightens_ansi_colors = false;
        s.padding = 12;
        s.scrollback_lines = 50_000;
        s.show_scrollbar = crate::settings::ScrollbarMode::Always;
        s.cursor_style = crate::settings::CursorStyle::Bar;
        s.cursor_blink = false;
        s.shell_path = "/bin/zsh".into();
        s.shell_args = vec!["-l".into()];
        s.scroll_speed = 7;
        s.bell_action = crate::settings::BellAction::Sound;
        s.url_detection = false;
        s.file_path_detection = false;
        s.editor_command = "vim {file}".into();
        s.copy_on_select = true;
        s.middle_click_paste = false;
        s.shift_enter_as_alt_enter = false;
        s.skk_mode = false;
        s.fold_enabled = false;
        s.clipboard_read_osc52 = false;
        s.clipboard_max_size_osc52 = 5 * 1024 * 1024;

        let path = tmp_path("roundtrip");
        let _ = std::fs::remove_file(&path);
        save_patch_to(&path, panel_patch(&s)).unwrap();

        let loaded = Settings::load_from(&path);
        assert_eq!(loaded.language, s.language);
        assert_eq!(loaded.ui_theme, s.ui_theme);
        assert_eq!(loaded.ui_theme_preset, s.ui_theme_preset);
        assert_eq!(loaded.ui_font_family, s.ui_font_family);
        assert_eq!(loaded.font_size, s.font_size);
        assert_eq!(loaded.font_family_fallback, s.font_family_fallback);
        assert_eq!(loaded.emoji_font, s.emoji_font);
        assert_eq!(loaded.terminal_color_scheme, s.terminal_color_scheme);
        assert_eq!(
            loaded.bold_brightens_ansi_colors,
            s.bold_brightens_ansi_colors
        );
        assert_eq!(loaded.padding, s.padding);
        assert_eq!(loaded.scrollback_lines, s.scrollback_lines);
        assert_eq!(loaded.show_scrollbar, s.show_scrollbar);
        assert_eq!(loaded.cursor_style, s.cursor_style);
        assert_eq!(loaded.cursor_blink, s.cursor_blink);
        assert_eq!(loaded.shell_path, s.shell_path);
        assert_eq!(loaded.shell_args, s.shell_args);
        assert_eq!(loaded.scroll_speed, s.scroll_speed);
        assert_eq!(loaded.bell_action, s.bell_action);
        assert_eq!(loaded.url_detection, s.url_detection);
        assert_eq!(loaded.file_path_detection, s.file_path_detection);
        assert_eq!(loaded.editor_command, s.editor_command);
        assert_eq!(loaded.copy_on_select, s.copy_on_select);
        assert_eq!(loaded.middle_click_paste, s.middle_click_paste);
        assert_eq!(loaded.shift_enter_as_alt_enter, s.shift_enter_as_alt_enter);
        assert_eq!(loaded.skk_mode, s.skk_mode);
        assert_eq!(loaded.fold_enabled, s.fold_enabled);
        assert_eq!(loaded.clipboard_read_osc52, s.clipboard_read_osc52);
        assert_eq!(loaded.clipboard_max_size_osc52, s.clipboard_max_size_osc52);
    }

    #[test]
    fn clamp_for_save_enforces_webview_ranges() {
        let mut s = Settings::default();
        s.font_size = 100.0;
        s.padding = 99;
        s.scrollback_lines = 1_000_000;
        s.scroll_speed = 0;
        s.clipboard_max_size_osc52 = 0;
        clamp_for_save(&mut s);
        assert_eq!(s.font_size, 32.0);
        assert_eq!(s.padding, 32);
        assert_eq!(s.scrollback_lines, 100_000);
        assert_eq!(s.scroll_speed, 1);
        assert_eq!(s.clipboard_max_size_osc52, 1024 * 1024);
    }
}
