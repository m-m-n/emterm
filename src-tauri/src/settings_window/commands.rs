//! Backend command handlers for the child settings window.
//!
//! The reused WebView settings panel calls Tauri commands through the
//! injected invoke bridge; this module implements them natively. Handlers
//! are pure functions over `serde_json::Value` so the dispatch (and the
//! save round-trip) is unit-testable without a window.
//!
//! `load_settings` / `save_settings` operate on the **full**
//! [`app_settings::AppSettings`] schema (shared with the Tauri build), so
//! the panel sees the same defaults-merged view of `settings.json` in both
//! binaries. Saves go through [`crate::settings_store::save_patch_to`] —
//! an atomic read-modify-write that preserves unknown top-level keys.

use std::path::PathBuf;

use app_settings::AppSettings;
use serde_json::{Map, Value, json};

/// Result of one dispatched command: the JSON reply plus whether the
/// command persisted `settings.json` (the caller notifies the parent
/// terminal process so it can reload + apply).
pub struct CommandOutcome {
    pub result: Result<Value, String>,
    pub saved: bool,
}

impl CommandOutcome {
    fn reply(result: Result<Value, String>) -> Self {
        Self {
            result,
            saved: false,
        }
    }
}

/// Dispatch one `{cmd, args}` invoke call from the panel.
pub fn handle(cmd: &str, args: &Value) -> CommandOutcome {
    match cmd {
        "load_settings" => CommandOutcome::reply(load_settings()),
        "save_settings" => {
            let result = save_settings(args);
            CommandOutcome {
                saved: result.is_ok(),
                result,
            }
        }
        "list_fonts" => CommandOutcome::reply(list_fonts()),
        "get_mux_action_defaults" => CommandOutcome::reply(Ok(mux_action_defaults())),
        "get_platform" => CommandOutcome::reply(Ok(json!(platform_token()))),
        "plugin:app|version" => CommandOutcome::reply(Ok(json!(env!("CARGO_PKG_VERSION")))),
        // The panel's language selector switches its own locale client-side;
        // the parent picks the persisted language up on the save that
        // follows. Nothing to do in the child.
        "set_language" => CommandOutcome::reply(Ok(Value::Null)),
        "detect_ssh_command" => CommandOutcome::reply(detect_ssh_command()),
        // SSH config browsing is not implemented in the native build yet;
        // an empty host list renders the section without suggestions.
        "load_ssh_config_hosts" => CommandOutcome::reply(Ok(json!([]))),
        // Recording on/off is persisted via the regular settings save; the
        // child itself records nothing.
        "set_log_recording" => CommandOutcome::reply(Ok(Value::Null)),
        "get_log_path" => CommandOutcome::reply(Ok(log_path()
            .map(|p| json!(p.to_string_lossy()))
            .unwrap_or(Value::Null))),
        "get_log_tail" => CommandOutcome::reply(log_tail(args)),
        "clear_log" => CommandOutcome::reply(clear_log()),
        other => CommandOutcome::reply(Err(format!("settings window: unknown command {other:?}"))),
    }
}

/// Load `settings.json` with the shared schema's serde defaults (the same
/// view the Tauri build's `load_settings` command produces), including the
/// legacy `font_family` → `font_family_primary` migration.
///
/// When `apply_migrations()` reports that any legacy key was rewritten,
/// we persist a one-time `.bak` snapshot of the existing file and
/// atomically rewrite `settings.json` with the new schema (SPEC FR7 /
/// OQ3 resolution). The `.bak` is best-effort: a failure to write it
/// is logged at `warn` but does NOT block the rewrite itself (the
/// migrated values would otherwise be discarded on every launch).
fn load_settings() -> Result<Value, String> {
    let path = crate::settings::settings_path();
    let original_bytes: Option<Vec<u8>> = match &path {
        Some(p) => match std::fs::read(p) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                log::warn!("settings window: failed to read {}: {e}", p.display());
                None
            }
        },
        None => None,
    };
    let mut settings = match &original_bytes {
        Some(bytes) => serde_json::from_slice::<AppSettings>(bytes).unwrap_or_else(|e| {
            log::warn!(
                "settings window: failed to parse {}: {e}",
                path.as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default()
            );
            AppSettings::default()
        }),
        None => AppSettings::default(),
    };

    let migrated = settings.apply_migrations();
    if migrated {
        if let (Some(path), Some(bytes)) = (path.as_ref(), original_bytes.as_ref()) {
            persist_migrated_settings(path, bytes, &settings);
        }
    }

    serde_json::to_value(&settings).map_err(|e| format!("settings window: serialize failed: {e}"))
}

