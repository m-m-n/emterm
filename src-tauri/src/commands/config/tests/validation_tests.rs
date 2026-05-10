//! Validation tests for `validate_settings` covering font size, scroll
//! speed, padding, and scrollback boundaries.

use super::*;

#[test]
fn test_validate_valid_settings() {
    let settings = AppSettings::default();
    assert!(validate_settings(&settings).is_ok());
}

#[test]
fn test_validate_rejects_font_size_below_min() {
    let mut settings = AppSettings::default();
    settings.font_size = 7;
    assert!(validate_settings(&settings).is_err());
}

#[test]
fn test_validate_rejects_font_size_above_max() {
    let mut settings = AppSettings::default();
    settings.font_size = 33;
    assert!(validate_settings(&settings).is_err());
}

#[test]
fn test_validate_rejects_scroll_speed_below_min() {
    let mut settings = AppSettings::default();
    settings.scroll_speed = 0;
    assert!(validate_settings(&settings).is_err());
}

#[test]
fn test_validate_rejects_scroll_speed_above_max() {
    let mut settings = AppSettings::default();
    settings.scroll_speed = 11;
    assert!(validate_settings(&settings).is_err());
}

#[test]
fn test_validate_rejects_padding_above_max() {
    let mut settings = AppSettings::default();
    settings.padding = 33;
    assert!(validate_settings(&settings).is_err());
}

#[test]
fn test_validate_rejects_scrollback_above_max() {
    let mut settings = AppSettings::default();
    settings.scrollback_lines = 100001;
    assert!(validate_settings(&settings).is_err());
}

#[test]
fn test_validate_accepts_boundary_values() {
    let mut settings = AppSettings::default();
    settings.font_size = MIN_FONT_SIZE;
    assert!(validate_settings(&settings).is_ok());

    settings.font_size = MAX_FONT_SIZE;
    assert!(validate_settings(&settings).is_ok());

    settings.scroll_speed = MIN_SCROLL_SPEED;
    assert!(validate_settings(&settings).is_ok());

    settings.scroll_speed = MAX_SCROLL_SPEED;
    assert!(validate_settings(&settings).is_ok());
}
