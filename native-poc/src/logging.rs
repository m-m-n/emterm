//! `env_logger` initialization with origin tagging.
//!
//! Tagging matches the project convention (`[LEVEL][ORIGIN]`). All native-poc
//! logs use the `NATIVE-POC` origin; this keeps them distinguishable from the
//! existing Tauri build logs in mixed sessions.
//!
//! When `settings.log_recording_enabled` holds (and the build is a release
//! build), WARN/ERROR lines are additionally appended — plain text, no ANSI
//! colors — to the same `emterm.log` the legacy Tauri build writes
//! (`app_log_dir()/emterm.log`), so post-mortem analysis reads one file
//! regardless of which binary produced the line. Unlike the legacy build's
//! bare timestamps, native-poc lines carry an explicit `±HH:MM` UTC offset
//! so their zone is never ambiguous next to lines from the other process.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, Once};

static INIT: Once = Once::new();

/// Whether WARN/ERROR lines are recorded to `emterm.log`. Seeded from
/// `settings.log_recording_enabled` after the settings load in `main`.
static RECORDING_ENABLED: AtomicBool = AtomicBool::new(false);

/// Append-mode handle to `emterm.log`. Set once by [`init_log_file`]
/// (release builds); stays `None` in debug builds and on open failure.
static LOG_FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);

/// Set the recording flag (from `settings.log_recording_enabled`).
pub fn set_recording_enabled(enabled: bool) {
    RECORDING_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Open `emterm.log` in append mode. The directory mirrors the legacy
/// Tauri build's `app_log_dir()`:
/// - Linux:   `$XDG_DATA_HOME/net.laser5.app.emterm/logs`
///            (default: `$HOME/.local/share/...`)
/// - Windows: `%LOCALAPPDATA%\net.laser5.app.emterm\logs`
///
/// Failures are reported on stderr and leave file recording disabled;
/// the stderr logger is unaffected either way.
pub fn init_log_file() {
    let Some(dir) = log_dir() else {
        eprintln!("[WARN][NATIVE-POC] log file: unable to resolve log directory");
        return;
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!(
            "[WARN][NATIVE-POC] log file: failed to create {}: {e}",
            dir.display()
        );
        return;
    }
    let path = dir.join("emterm.log");
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(file) => *LOG_FILE.lock().unwrap() = Some(file),
        Err(e) => {
            eprintln!(
                "[WARN][NATIVE-POC] log file: failed to open {}: {e}",
                path.display()
            );
        }
    }
}

/// Resolve the platform log directory (see [`init_log_file`]). Returns
/// `None` on unsupported targets.
fn log_dir() -> Option<std::path::PathBuf> {
    const APP_ID: &str = "net.laser5.app.emterm";

    #[cfg(target_os = "linux")]
    {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|h| std::path::PathBuf::from(h).join(".local").join("share"))
            })?;
        Some(base.join(APP_ID).join("logs"))
    }

    #[cfg(target_os = "windows")]
    {
        let base = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from)?;
        Some(base.join(APP_ID).join("logs"))
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        None
    }
}

/// Append one plain-text line to `emterm.log`. No-op while recording is
/// disabled or the file was never opened (debug builds, open failure).
///
/// The whole line is formatted into one `String` and emitted via a
/// single `write_all` so the append is one `write()` syscall — the
/// legacy Tauri build appends to the same file from its own process,
/// and O_APPEND atomicity is only per-syscall, so a multi-write
/// `writeln!` could interleave mid-line with the other producer.
fn write_to_log_file(level: log::Level, message: &std::fmt::Arguments<'_>) {
    if !RECORDING_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let Ok(mut guard) = LOG_FILE.lock() else {
        return;
    };
    let Some(file) = guard.as_mut() else {
        return;
    };
    let line = format!("{} [{}][NATIVE-POC] {}\n", timestamp(), level, message);
    let _ = file.write_all(line.as_bytes());
    let _ = file.flush();
}

