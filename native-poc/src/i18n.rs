//! Minimal i18n layer: resolves the `language` setting to a concrete
//! locale at startup.
//!
//! Mirrors the legacy build's two-sided resolution:
//! - src-tauri `resolve_system_locale` (`sys-locale` + base-tag match
//!   against the supported set, English fallback)
//! - WebView `resolveLocale` (`navigator.language`-based, same rules)
//!
//! Supported locales are `en` and `ja`, identical to src-tauri's
//! `SUPPORTED_LOCALES`. Translated strings live next to their use
//! sites (e.g. `crate::notifications::notification_body`) and switch
//! on the [`Locale`] resolved here.

use crate::settings::Language;

/// Concrete display locale, resolved once at startup from
/// [`Language`]. Unlike `Language` there is no `Auto` — resolution has
/// already consulted the OS locale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Locale {
    #[default]
    En,
    Ja,
}

/// Resolve the `language` setting to a [`Locale`]. `Auto` consults the
/// OS locale via `sys-locale`; unsupported / undetectable locales fall
/// back to English (matching src-tauri's `resolve_system_locale`).
pub fn resolve(language: Language) -> Locale {
    match language {
        Language::En => Locale::En,
        Language::Ja => Locale::Ja,
        Language::Auto => locale_from_tag(&sys_locale::get_locale().unwrap_or_default()),
    }
}

/// Map a BCP 47 / POSIX locale tag (`"ja-JP"`, `"ja_JP.UTF-8"`, …) to a
/// supported [`Locale`] by its base language code. Unsupported bases
/// fall back to English.
pub fn locale_from_tag(tag: &str) -> Locale {
    let base = tag.split(['-', '_', '.']).next().unwrap_or("");
    match base.to_ascii_lowercase().as_str() {
        "ja" => Locale::Ja,
        _ => Locale::En,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_languages_bypass_system_lookup() {
        assert_eq!(resolve(Language::En), Locale::En);
        assert_eq!(resolve(Language::Ja), Locale::Ja);
    }

    #[test]
    fn bcp47_tags_resolve_by_base_language() {
        assert_eq!(locale_from_tag("ja-JP"), Locale::Ja);
        assert_eq!(locale_from_tag("en-US"), Locale::En);
    }

    #[test]
    fn posix_tags_resolve_by_base_language() {
        assert_eq!(locale_from_tag("ja_JP.UTF-8"), Locale::Ja);
        assert_eq!(locale_from_tag("en_GB"), Locale::En);
    }

    #[test]
    fn unsupported_and_empty_tags_fall_back_to_english() {
        assert_eq!(locale_from_tag("fr-FR"), Locale::En);
        assert_eq!(locale_from_tag(""), Locale::En);
    }

    #[test]
    fn base_tag_match_is_case_insensitive() {
        assert_eq!(locale_from_tag("JA-JP"), Locale::Ja);
    }
}
