//! One-shot auto-import of `~/.tmux.conf` into `settings.json`.
//!
//! Mirrors `src-tauri/src/mux/tmux_import.rs` but operates on the raw
//! `serde_json::Value` instead of the typed `AppSettings`. native-poc's
//! [`crate::settings::RawSettings`] intentionally does not model the
//! `mux.tmux_conf_imported` field —
//! it exists only to seed tmux import bookkeeping, and the renderer
//! never reads it. Going through a JSON patch lets us write that
//! key back without forcing the native loader to learn a field it does
//! not consume (the same regime [`crate::settings_store`] uses).
//!
//! The function is idempotent: once `mux.tmux_conf_imported == true` is
//! written, subsequent calls return without touching the file. The flag
//! is set even when no `~/.tmux.conf` exists, so a user who later
//! creates one does not get a surprise import.

use std::path::Path;

use serde_json::{Map, Value};

use crate::settings::settings_path;
use crate::settings_store::save_patch_to;

use super::tmux_conf::converter::{ConversionResult, convert_directives};
use super::tmux_conf::parser::parse_tmux_conf;

/// Hard cap on the size of `~/.tmux.conf` accepted by the importer. The
/// loader reads the whole file into memory and materializes every
/// directive into a `Vec<TmuxDirective>` (unsupported lines re-copy the
/// raw line text), so a pathological or accidentally-generated
/// multi-megabyte config could double-allocate enough to OOM the app
/// during startup before the latch is written. Real tmux configs are
/// well under this cap (a few hundred lines).
const TMUX_CONF_MAX_BYTES: u64 = 1024 * 1024;

/// Auto-import `~/.tmux.conf` settings on first mux startup. Reads the
/// current `settings.json`, checks the `mux.tmux_conf_imported` latch,
/// and (if unset) merges any converted settings into the `mux` object
/// before writing back atomically via [`save_patch_to`].
pub fn import_tmux_conf_if_needed() {
    let Some(path) = settings_path() else {
        return;
    };
    import_tmux_conf_into(&path, auto_import_tmux_conf);
}

/// Read `~/.tmux.conf` from the user's home directory, parse it, and
/// return the converted settings. Returns `None` when `HOME` is unset,
/// the file doesn't exist, or the file exceeds [`TMUX_CONF_MAX_BYTES`]
/// (an oversized config is logged and skipped — the importer's latch
/// still fires so the warning is not spammed every launch).
fn auto_import_tmux_conf() -> Option<ConversionResult> {
    let home = std::env::var_os("HOME")?;
    let conf_path = std::path::PathBuf::from(home).join(".tmux.conf");
    let meta = std::fs::metadata(&conf_path).ok()?;
    if !meta.is_file() {
        return None;
    }
    if meta.len() > TMUX_CONF_MAX_BYTES {
        log::warn!(
            "tmux.conf import: skipping {} ({} bytes exceeds {}-byte cap)",
            conf_path.display(),
            meta.len(),
            TMUX_CONF_MAX_BYTES
        );
        return None;
    }
    let contents = std::fs::read_to_string(&conf_path).ok()?;
    let directives = parse_tmux_conf(&contents);
    Some(convert_directives(&directives))
}

/// Path-injectable inner for testing. `loader` returns `None` when no
/// `~/.tmux.conf` exists (the WebView importer's contract), or
/// `Some(result)` with the converted settings + warnings.
pub(crate) fn import_tmux_conf_into<F>(path: &Path, loader: F)
where
    F: FnOnce() -> Option<ConversionResult>,
{
    let mut root = read_settings_object(path);

    let mut mux = match root.get("mux") {
        Some(Value::Object(map)) => map.clone(),
        _ => Map::new(),
    };

    if matches!(mux.get("tmux_conf_imported"), Some(Value::Bool(true))) {
        return;
    }

    // Latch first so a missing / empty / unreadable ~/.tmux.conf still
    // marks the import as done — matches WebView behaviour, prevents
    // retry on every launch.
    mux.insert("tmux_conf_imported".to_string(), Value::Bool(true));

    if let Some(result) = loader() {
        apply_conversion(&mut mux, &result);
        for warning in &result.warnings {
            log::warn!("tmux.conf import: {}", warning);
        }
        if !result.settings.is_empty() {
            log::info!(
                "tmux.conf: imported {} settings ({} warnings)",
                result.settings.len(),
                result.warnings.len()
            );
        }
    }

    root.insert("mux".to_string(), Value::Object(mux));

    // The latch (`mux.tmux_conf_imported = true`) is only persisted when
    // `save_patch_to` succeeds — it writes a sibling temp file and renames
    // it over the target, so a failed save leaves the on-disk
    // `settings.json` untouched. That means the next launch will re-attempt
    // the import naturally (re-derived from disk), instead of permanently
    // skipping it because of a transient disk-full / permissions error.
    // We log at `error` (not `warn`) so the user notices and can fix the
    // underlying cause before the next launch.
    if let Err(e) = save_patch_to(path, root) {
        log::error!("tmux.conf import: failed to save settings.json: {e}");
    }
}

fn read_settings_object(path: &Path) -> Map<String, Value> {
    match std::fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
            Ok(Value::Object(map)) => map,
            _ => Map::new(),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Map::new(),
        Err(e) => {
            log::warn!(
                "tmux.conf import: failed to read {}: {e}; starting from empty object",
                path.display()
            );
            Map::new()
        }
    }
}