/// `YYYY-MM-DD HH:MM:SS.mmm ±HH:MM` — src-tauri's chrono-built shape
/// plus an explicit UTC offset, rendered from the crate's chrono-free
/// local-time decomposition (`crate::localtime`). The offset makes
/// each line self-describing: on the non-unix fallback the components
/// are UTC and the suffix reads `+00:00`, so native-poc lines can
/// never be mistaken for (or silently disagree with) the legacy
/// build's local-time lines in the shared file.
fn timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let ((y, mo, d, h, mi, s), off) =
        crate::localtime::local_components_and_offset(now.as_secs() as i64);
    format!(
        "{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}.{ms:03} {offset}",
        ms = now.subsec_millis(),
        offset = offset_suffix(off)
    )
}

/// Render a UTC offset in seconds as `±HH:MM` (RFC 3339 style).
fn offset_suffix(offset_secs: i32) -> String {
    let sign = if offset_secs < 0 { '-' } else { '+' };
    let abs = offset_secs.unsigned_abs();
    format!("{sign}{:02}:{:02}", abs / 3600, (abs % 3600) / 60)
}

/// Initialize the global logger. Safe to call multiple times.
///
/// Reads `RUST_LOG`. When unset, defaults to `info` for native-poc itself
/// while clamping noisy framework loggers (`wgpu*`, `naga`) to `warn` so
/// the per-frame `Device::maintain` info chatter does not flood the
/// stderr in normal runs. Users can still opt into the verbose stream
/// via `RUST_LOG=wgpu_core=info` (or similar) when debugging.
pub fn init() {
    INIT.call_once(|| {
        // Set the env-var only if the user hasn't already provided one.
        // We touch it from a single-threaded startup path (call_once on
        // the main thread), well before any other component spawns
        // threads that might also touch the environment. The intent is
        // "default filter unless the user overrode it"; users can still
        // opt back into the verbose stream with e.g.
        // `RUST_LOG=wgpu_core=info`.
        if std::env::var_os("RUST_LOG").is_none() {
            // 2024-edition note: when this crate eventually moves to
            // edition 2024 `set_var` becomes `unsafe`. Until then the
            // call is safe under the single-threaded-startup invariant
            // above.
            std::env::set_var(
                "RUST_LOG",
                "info,wgpu=warn,wgpu_core=warn,wgpu_hal=warn,naga=warn",
            );
        }
        let mut builder =
            env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"));
        builder.format(|buf, record| {
            // Release builds mirror src-tauri's `BackendLogger::log`: WARN
            // and ERROR also land in `emterm.log` when recording is on.
            if !cfg!(debug_assertions) && record.level() <= log::Level::Warn {
                write_to_log_file(record.level(), record.args());
            }
            writeln!(buf, "[{}][NATIVE-POC] {}", record.level(), record.args())
        });
        // Best-effort init; if a logger was already installed (unlikely in
        // this binary) we silently continue.
        let _ = builder.try_init();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_suffix_renders_rfc3339_style() {
        assert_eq!(offset_suffix(9 * 3600), "+09:00");
        assert_eq!(offset_suffix(0), "+00:00");
        assert_eq!(offset_suffix(-(3 * 3600 + 30 * 60)), "-03:30");
        assert_eq!(offset_suffix(5 * 3600 + 45 * 60), "+05:45");
    }

    #[test]
    fn timestamp_carries_an_explicit_offset() {
        let ts = timestamp();
        // `YYYY-MM-DD HH:MM:SS.mmm ±HH:MM` — the suffix is the part that
        // keeps shared-log lines unambiguous, so pin its shape.
        let (_, suffix) = ts.split_at(ts.len() - 6);
        assert!(
            suffix.starts_with('+') || suffix.starts_with('-'),
            "timestamp missing offset suffix: {ts}"
        );
        assert_eq!(&suffix[3..4], ":");
    }
}
