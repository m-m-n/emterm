//! Tauri commands for the Linux X11/Wayland PRIMARY selection.
//!
//! PRIMARY is Linux's "select-to-copy, middle-click-to-paste" clipboard and is
//! independent from the standard CLIPBOARD used by Ctrl+C / Ctrl+V. The two
//! commands here expose read/write access to PRIMARY.
//!
//! On non-Linux platforms both commands compile down to no-ops that return
//! `Ok(())` / `Ok(String::new())`. The `arboard` dependency is
//! `cfg(target_os = "linux")`-gated in `Cargo.toml`, so Windows builds never
//! pull it in.
//!
//! ## Connection caching (Linux)
//!
//! `arboard::Clipboard::new()` opens an X11 display connection (or a Wayland
//! `wl_display` connection via `wayland-data-control`) and on X11 spawns a
//! dedicated background thread to service `SelectionRequest` events. Doing
//! this on every selection / middle-click is expensive (5–30 ms typical) and
//! creates churn in the X server's client table. We therefore keep a single
//! long-lived `Clipboard` instance behind a `Mutex` and reuse it for every
//! read and write. The mutex is initialised lazily on first use; if the
//! initial connection fails (e.g. no display server), subsequent calls will
//! retry instead of being permanently broken.

#[cfg(target_os = "linux")]
mod cache {
    use std::sync::{Mutex, OnceLock};

    use arboard::Clipboard;

    /// Single long-lived clipboard handle. Lazily populated on first
    /// successful `Clipboard::new()`. Mutex protects against concurrent
    /// Tauri command invocations from different worker threads.
    static CLIPBOARD: OnceLock<Mutex<Option<Clipboard>>> = OnceLock::new();

    /// Run `f` against the cached clipboard handle, lazily creating it if
    /// necessary. If the cached handle is missing or initialisation has not
    /// yet succeeded, attempts to create a fresh `Clipboard` first. The
    /// handle stays cached for subsequent calls so the X11/Wayland
    /// connection is reused.
    pub fn with_clipboard<F, R>(f: F) -> Result<R, String>
    where
        F: FnOnce(&mut Clipboard) -> Result<R, String>,
    {
        let mtx = CLIPBOARD.get_or_init(|| Mutex::new(None));
        let mut guard = mtx
            .lock()
            .map_err(|_| "PRIMARY clipboard mutex poisoned".to_string())?;
        if guard.is_none() {
            *guard = Some(Clipboard::new().map_err(|e| e.to_string())?);
        }
        // Safe: we just ensured Some above.
        f(guard.as_mut().expect("clipboard initialised"))
    }
}

/// Write `text` to the Linux PRIMARY selection.
///
/// On non-Linux platforms this is a no-op that always returns `Ok(())`.
///
/// # Errors
///
/// Returns an `Err(String)` on Linux when the clipboard backend (X11 or Wayland
/// via `wayland-data-control`) cannot be initialized or the write call fails.
#[cfg(feature = "gui")]
#[tauri::command]
pub async fn clipboard_write_primary(text: String) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        // Move the blocking X11/Wayland selection round-trip off the Tauri IPC
        // thread so a slow or unresponsive PRIMARY owner cannot freeze the UI.
        tokio::task::spawn_blocking(move || {
            use arboard::{LinuxClipboardKind, SetExtLinux};
            cache::with_clipboard(|clipboard| {
                clipboard
                    .set()
                    .clipboard(LinuxClipboardKind::Primary)
                    .text(text)
                    .map_err(|e| e.to_string())
            })
        })
        .await
        .map_err(|e| e.to_string())?
    }
    #[cfg(not(target_os = "linux"))]
    {
        // Silence the unused-parameter warning on non-Linux builds.
        let _ = text;
        Ok(())
    }
}

/// Read the current Linux PRIMARY selection content.
///
/// Returns an empty string when PRIMARY is empty (`ContentNotAvailable` from
/// arboard) or when the current platform is not Linux. Non-empty PRIMARY text
/// is returned verbatim.
///
/// # Errors
///
/// Returns an `Err(String)` on Linux when the clipboard backend cannot be
/// initialized or the read call fails for a reason other than "content not
/// available". Callers (in particular `pastePrimaryFirst` on the frontend)
/// must distinguish `Ok("")` (genuinely empty) from `Err(_)` (real failure)
/// because falling back to CLIPBOARD on a real PRIMARY error would defeat
/// the privacy goal of keeping PRIMARY and CLIPBOARD separate.
#[cfg(feature = "gui")]
#[tauri::command]
pub async fn clipboard_read_primary() -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        // X11 `XConvertSelection` blocks until the PRIMARY owner responds (or
        // arboard's internal timeout fires). Running this on the Tauri IPC
        // thread would stall the entire UI, so dispatch to a blocking worker.
        tokio::task::spawn_blocking(|| {
            use arboard::{GetExtLinux, LinuxClipboardKind};
            cache::with_clipboard(|clipboard| {
                match clipboard
                    .get()
                    .clipboard(LinuxClipboardKind::Primary)
                    .text()
                {
                    Ok(text) => Ok(text),
                    Err(arboard::Error::ContentNotAvailable) => Ok(String::new()),
                    Err(e) => Err(e.to_string()),
                }
            })
        })
        .await
        .map_err(|e| e.to_string())?
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(String::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// On non-Linux builds the write command must be a no-op and always return Ok.
    #[cfg(not(target_os = "linux"))]
    #[tokio::test]
    async fn write_primary_non_linux_is_noop() {
        assert!(clipboard_write_primary("hello".to_string()).await.is_ok());
        assert!(clipboard_write_primary(String::new()).await.is_ok());
    }

    /// On non-Linux builds the read command must return an empty string.
    #[cfg(not(target_os = "linux"))]
    #[tokio::test]
    async fn read_primary_non_linux_returns_empty() {
        let result = clipboard_read_primary().await;
        assert_eq!(result.unwrap(), "");
    }

    /// On Linux without a display server, the write path should surface the
    /// error as a `String` instead of panicking. We do not assert on the exact
    /// message because it varies by backend (X11, Wayland, no display).
    ///
    /// We tolerate success in environments where a display server is available
    /// (e.g. interactive Linux desktops) so the test is useful in CI headless
    /// containers and in local dev alike.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn write_primary_linux_returns_without_panicking() {
        let _ = clipboard_write_primary("test".to_string()).await;
    }

    /// Same reasoning as `write_primary_linux_returns_without_panicking`: the
    /// test only asserts that the command does not panic and returns a proper
    /// `Result`.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn read_primary_linux_returns_without_panicking() {
        let _ = clipboard_read_primary().await;
    }
}