fn apply_conversion(mux: &mut Map<String, Value>, result: &ConversionResult) {
    for (key, value) in &result.settings {
        match key.as_str() {
            "prefix" => {
                mux.insert("prefix".to_string(), Value::String(value.clone()));
            }
            k if k.starts_with("keybind.") => {
                let bind_key = k.strip_prefix("keybind.").unwrap().to_string();
                let kb = mux
                    .entry("keybinds".to_string())
                    .or_insert_with(|| Value::Object(Map::new()));
                if let Value::Object(map) = kb {
                    map.insert(bind_key, Value::String(value.clone()));
                } else {
                    // Existing non-object value at `mux.keybinds`: replace
                    // with a fresh map containing only this entry so we
                    // never silently swallow the imported binding.
                    let mut map = Map::new();
                    map.insert(bind_key, Value::String(value.clone()));
                    *kb = Value::Object(map);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tmp_settings_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "emterm-tmux-import-test-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("settings.json")
    }

    fn read_json(path: &Path) -> Value {
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    fn empty_loader() -> Option<ConversionResult> {
        None
    }

    fn loader_with(settings: Vec<(&str, &str)>) -> impl FnOnce() -> Option<ConversionResult> {
        let owned: Vec<(String, String)> = settings
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move || {
            Some(ConversionResult {
                settings: owned,
                warnings: Vec::new(),
            })
        }
    }

    #[test]
    fn missing_file_creates_settings_with_latch_only() {
        let path = tmp_settings_path("missing");
        assert!(!path.exists());
        import_tmux_conf_into(&path, empty_loader);
        let v = read_json(&path);
        assert_eq!(v["mux"]["tmux_conf_imported"], json!(true));
        // No conversion → no other mux keys should appear.
        assert!(v["mux"].get("prefix").is_none());
    }

    #[test]
    fn latch_skips_second_run() {
        let path = tmp_settings_path("latch");
        std::fs::write(&path, r#"{"mux": {"tmux_conf_imported": true}}"#).unwrap();

        // Second loader would set prefix, but the latch must suppress the
        // entire import.
        import_tmux_conf_into(&path, loader_with(vec![("prefix", "Ctrl+A")]));

        let v = read_json(&path);
        assert!(v["mux"].get("prefix").is_none());
    }

    #[test]
    fn applies_conversion_and_sets_latch() {
        let path = tmp_settings_path("apply");
        import_tmux_conf_into(
            &path,
            loader_with(vec![
                ("prefix", "Ctrl+A"),
                ("keybind.new-window", "c"),
                ("keybind.detach", "d"),
            ]),
        );

        let v = read_json(&path);
        assert_eq!(v["mux"]["tmux_conf_imported"], json!(true));
        assert_eq!(v["mux"]["prefix"], json!("Ctrl+A"));
        assert_eq!(v["mux"]["keybinds"]["new-window"], json!("c"));
        assert_eq!(v["mux"]["keybinds"]["detach"], json!("d"));
    }

    #[test]
    fn preserves_unrelated_keys() {
        let path = tmp_settings_path("preserve");
        std::fs::write(
            &path,
            r#"{
                "font_size": 15.0,
                "profiles": [{"name": "p1"}],
                "mux": {
                    "prefix": "Ctrl+X",
                    "keybinds": {"existing": "z"}
                }
            }"#,
        )
        .unwrap();

        // The loader injects a NEW keybind plus changes prefix; existing
        // sibling keys (font_size, profiles) and the existing keybind
        // must round-trip untouched.
        import_tmux_conf_into(
            &path,
            loader_with(vec![("prefix", "Ctrl+A"), ("keybind.detach", "d")]),
        );

        let v = read_json(&path);
        assert_eq!(v["font_size"], json!(15.0));
        assert_eq!(v["profiles"][0]["name"], json!("p1"));
        assert_eq!(v["mux"]["prefix"], json!("Ctrl+A"));
        assert_eq!(v["mux"]["keybinds"]["existing"], json!("z"));
        assert_eq!(v["mux"]["keybinds"]["detach"], json!("d"));
        assert_eq!(v["mux"]["tmux_conf_imported"], json!(true));
    }

    #[test]
    fn keybinds_replaces_non_object_value() {
        let path = tmp_settings_path("keybinds_nonobject");
        // Pathological: existing `mux.keybinds` is a string (shouldn't
        // happen in practice, but a hand-edited settings.json could put
        // anything there). The importer must not panic and the imported
        // binding must end up in a fresh object.
        std::fs::write(&path, r#"{"mux": {"keybinds": "broken"}}"#).unwrap();
        import_tmux_conf_into(&path, loader_with(vec![("keybind.new-window", "c")]));
        let v = read_json(&path);
        assert_eq!(v["mux"]["keybinds"]["new-window"], json!("c"));
    }

    #[test]
    fn auto_import_tmux_conf_skips_oversized_file() {
        // Synthesize a fake ~/.tmux.conf that exceeds the byte cap and
        // point HOME at the parent dir. `auto_import_tmux_conf` must
        // return None and not read the file's contents into memory.
        let dir = std::env::temp_dir().join(format!(
            "emterm-tmux-import-oversize-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let conf_path = dir.join(".tmux.conf");
        // 1 byte over the cap is enough to trip the guard.
        let oversize = vec![b'#'; (TMUX_CONF_MAX_BYTES + 1) as usize];
        std::fs::write(&conf_path, &oversize).unwrap();

        // SAFETY: tests are single-threaded for env mutation; this test
        // owns HOME for its duration. Restore on exit.
        let prev_home = std::env::var_os("HOME");
        unsafe {
            std::env::set_var("HOME", &dir);
        }
        let got = auto_import_tmux_conf();
        unsafe {
            match prev_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
        }
        assert!(got.is_none(), "oversized tmux.conf must be skipped");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
