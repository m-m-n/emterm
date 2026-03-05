use rust_i18n::t;

use super::settings::AppSettings;
use super::types::*;

// ============================================================
// Validation
// ============================================================

/// Validates settings values and returns an error message if invalid.
pub(super) fn validate_settings(settings: &AppSettings) -> Result<(), String> {
    if settings.font_size < MIN_FONT_SIZE || settings.font_size > MAX_FONT_SIZE {
        return Err(t!(
            "validation.fontSize",
            min = MIN_FONT_SIZE,
            max = MAX_FONT_SIZE
        )
        .to_string());
    }

    if settings.padding > MAX_PADDING {
        return Err(t!("validation.padding", min = MIN_PADDING, max = MAX_PADDING).to_string());
    }

    if settings.scrollback_lines > MAX_SCROLLBACK_LINES {
        return Err(t!(
            "validation.scrollbackLines",
            min = MIN_SCROLLBACK_LINES,
            max = MAX_SCROLLBACK_LINES
        )
        .to_string());
    }

    if settings.scroll_speed < MIN_SCROLL_SPEED || settings.scroll_speed > MAX_SCROLL_SPEED {
        return Err(t!(
            "validation.scrollSpeed",
            min = MIN_SCROLL_SPEED,
            max = MAX_SCROLL_SPEED
        )
        .to_string());
    }

    if settings.markdown_font_size < MIN_FONT_SIZE || settings.markdown_font_size > MAX_FONT_SIZE {
        return Err(t!(
            "validation.markdownFontSize",
            min = MIN_FONT_SIZE,
            max = MAX_FONT_SIZE
        )
        .to_string());
    }

    for (i, profile) in settings.profiles.iter().enumerate() {
        if profile.name.trim().is_empty() {
            return Err(t!("validation.profileNameEmpty", index = i + 1).to_string());
        }
    }

    Ok(())
}
