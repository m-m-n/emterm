//! Atomic patch-based persistence for `settings.json`.
//!
//! The on-disk JSON is parsed as a raw `serde_json` object, the patch's
//! top-level keys are overwritten, and the result is written back
//! atomically (temp file + rename). Keys absent from the patch
//! round-trip untouched, so writers that model only a subset of the
//! schema can never delete what they do not know about.
//!
//! Used by the child settings window's `save_settings` command
//! ([`crate::settings_window::commands`]); [`clamp_for_save`] is shared
//! with the live-apply path so a programmatic caller can not push a
//! value outside what the WebView build accepts.

use std::path::Path;

use serde_json::{Map, Value};

use crate::settings::Settings;

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
    use serde_json::json;

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
    fn app_settings_save_round_trips_through_native_loader() {
        // The child settings window saves the FULL shared schema
        // (`app_settings::AppSettings`) as a patch; the parent reloads
        // through the native loader. Every native-modeled key must
        // round-trip across the two schemas.
        let mut s = app_settings::AppSettings::default();
        s.language = "ja".into();
        s.ui_theme = app_settings::UiTheme::Light;
        s.ui_theme_preset = app_settings::UiThemePreset::Green;
        s.ui_font_family = "Noto Sans JP".into();
        s.font_size = 15;
        s.font_family_primary = "JetBrains Mono".into();
        s.font_family_secondary = "Noto Sans JP".into();
        s.font_family_emoji = "Twemoji".into();
        s.terminal_color_scheme = "dracula".into();
        s.bold_brightens_ansi_colors = false;
        s.padding = 12;
        s.scrollback_lines = 50_000;
        s.show_scrollbar = app_settings::ScrollbarMode::Always;
        s.cursor_style = app_settings::CursorStyle::Bar;
        s.cursor_blink = false;
        s.shell_path = "/bin/zsh".into();
        s.shell_args = vec!["-l".into()];
        s.scroll_speed = 7;
        s.bell_action = app_settings::BellAction::Sound;
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
        let Value::Object(patch) = serde_json::to_value(&s).unwrap() else {
            panic!("AppSettings must serialize to an object");
        };
        save_patch_to(&path, patch).unwrap();

        let loaded = Settings::load_from(&path);
        assert_eq!(loaded.language, crate::settings::Language::Ja);
        assert_eq!(loaded.ui_theme, crate::settings::UiTheme::Light);
        assert_eq!(
            loaded.ui_theme_preset,
            crate::settings::UiThemePreset::Green
        );
        assert_eq!(loaded.ui_font_family, "Noto Sans JP");
        assert_eq!(loaded.font_size, 15.0);
        assert_eq!(
            loaded.font_family_fallback,
            vec!["JetBrains Mono".to_string(), "Noto Sans JP".to_string()]
        );
        assert_eq!(loaded.emoji_font.as_deref(), Some("Twemoji"));
        assert_eq!(loaded.terminal_color_scheme, "dracula");
        assert!(!loaded.bold_brightens_ansi_colors);
        assert_eq!(loaded.padding, 12);
        assert_eq!(loaded.scrollback_lines, 50_000);
        assert_eq!(
            loaded.show_scrollbar,
            crate::settings::ScrollbarMode::Always
        );
        assert_eq!(loaded.cursor_style, crate::settings::CursorStyle::Bar);
        assert!(!loaded.cursor_blink);
        assert_eq!(loaded.shell_path, "/bin/zsh");
        assert_eq!(loaded.shell_args, vec!["-l".to_string()]);
        assert_eq!(loaded.scroll_speed, 7);
        assert_eq!(loaded.bell_action, crate::settings::BellAction::Sound);
        assert!(!loaded.url_detection);
        assert!(!loaded.file_path_detection);
        assert_eq!(loaded.editor_command, "vim {file}");
        assert!(loaded.copy_on_select);
        assert!(!loaded.middle_click_paste);
        assert!(!loaded.shift_enter_as_alt_enter);
        assert!(!loaded.skk_mode);
        assert!(!loaded.fold_enabled);
        assert!(!loaded.clipboard_read_osc52);
        assert_eq!(loaded.clipboard_max_size_osc52, 5 * 1024 * 1024);
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