/// Legacy top-level keys consumed by the migration. They must be
/// stripped from the rewritten `settings.json` so a subsequent load
/// does not re-trigger the migration on every launch (the round-trip
/// would otherwise leave a stale `.bak` rewrite each time).
const LEGACY_KEYS_REMOVED_AFTER_MIGRATION: &[&str] = &["font_family"];

/// Persist a migrated `settings.json`:
/// 1. Best-effort copy of the existing bytes to `settings.json.bak`.
///    Logs a warn and continues on failure.
/// 2. Serialize the migrated `AppSettings` and write it via the same
///    atomic-write path used by `save_settings` (so unknown top-level
///    keys are preserved across the rewrite). Legacy top-level keys
///    that the migration consumed are explicitly removed from the
///    rewritten file so the second load is a true no-op.
fn persist_migrated_settings(
    path: &std::path::Path,
    original_bytes: &[u8],
    settings: &AppSettings,
) {
    // 1. .bak snapshot. Skip when the existing file is empty (no point
    //    backing up zero bytes — and `.bak` only makes sense when the
    //    user had content worth preserving).
    if !original_bytes.is_empty() {
        let bak = path.with_extension("json.bak");
        if let Err(e) = std::fs::write(&bak, original_bytes) {
            log::warn!(
                "settings.migrate.failed_persist: backup write failed at {}: {e}",
                bak.display()
            );
        }
    }
    // 2. Re-serialize the migrated struct as a patch.
    let serialized = match serde_json::to_value(settings) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("settings.migrate.failed_persist: serialize failed: {e}");
            return;
        }
    };
    let Value::Object(mut patch) = serialized else {
        log::warn!(
            "settings.migrate.failed_persist: migrated settings did not serialize to an object"
        );
        return;
    };
    // Tag the legacy keys as `Value::Null` so `save_patch_to`'s
    // read-modify-write overwrites the existing string values with
    // `null`, then drop them from the rewritten root immediately
    // afterwards. We can't do this through `save_patch_to` alone
    // (it only knows how to overwrite, not remove keys), so we
    // do the read-modify-write inline.
    for key in LEGACY_KEYS_REMOVED_AFTER_MIGRATION {
        patch.remove(*key);
    }

    let mut root: Map<String, Value> = match std::fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
            Ok(Value::Object(map)) => map,
            _ => Map::new(),
        },
        Err(_) => Map::new(),
    };
    for key in LEGACY_KEYS_REMOVED_AFTER_MIGRATION {
        root.remove(*key);
    }
    for (k, v) in patch {
        root.insert(k, v);
    }

    let body = match serde_json::to_vec_pretty(&Value::Object(root)) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("settings.migrate.failed_persist: serialize failed: {e}");
            return;
        }
    };

    let parent = match path.parent() {
        Some(p) => p,
        None => {
            log::warn!(
                "settings.migrate.failed_persist: no parent dir for {}",
                path.display()
            );
            return;
        }
    };
    if let Err(e) = std::fs::create_dir_all(parent) {
        log::warn!(
            "settings.migrate.failed_persist: create dir {} failed: {e}",
            parent.display()
        );
        return;
    }
    let tmp = parent.join(format!(".settings.json.tmp.{}", std::process::id()));
    if let Err(e) = std::fs::write(&tmp, &body) {
        log::warn!(
            "settings.migrate.failed_persist: write {} failed: {e}",
            tmp.display()
        );
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        log::warn!(
            "settings.migrate.failed_persist: rename to {} failed: {e}",
            path.display()
        );
    }
}

