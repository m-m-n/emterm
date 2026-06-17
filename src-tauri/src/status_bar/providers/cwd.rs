//! `{cwd}` provider.
//!
//! Reads the active tab's current working directory from
//! `NativeCallbackState::cwd` (populated by OSC 7) via a caller-
//! supplied closure, then renders the basename so the status bar
//! shows the directory name rather than a long absolute path.
//!
//! Path conventions handled:
//! - Unix paths (`/home/me`)
//! - Windows paths (`C:\Users\me`)
//! - `file://` URIs (`file:///home/me`, `file://host/home/me`)
//! - Percent-encoded segments inside `file://` URIs

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex;

use crate::html::CssColor;
use crate::status_bar::template_engine::VariableProvider;
use crate::wakeup::WakeFn;

/// Closure that returns the current cwd. The runtime sets this to a
/// snapshot reader over the active tab's `NativeCallbackState`.
pub type CwdSource = Arc<dyn Fn() -> Option<String> + Send + Sync>;

pub struct CwdProvider {
    source: CwdSource,
    last: Mutex<Option<String>>,
    version: AtomicU64,
    /// Wake handle invoked from [`Self::set_cwd`] when a new cwd
    /// arrives via OSC 7. The CwdProvider has no polling thread of
    /// its own — wakes are event-driven. `None` retains the legacy
    /// "no wake" behaviour for unit tests that only exercise
    /// [`VariableProvider::get_value`].
    wake: Option<WakeFn>,
}

impl CwdProvider {
    /// Construct without a wake handle. Retained for unit tests.
    pub fn new(source: CwdSource) -> Self {
        Self {
            source,
            last: Mutex::new(None),
            version: AtomicU64::new(0),
            wake: None,
        }
    }

    /// Construct with a wake handle. Calling [`Self::set_cwd`] later
    /// will invoke `wake` so the egui frame schedules a redraw.
    pub fn with_wake(source: CwdSource, wake: WakeFn) -> Self {
        Self {
            source,
            last: Mutex::new(None),
            version: AtomicU64::new(0),
            wake: Some(wake),
        }
    }

    /// Notify the provider that the active tab's cwd changed.
    ///
    /// Bumps the version counter (so Phase F's run-list cache
    /// invalidates) and invokes the installed [`WakeFn`] so the egui
    /// frame schedules a redraw. Callers are typically the OSC 7
    /// route in `NativeCallbacks::handle_cwd`, which holds the
    /// runtime-provided clone of the provider.
    pub fn set_cwd(&self, cwd: Option<&str>) {
        let new_basename = cwd.map(basename).unwrap_or_default();
        let mut last = self.last.lock().unwrap();
        if last.as_deref() != Some(new_basename.as_str()) {
            *last = Some(new_basename);
            drop(last);
            self.version.fetch_add(1, Ordering::Relaxed);
            if let Some(w) = &self.wake {
                w();
            }
        }
    }
}

impl VariableProvider for CwdProvider {
    fn name(&self) -> &str {
        "cwd"
    }

    fn get_value(&self, _argument: Option<&str>) -> String {
        let raw = (self.source)();
        let resolved = raw.as_deref().map(basename).unwrap_or_default();
        // Bump version when value changes.
        let mut last = self.last.lock().unwrap();
        if last.as_deref() != Some(resolved.as_str()) {
            *last = Some(resolved.clone());
            self.version.fetch_add(1, Ordering::Relaxed);
        }
        resolved
    }

    fn get_color(&self, _argument: Option<&str>) -> Option<CssColor> {
        None
    }

    fn version(&self, _argument: Option<&str>) -> u64 {
        self.version.load(Ordering::Relaxed)
    }
}

/// Extract the trailing path segment from a raw OSC 7 string.
pub fn basename(raw: &str) -> String {
    let s = raw.trim();
    if s.is_empty() {
        return String::new();
    }
    // file:// URI handling. Drop the optional host component, then
    // pull the basename out of the path.
    let path = if let Some(rest) = s.strip_prefix("file://") {
        // file://[host]/path → strip the host segment if present.
        match rest.find('/') {
            Some(idx) => &rest[idx..],
            None => rest,
        }
    } else {
        s
    };
    // Percent-decode the path before splitting; we need to handle
    // `%20` inside path segments.
    let decoded = percent_decode(path);
    let trimmed = decoded.trim_end_matches(|c| c == '/' || c == '\\');
    if trimmed.is_empty() {
        // Whole path was separators (e.g. `/`). Return the root marker.
        return "/".to_string();
    }
    // Find the last separator.
    let last = trimmed.rfind(|c| c == '/' || c == '\\');
    match last {
        Some(idx) => trimmed[idx + 1..].to_string(),
        None => {
            // No separator at all → maybe a `C:` drive or a bare
            // name. Return the whole thing minus the trailing colon.
            trimmed.trim_end_matches(':').to_string()
        }
    }
}

