use font_kit::source::SystemSource;
use serde::Serialize;
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize)]
pub struct FontListResponse {
    pub monospace_fonts: Vec<String>,
    pub all_fonts: Vec<String>,
    pub emoji_fonts: Vec<String>,
}

static FONT_CACHE: OnceLock<FontListResponse> = OnceLock::new();

#[tauri::command]
pub fn list_fonts() -> Result<FontListResponse, String> {
    let response = FONT_CACHE.get_or_init(enumerate_fonts);
    Ok(response.clone())
}

fn enumerate_fonts() -> FontListResponse {
    let source = SystemSource::new();
    let families = source.all_families().unwrap_or_default();

    let mut monospace_fonts = Vec::new();
    let mut all_fonts = Vec::new();
    let mut emoji_fonts = Vec::new();

    for family_name in &families {
        all_fonts.push(family_name.clone());

        // Emoji detection: name-based heuristic
        if family_name.to_lowercase().contains("emoji") {
            emoji_fonts.push(family_name.clone());
        }

        // Monospace detection: load font and check property
        if let Ok(family) = source.select_family_by_name(family_name) {
            if let Some(font) = family.fonts().first() {
                if let Ok(font) = font.load() {
                    if font.is_monospace() {
                        monospace_fonts.push(family_name.clone());
                    }
                }
            }
        }
    }

    monospace_fonts.sort_by_key(|a| a.to_lowercase());
    all_fonts.sort_by_key(|a| a.to_lowercase());
    emoji_fonts.sort_by_key(|a| a.to_lowercase());

    monospace_fonts.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    all_fonts.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    emoji_fonts.dedup_by(|a, b| a.eq_ignore_ascii_case(b));

    FontListResponse {
        monospace_fonts,
        all_fonts,
        emoji_fonts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enumerate_fonts_returns_non_empty_all_fonts() {
        let result = enumerate_fonts();
        assert!(
            !result.all_fonts.is_empty(),
            "all_fonts should contain at least one system font"
        );
    }

    #[test]
    fn test_enumerate_fonts_all_fonts_are_sorted() {
        let result = enumerate_fonts();
        for window in result.all_fonts.windows(2) {
            assert!(
                window[0].to_lowercase() <= window[1].to_lowercase(),
                "all_fonts not sorted: '{}' should come before '{}'",
                window[0],
                window[1]
            );
        }
    }

    #[test]
    fn test_enumerate_fonts_monospace_fonts_are_sorted() {
        let result = enumerate_fonts();
        for window in result.monospace_fonts.windows(2) {
            assert!(
                window[0].to_lowercase() <= window[1].to_lowercase(),
                "monospace_fonts not sorted: '{}' should come before '{}'",
                window[0],
                window[1]
            );
        }
    }

    #[test]
    fn test_enumerate_fonts_emoji_fonts_are_sorted() {
        let result = enumerate_fonts();
        for window in result.emoji_fonts.windows(2) {
            assert!(
                window[0].to_lowercase() <= window[1].to_lowercase(),
                "emoji_fonts not sorted: '{}' should come before '{}'",
                window[0],
                window[1]
            );
        }
    }

    #[test]
    fn test_enumerate_fonts_no_duplicates_in_all_fonts() {
        let result = enumerate_fonts();
        let mut deduped = result.all_fonts.clone();
        deduped.dedup();
        assert_eq!(
            result.all_fonts.len(),
            deduped.len(),
            "all_fonts should have no duplicates"
        );
    }

    #[test]
    fn test_enumerate_fonts_no_duplicates_in_monospace_fonts() {
        let result = enumerate_fonts();
        let mut deduped = result.monospace_fonts.clone();
        deduped.dedup();
        assert_eq!(
            result.monospace_fonts.len(),
            deduped.len(),
            "monospace_fonts should have no duplicates"
        );
    }

    #[test]
    fn test_enumerate_fonts_no_duplicates_in_emoji_fonts() {
        let result = enumerate_fonts();
        let mut deduped = result.emoji_fonts.clone();
        deduped.dedup();
        assert_eq!(
            result.emoji_fonts.len(),
            deduped.len(),
            "emoji_fonts should have no duplicates"
        );
    }

    #[test]
    fn test_enumerate_fonts_monospace_is_subset_of_all() {
        let result = enumerate_fonts();
        for mono_font in &result.monospace_fonts {
            assert!(
                result.all_fonts.contains(mono_font),
                "monospace font '{}' should be in all_fonts",
                mono_font
            );
        }
    }

    #[test]
    fn test_enumerate_fonts_emoji_fonts_contain_emoji_in_name() {
        let result = enumerate_fonts();
        for emoji_font in &result.emoji_fonts {
            assert!(
                emoji_font.to_lowercase().contains("emoji"),
                "emoji font '{}' should contain 'emoji' in name",
                emoji_font
            );
        }
    }

    #[test]
    fn test_list_fonts_returns_ok() {
        // Note: OnceLock is static, so this test uses the cached value
        // if another test ran first, or populates the cache.
        let result = list_fonts();
        assert!(result.is_ok(), "list_fonts should return Ok");
    }

    #[test]
    fn test_list_fonts_cache_returns_same_result() {
        let result1 = list_fonts().unwrap();
        let result2 = list_fonts().unwrap();
        assert_eq!(
            result1.all_fonts, result2.all_fonts,
            "Cached results should be identical"
        );
        assert_eq!(
            result1.monospace_fonts, result2.monospace_fonts,
            "Cached monospace results should be identical"
        );
        assert_eq!(
            result1.emoji_fonts, result2.emoji_fonts,
            "Cached emoji results should be identical"
        );
    }
}