/// Default mux action keybindings as an ordered `[{ action, key }]` list,
/// derived from the Rust SSOT [`crate::mux::prefix::DEFAULT_ACTION_BINDINGS`].
/// The settings panel reads these instead of duplicating the table in
/// TypeScript, so the displayed defaults can never drift from the runtime
/// authority and unset actions show their real default chord.
fn mux_action_defaults() -> Value {
    let list: Vec<Value> = crate::mux::prefix::default_action_bindings_as_strings()
        .into_iter()
        .map(|(action, key)| json!({ "action": action, "key": key }))
        .collect();
    Value::Array(list)
}

/// Persist the panel's full `AppSettings`. The struct round-trip applies
/// the schema's null-tolerant deserialization; the atomic patch write
/// preserves any top-level keys the schema does not model.
fn save_settings(args: &Value) -> Result<Value, String> {
    let raw = args
        .get("settings")
        .ok_or_else(|| "settings window: save_settings missing `settings` arg".to_string())?;
    let settings: AppSettings = serde_json::from_value(raw.clone())
        .map_err(|e| format!("settings window: invalid settings payload: {e}"))?;
    let serialized = serde_json::to_value(&settings)
        .map_err(|e| format!("settings window: serialize failed: {e}"))?;
    let Value::Object(patch) = serialized else {
        return Err("settings window: settings did not serialize to an object".to_string());
    };
    let path = crate::settings::settings_path()
        .ok_or_else(|| "settings window: unable to resolve config dir".to_string())?;
    save_patch(&path, patch)?;
    Ok(Value::Null)
}

/// Atomic merge-write, separated for tests (`save_patch_to` is the shared
/// implementation used by the legacy panel save path too).
fn save_patch(path: &std::path::Path, patch: Map<String, Value>) -> Result<(), String> {
    crate::settings_store::save_patch_to(path, patch)
}

/// Enumerate system font families via fontdb, shaped like the Tauri
/// build's `list_fonts` response (`FontListResponse`).
///
/// The result is cached for the process lifetime via a `OnceLock`; font
/// families do not change while the settings window is open, and
/// `fontdb::Database::load_system_fonts` can take hundreds of milliseconds
/// on systems with many (or NFS-mounted) font paths.
fn list_fonts() -> Result<Value, String> {
    use std::sync::OnceLock;
    static CACHE: OnceLock<Value> = OnceLock::new();
    Ok(CACHE.get_or_init(build_font_list).clone())
}

fn build_font_list() -> Value {
    use std::collections::BTreeMap;

    let mut db = fontdb::Database::new();
    db.load_system_fonts();

    // family name (first/typographic) → is any face monospaced
    let mut families: BTreeMap<String, bool> = BTreeMap::new();
    for face in db.faces() {
        let Some((name, _)) = face.families.first() else {
            continue;
        };
        let mono = families.entry(name.clone()).or_insert(false);
        *mono = *mono || face.monospaced;
    }

    let mut all_fonts: Vec<String> = Vec::new();
    let mut monospace_fonts: Vec<String> = Vec::new();
    let mut emoji_fonts: Vec<String> = Vec::new();
    for (name, mono) in &families {
        all_fonts.push(name.clone());
        if *mono {
            monospace_fonts.push(name.clone());
        }
        if name.to_lowercase().contains("emoji") {
            emoji_fonts.push(name.clone());
        }
    }
    for list in [&mut all_fonts, &mut monospace_fonts, &mut emoji_fonts] {
        list.sort_by_key(|a| a.to_lowercase());
        list.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    }

    json!({
        "monospace_fonts": monospace_fonts,
        "all_fonts": all_fonts,
        "emoji_fonts": emoji_fonts,
    })
}