/// Minimal `%XX` decoder for ASCII bytes. Multi-byte UTF-8 escape
/// sequences are reassembled if both bytes decode successfully;
/// malformed escapes are passed through verbatim.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let len = bytes.len();
    while i < len {
        if bytes[i] == b'%' && i + 2 < len {
            let hi = hex_digit(bytes[i + 1]);
            let lo = hex_digit(bytes[i + 2]);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(&e.into_bytes()).into_owned())
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_unix_path() {
        assert_eq!(basename("/home/me/projects"), "projects");
        assert_eq!(basename("/home/me/projects/"), "projects");
    }

    #[test]
    fn basename_root_path() {
        assert_eq!(basename("/"), "/");
    }

    #[test]
    fn basename_windows_path() {
        assert_eq!(basename("C:\\Users\\me\\Projects"), "Projects");
        // Drive root `D:\` after trimming separators is `D:`, which we
        // surface as the bare drive letter. Callers can disambiguate
        // by checking for a single-char result if they care.
        assert_eq!(basename("D:\\"), "D");
    }

    #[test]
    fn basename_file_uri_with_host() {
        assert_eq!(basename("file://host01/home/me/repo"), "repo");
    }

    #[test]
    fn basename_file_uri_no_host() {
        assert_eq!(basename("file:///home/me/repo"), "repo");
    }

    #[test]
    fn basename_percent_decoded_segments() {
        assert_eq!(basename("file:///home/me/my%20repo"), "my repo");
    }

    #[test]
    fn basename_empty_input() {
        assert_eq!(basename(""), "");
    }

    #[test]
    fn basename_bare_name_returns_name() {
        assert_eq!(basename("hello"), "hello");
    }

    #[test]
    fn provider_returns_basename_via_closure() {
        let source: CwdSource = Arc::new(|| Some("/home/me/repo".to_string()));
        let p = CwdProvider::new(source);
        assert_eq!(p.get_value(None), "repo");
    }

    #[test]
    fn provider_bumps_version_on_change() {
        use std::sync::{Arc, Mutex};
        let store: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let store_clone = store.clone();
        let source: CwdSource = Arc::new(move || store_clone.lock().unwrap().clone());
        let p = CwdProvider::new(source);
        assert_eq!(p.get_value(None), "");
        let v0 = p.version(None);
        *store.lock().unwrap() = Some("/a".to_string());
        let _ = p.get_value(None);
        assert!(p.version(None) > v0);
    }

    // ── Event-driven wake on OSC 7 (provider-ownership refresh) ─

    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrd};

    #[test]
    fn set_cwd_with_wake_invokes_wake_on_change() {
        let count = Arc::new(AtomicUsize::new(0));
        let c2 = count.clone();
        let wake: crate::wakeup::WakeFn = Arc::new(move || {
            c2.fetch_add(1, AtomicOrd::Relaxed);
        });
        let source: CwdSource = Arc::new(|| None);
        let p = CwdProvider::with_wake(source, wake);
        let v0 = p.version(None);
        p.set_cwd(Some("/home/me/repo"));
        assert!(p.version(None) > v0);
        assert_eq!(count.load(AtomicOrd::Relaxed), 1);
    }

    #[test]
    fn set_cwd_is_idempotent_no_wake_when_unchanged() {
        let count = Arc::new(AtomicUsize::new(0));
        let c2 = count.clone();
        let wake: crate::wakeup::WakeFn = Arc::new(move || {
            c2.fetch_add(1, AtomicOrd::Relaxed);
        });
        let source: CwdSource = Arc::new(|| None);
        let p = CwdProvider::with_wake(source, wake);
        p.set_cwd(Some("/home/me/repo"));
        p.set_cwd(Some("/home/me/repo"));
        // Only the first call should fire wake.
        assert_eq!(count.load(AtomicOrd::Relaxed), 1);
    }

    #[test]
    fn set_cwd_without_wake_is_safe() {
        // Pure `new()` (no wake) must accept `set_cwd` without panicking.
        let source: CwdSource = Arc::new(|| None);
        let p = CwdProvider::new(source);
        p.set_cwd(Some("/anywhere"));
        // Version still advances so Phase F cache invalidates.
        assert!(p.version(None) >= 1);
    }
}
