//! CLI-shared settings primitives.
//!
//! Hosts the [`Language`] enum and [`settings_path`] resolver that the
//! CLI subcommand dispatcher (`crate::cli::active_locale`) needs before
//! any GUI subsystem boots. Keeping these out of `crate::settings` lets
//! the CLI-only build (`--no-default-features`) skip the heavy GUI
//! settings runtime entirely. The GUI build's `crate::settings` module
//! re-exports both items so existing call sites keep working.

/// UI language mirrored from the legacy WebView settings
/// (`"auto" | "en" | "ja"`). `Auto` resolves against the OS locale at
/// startup (see [`crate::i18n::resolve`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    Auto,
    En,
    Ja,
}

impl Language {
    pub fn parse_or_warn(spec: &str) -> Self {
        match spec.trim().to_ascii_lowercase().as_str() {
            "auto" | "" => Self::Auto,
            "en" => Self::En,
            "ja" => Self::Ja,
            other => {
                warn_unknown_language_once(other);
                Self::Auto
            }
        }
    }

    /// Canonical `settings.json` spelling. Inverse of
    /// [`Language::parse_or_warn`] for the settings-panel save path.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::En => "en",
            Self::Ja => "ja",
        }
    }
}

fn warn_unknown_language_once(seen: &str) {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    let owned = seen.to_string();
    ONCE.call_once(move || {
        log::warn!(
            "settings.language: unknown value {:?}, falling back to \"auto\"",
            owned
        );
    });
}

/// Resolve the `settings.json` path on the current platform. Returns
/// `None` only on unsupported targets (macOS / others); GUI callers fall
/// back to `Settings::default`.
pub fn settings_path() -> Option<std::path::PathBuf> {
    const APP_ID: &str = "net.laser5.app.emterm";

    #[cfg(target_os = "linux")]
    {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .or_else(|| {
                std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config"))
            })?;
        Some(base.join(APP_ID).join("settings.json"))
    }

    #[cfg(target_os = "windows")]
    {
        let base = std::env::var_os("APPDATA").map(std::path::PathBuf::from)?;
        Some(base.join(APP_ID).join("settings.json"))
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        None
    }
}