fn platform_token() -> &'static str {
    if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "unknown"
    }
}

/// Find `ssh` on PATH (the Tauri build's `detect_ssh_command`).
fn detect_ssh_command() -> Result<Value, String> {
    let exe = if cfg!(windows) { "ssh.exe" } else { "ssh" };
    let paths = std::env::var_os("PATH").ok_or_else(|| "PATH is not set".to_string())?;
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(exe);
        if candidate.is_file() {
            return Ok(json!(candidate.to_string_lossy()));
        }
    }
    Err("ssh command not found on PATH".to_string())
}

/// The shared `emterm.log` path (same file the parent and the Tauri build
/// append to). `None` when the platform data dir cannot be resolved.
fn log_path() -> Option<PathBuf> {
    crate::logging::log_dir().map(|d| d.join("emterm.log"))
}

/// Return the last `lines` lines of `emterm.log` as one string.
fn log_tail(args: &Value) -> Result<Value, String> {
    let lines = args.get("lines").and_then(Value::as_u64).unwrap_or(500) as usize;
    let path = log_path().ok_or_else(|| "log path unavailable".to_string())?;
    use std::io::{Read as _, Seek as _, SeekFrom};
    // Cap reads at 4 MB to bound the allocation regardless of log size.
    const MAX_READ: u64 = 4 * 1024 * 1024;
    let mut f = std::fs::File::open(&path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    if len > MAX_READ {
        f.seek(SeekFrom::End(-(MAX_READ as i64))).ok();
    }
    let mut buf = Vec::with_capacity(MAX_READ.min(len.max(1)) as usize);
    f.read_to_end(&mut buf)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    // Lossy decode: a seek into the middle of a multi-byte UTF-8 character
    // must not fail the whole tail read.
    let text = String::from_utf8_lossy(&buf);
    let all: Vec<&str> = text.lines().collect();
    let start = all.len().saturating_sub(lines);
    Ok(json!(all[start..].join("\n")))
}

/// Truncate `emterm.log` (the Tauri build's `clear_log`).
fn clear_log() -> Result<Value, String> {
    let path = log_path().ok_or_else(|| "log path unavailable".to_string())?;
    match std::fs::File::create(&path) {
        Ok(_) => Ok(Value::Null),
        Err(e) => Err(format!("failed to clear {}: {e}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_command_is_an_error_without_save() {
        let out = handle("nope", &Value::Null);
        assert!(out.result.is_err());
        assert!(!out.saved);
    }

    #[test]
    fn get_platform_returns_a_known_token() {
        let out = handle("get_platform", &Value::Null);
        let v = out.result.unwrap();
        assert!(matches!(
            v.as_str().unwrap(),
            "linux" | "windows" | "macos" | "unknown"
        ));
    }

    #[test]
    fn version_command_matches_crate_version() {
        let out = handle("plugin:app|version", &Value::Null);
        assert_eq!(out.result.unwrap(), json!(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn set_language_and_log_recording_are_null_no_ops() {
        for cmd in ["set_language", "set_log_recording"] {
            let out = handle(cmd, &json!({"language": "ja", "enabled": true}));
            assert_eq!(out.result.unwrap(), Value::Null);
            assert!(!out.saved);
        }
    }

    #[test]
    fn save_settings_requires_settings_arg() {
        let out = handle("save_settings", &json!({}));
        assert!(out.result.is_err());
        assert!(!out.saved);
    }

    #[test]
    fn save_settings_rejects_non_object_payload() {
        let out = handle("save_settings", &json!({"settings": 42}));
        assert!(out.result.is_err());
        assert!(!out.saved);
    }

    #[test]
    fn list_fonts_has_the_response_shape() {
        let v = list_fonts().unwrap();
        assert!(v.get("monospace_fonts").unwrap().is_array());
        assert!(v.get("all_fonts").unwrap().is_array());
        assert!(v.get("emoji_fonts").unwrap().is_array());
    }

    #[test]
    fn get_mux_action_defaults_returns_ordered_action_key_list() {
        let out = handle("get_mux_action_defaults", &Value::Null);
        let v = out.result.unwrap();
        let arr = v.as_array().expect("array");
        // Mirrors the Rust SSOT order/values (crate::mux::prefix).
        assert_eq!(arr.len(), 6);
        assert_eq!(arr[0], json!({ "action": "detach", "key": "Ctrl+D" }));
        assert_eq!(arr[5], json!({ "action": "move-window", "key": "Ctrl+T" }));
        assert!(!out.saved);
    }

    #[test]
    fn app_settings_full_roundtrip_through_patch_save() {
        // A defaults-built AppSettings serialized as a patch and merged over
        // an existing file must preserve unknown keys and round-trip every
        // schema key.
        let dir = std::env::temp_dir().join(format!(
            "emterm-settings-window-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(&path, r#"{"some_future_key": {"x": 1}, "font_size": 20}"#).unwrap();

        let settings = AppSettings::default();
        let Value::Object(patch) = serde_json::to_value(&settings).unwrap() else {
            panic!("AppSettings must serialize to an object");
        };
        save_patch(&path, patch).unwrap();

        let v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        // Unknown key survives the save.
        assert_eq!(v["some_future_key"]["x"], json!(1));
        // Schema keys come from the saved struct (default font_size wins
        // over the stale 20 — the panel always saves its full state).
        assert_eq!(
            v["font_size"],
            serde_json::to_value(&settings).unwrap()["font_size"]
        );
        // And the file parses back into the schema.
        let back: AppSettings = serde_json::from_value(v).unwrap();
        assert_eq!(back.font_size, settings.font_size);
    }

    /// Loading a settings file that carries the legacy `font_family`
    /// key must migrate it to `font_family_primary` and rewrite the
    /// file with the new schema (one-time `.bak` snapshot). Reloading
    /// the rewritten file is a no-op and must NOT produce a second
    /// `.bak`.
    #[test]
    fn legacy_font_family_persists_with_bak() {
        let dir = std::env::temp_dir().join(format!(
            "emterm-settings-window-migrate-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        let bak = path.with_extension("json.bak");
        let original = br#"{"font_family": "Legacy Mono"}"#;
        std::fs::write(&path, original).unwrap();

        let original_bytes = std::fs::read(&path).unwrap();
        let mut settings: AppSettings = serde_json::from_slice(&original_bytes).unwrap();
        let migrated = settings.apply_migrations();
        assert!(migrated, "legacy key must trigger a migration");
        persist_migrated_settings(&path, &original_bytes, &settings);

        // The .bak captures the pre-migration bytes exactly.
        let bak_bytes = std::fs::read(&bak).expect(".bak must exist");
        assert_eq!(
            bak_bytes.as_slice(),
            original,
            ".bak must equal the pre-migration file bytes"
        );

        // The new file carries the migrated key under the new name.
        let v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            v["font_family_primary"], "Legacy Mono",
            "legacy font_family must migrate into font_family_primary"
        );
        assert!(
            v.get("font_family").is_none(),
            "legacy font_family key must be stripped after rewrite"
        );

        // Reloading the rewritten file should be idempotent and must
        // NOT clobber `.bak` again.
        let bak_aside = path.with_extension("json.bak.aside");
        std::fs::rename(&bak, &bak_aside).unwrap();
        let reloaded_bytes = std::fs::read(&path).unwrap();
        let mut reloaded: AppSettings = serde_json::from_slice(&reloaded_bytes).unwrap();
        let reloaded_migrated = reloaded.apply_migrations();
        assert!(
            !reloaded_migrated,
            "second load must not re-migrate (idempotent)"
        );
        assert!(
            !bak.exists(),
            "second load must NOT regenerate {}",
            bak.display()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
