//! Drift-detection + kind-rule introspection + OK-label rejection tests.
//!
//! These tests guard the contract between
//! `doc/UI-DESIGN-GUIDELINES.yaml`, the Rust [`super::tokens`]
//! constants, and the `--md-sys-color-*` variables in
//! `src-tauri/web-shared/styles.css`. They run under the standard
//! `cargo test --lib` invocation and are GUI-gated together with the
//! parent module.

use std::collections::BTreeSet;

use serde::Deserialize;

use super::{Dialog, DialogKind, kinds, tokens};

const YAML_SRC: &str = include_str!("../../../../doc/UI-DESIGN-GUIDELINES.yaml");
const STYLES_CSS_SRC: &str = include_str!("../../../web-shared/styles.css");
const DIALOG_CSS_SRC: &str = include_str!("../../../web-shared/dialog/dialog-shell.css");

/// Tolerant projection of the yaml: only the fields the tests inspect
/// are modeled. `#[serde(deny_unknown_fields)]` is intentionally NOT
/// used so the yaml can grow without breaking the tests.
#[derive(Debug, Deserialize)]
struct YamlRoot {
    tokens: YamlTokens,
    dialogs: YamlDialogs,
    #[serde(rename = "known-issues")]
    known_issues: serde_yml::Value,
}

#[derive(Debug, Deserialize)]
struct YamlTokens {
    #[serde(rename = "color-roles")]
    color_roles: std::collections::BTreeMap<String, String>,
    elevation: YamlElevation,
    typography: YamlTypography,
}

#[derive(Debug, Deserialize)]
struct YamlElevation {
    #[serde(rename = "elevation-3")]
    elevation_3: YamlElevationLevel,
}

#[derive(Debug, Deserialize)]
struct YamlElevationLevel {
    #[serde(rename = "box-shadow")]
    box_shadow: String,
}

#[derive(Debug, Deserialize)]
struct YamlTypography {
    #[serde(rename = "title-large")]
    title_large: YamlTypeScale,
    #[serde(rename = "body-medium")]
    body_medium: YamlTypeScale,
}

#[derive(Debug, Deserialize)]
struct YamlTypeScale {
    #[serde(rename = "font-size")]
    font_size: String,
}

#[derive(Debug, Deserialize)]
struct YamlDialogs {
    scrim: String,
    layout: YamlDialogLayout,
}

#[derive(Debug, Deserialize)]
struct YamlDialogLayout {
    #[serde(rename = "corner-radius")]
    corner_radius: String,
    padding: String,
    #[serde(rename = "width-standard")]
    width_standard: String,
    #[serde(rename = "width-compact")]
    width_compact: String,
    #[serde(rename = "max-height-standard")]
    max_height_standard: String,
    #[serde(rename = "max-height-compact")]
    max_height_compact: String,
    #[serde(rename = "actions-gap")]
    actions_gap: String,
    #[serde(rename = "title-to-body-margin")]
    title_to_body_margin: String,
    #[serde(rename = "actions-top-margin")]
    actions_top_margin: String,
    #[serde(rename = "body-item-spacing")]
    body_item_spacing: String,
}

fn parse_yaml() -> YamlRoot {
    serde_yml::from_str(YAML_SRC).expect("UI-DESIGN-GUIDELINES.yaml deserialized")
}

/// Walk `serde_yml::Value` by a key path and return the leaf as a
/// `String`. Used for drift assertions on nested yaml branches that
/// aren't worth modeling as full `serde::Deserialize` types.
fn yaml_lookup(value: &serde_yml::Value, path: &[&str]) -> String {
    let mut current = value;
    for key in path {
        current = current.get(*key).unwrap_or_else(|| {
            panic!("yaml path missing at segment {key:?} (full path: {path:?})")
        });
    }
    current
        .as_str()
        .unwrap_or_else(|| panic!("yaml leaf not a string: {path:?}"))
        .to_string()
}

/// Parse a CSS pixel literal (e.g. `"28px"`) into an `f32`.
fn parse_px(s: &str) -> f32 {
    let trimmed = s.trim();
    let num = trimmed
        .strip_suffix("px")
        .unwrap_or(trimmed)
        .trim()
        .parse::<f32>()
        .unwrap_or_else(|_| panic!("could not parse CSS pixel value: {s:?}"));
    num
}

/// Parse a CSS viewport-height literal (e.g. `"80vh"`) into the
/// fractional part (`0.80`). Other suffixes are rejected.
fn parse_vh_frac(s: &str) -> f32 {
    let trimmed = s.trim();
    let num = trimmed
        .strip_suffix("vh")
        .unwrap_or_else(|| panic!("not a vh literal: {s:?}"))
        .trim()
        .parse::<f32>()
        .unwrap_or_else(|_| panic!("could not parse vh value: {s:?}"));
    num / 100.0
}

/// Parse a CSS `rgba(...)` literal and return the alpha component
/// (0.0..=1.0). Other components are ignored.
fn parse_rgba_alpha(s: &str) -> f32 {
    let trimmed = s.trim();
    let inside = trimmed
        .strip_prefix("rgba(")
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or_else(|| panic!("not an rgba() literal: {s:?}"));
    let parts: Vec<&str> = inside.split(',').map(str::trim).collect();
    assert_eq!(parts.len(), 4, "expected rgba(r,g,b,a), got {s:?}");
    parts[3]
        .parse::<f32>()
        .unwrap_or_else(|_| panic!("could not parse rgba alpha: {s:?}"))
}

#[test]
fn yaml_scrim_matches_constant() {
    let root = parse_yaml();
    let alpha = parse_rgba_alpha(&root.dialogs.scrim);
    assert!(
        (alpha - tokens::SCRIM_ALPHA).abs() < 1e-4,
        "dialogs.scrim alpha {alpha} != tokens::SCRIM_ALPHA {}",
        tokens::SCRIM_ALPHA
    );
}

#[test]
fn yaml_corner_radius_matches_constant() {
    let root = parse_yaml();
    let radius = parse_px(&root.dialogs.layout.corner_radius);
    assert!(
        (radius - tokens::CORNER_RADIUS).abs() < 1e-4,
        "dialogs.layout.corner-radius {radius} != tokens::CORNER_RADIUS {}",
        tokens::CORNER_RADIUS
    );
}

#[test]
fn yaml_padding_matches_constant() {
    let root = parse_yaml();
    let padding = parse_px(&root.dialogs.layout.padding);
    assert!(
        (padding - tokens::PADDING).abs() < 1e-4,
        "dialogs.layout.padding {padding} != tokens::PADDING {}",
        tokens::PADDING
    );
}

#[test]
fn yaml_layout_widths_match_constants() {
    let root = parse_yaml();
    let pairs = [
        (
            "width-standard",
            parse_px(&root.dialogs.layout.width_standard),
            tokens::WIDTH_STANDARD,
        ),
        (
            "width-compact",
            parse_px(&root.dialogs.layout.width_compact),
            tokens::WIDTH_COMPACT,
        ),
    ];
    for (name, yaml_val, rust_val) in pairs {
        assert!(
            (yaml_val - rust_val).abs() < 1e-4,
            "dialogs.layout.{name} {yaml_val} != tokens {rust_val}"
        );
    }
}

#[test]
fn yaml_modal_actions_button_size_matches_constants() {
    let value: serde_yml::Value =
        serde_yml::from_str(YAML_SRC).expect("UI-DESIGN-GUIDELINES.yaml parsed as Value");
    let height = yaml_lookup(
        &value,
        &[
            "components",
            "buttons",
            "modal-actions",
            "properties",
            "height",
        ],
    );
    let min_width = yaml_lookup(
        &value,
        &[
            "components",
            "buttons",
            "modal-actions",
            "properties",
            "min-width",
        ],
    );
    let height_px = parse_px(&height);
    let min_width_px = parse_px(&min_width);
    assert!(
        (height_px - tokens::ACTION_BUTTON_HEIGHT).abs() < 1e-4,
        "components.buttons.modal-actions.properties.height {height_px} != tokens::ACTION_BUTTON_HEIGHT {}",
        tokens::ACTION_BUTTON_HEIGHT
    );
    assert!(
        (min_width_px - tokens::ACTION_BUTTON_MIN_WIDTH).abs() < 1e-4,
        "components.buttons.modal-actions.properties.min-width {min_width_px} != tokens::ACTION_BUTTON_MIN_WIDTH {}",
        tokens::ACTION_BUTTON_MIN_WIDTH
    );
}

#[test]
fn yaml_layout_max_heights_match_constants() {
    let root = parse_yaml();
    let pairs = [
        (
            "max-height-standard",
            parse_vh_frac(&root.dialogs.layout.max_height_standard),
            tokens::MAX_HEIGHT_STANDARD_FRAC,
        ),
        (
            "max-height-compact",
            parse_vh_frac(&root.dialogs.layout.max_height_compact),
            tokens::MAX_HEIGHT_COMPACT_FRAC,
        ),
    ];
    for (name, yaml_val, rust_val) in pairs {
        assert!(
            (yaml_val - rust_val).abs() < 1e-4,
            "dialogs.layout.{name} {yaml_val} != tokens {rust_val}"
        );
    }
}

#[test]
fn yaml_layout_spacings_match_constants() {
    let root = parse_yaml();
    let pairs = [
        (
            "actions-gap",
            parse_px(&root.dialogs.layout.actions_gap),
            tokens::ACTIONS_GAP,
        ),
        (
            "title-to-body-margin",
            parse_px(&root.dialogs.layout.title_to_body_margin),
            tokens::TITLE_TO_BODY_MARGIN,
        ),
        (
            "actions-top-margin",
            parse_px(&root.dialogs.layout.actions_top_margin),
            tokens::ACTIONS_TOP_MARGIN,
        ),
        (
            "body-item-spacing",
            parse_px(&root.dialogs.layout.body_item_spacing),
            tokens::BODY_ITEM_SPACING,
        ),
    ];
    for (name, yaml_val, rust_val) in pairs {
        assert!(
            (yaml_val - rust_val).abs() < 1e-4,
            "dialogs.layout.{name} {yaml_val} != tokens {rust_val}"
        );
    }
}

#[test]
fn yaml_typography_sizes_match_constants() {
    let root = parse_yaml();
    let title = parse_px(&root.tokens.typography.title_large.font_size);
    let body = parse_px(&root.tokens.typography.body_medium.font_size);
    assert!(
        (title - tokens::TITLE_LARGE_SIZE).abs() < 1e-4,
        "typography.title-large.font-size {title} != tokens::TITLE_LARGE_SIZE {}",
        tokens::TITLE_LARGE_SIZE
    );
    assert!(
        (body - tokens::BODY_MEDIUM_SIZE).abs() < 1e-4,
        "typography.body-medium.font-size {body} != tokens::BODY_MEDIUM_SIZE {}",
        tokens::BODY_MEDIUM_SIZE
    );
}

/// Extract the first numeric pixel value of a CSS property inside the
/// first rule whose selector list contains `selector` exactly. Returns
/// the float (e.g. `28.0` for `28px`). Panics if the selector or
/// property is missing — these are SSOT-bridge assertions.
fn css_property_px(css: &str, selector: &str, property: &str) -> f32 {
    let needle_open = format!("{selector} {{");
    let start = css
        .find(&needle_open)
        .or_else(|| {
            // Tolerate selectors that share a rule with siblings:
            // ".dialog-surface,\n.other-surface { ... }".
            css.find(&format!("{selector},"))
                .or_else(|| css.find(&format!("{selector}\n")))
        })
        .unwrap_or_else(|| panic!("CSS selector not found: {selector}"));
    let block_start = css[start..]
        .find('{')
        .map(|i| start + i + 1)
        .unwrap_or_else(|| panic!("CSS rule has no body: {selector}"));
    let block_end = css[block_start..]
        .find('}')
        .map(|i| block_start + i)
        .unwrap_or_else(|| panic!("CSS rule has no closing brace: {selector}"));
    let body = &css[block_start..block_end];
    let prop_needle = format!("{property}:");
    let prop_pos = body
        .find(&prop_needle)
        .unwrap_or_else(|| panic!("CSS property not found: {selector} {{ {property}: ... }}"));
    let after = &body[prop_pos + prop_needle.len()..];
    let semi = after
        .find(';')
        .unwrap_or_else(|| panic!("CSS property not semicolon-terminated: {selector} {property}"));
    let value = after[..semi].trim();
    parse_px(value)
}

#[test]
fn css_dialog_surface_matches_yaml_layout() {
    let root = parse_yaml();
    let yaml_padding = parse_px(&root.dialogs.layout.padding);
    let yaml_max_width = parse_px(&root.dialogs.layout.width_standard);
    let css_padding = css_property_px(DIALOG_CSS_SRC, ".dialog-surface", "padding");
    let css_max_width = css_property_px(DIALOG_CSS_SRC, ".dialog-surface", "max-width");
    assert!(
        (yaml_padding - css_padding).abs() < 1e-4,
        ".dialog-surface padding {css_padding}px != yaml {yaml_padding}px"
    );
    assert!(
        (yaml_max_width - css_max_width).abs() < 1e-4,
        ".dialog-surface max-width {css_max_width}px != yaml {yaml_max_width}px"
    );
}

#[test]
fn css_dialog_body_gap_matches_yaml_body_item_spacing() {
    let root = parse_yaml();
    let yaml_gap = parse_px(&root.dialogs.layout.body_item_spacing);
    let css_gap = css_property_px(DIALOG_CSS_SRC, ".dialog-body", "gap");
    assert!(
        (yaml_gap - css_gap).abs() < 1e-4,
        ".dialog-body gap {css_gap}px != yaml body-item-spacing {yaml_gap}px"
    );
}

#[test]
fn css_dialog_actions_match_yaml_layout() {
    let root = parse_yaml();
    let yaml_gap = parse_px(&root.dialogs.layout.actions_gap);
    let yaml_top = parse_px(&root.dialogs.layout.actions_top_margin);
    let css_gap = css_property_px(DIALOG_CSS_SRC, ".dialog-actions", "gap");
    let css_top = css_property_px(DIALOG_CSS_SRC, ".dialog-actions", "margin-top");
    assert!(
        (yaml_gap - css_gap).abs() < 1e-4,
        ".dialog-actions gap {css_gap}px != yaml actions-gap {yaml_gap}px"
    );
    assert!(
        (yaml_top - css_top).abs() < 1e-4,
        ".dialog-actions margin-top {css_top}px != yaml actions-top-margin {yaml_top}px"
    );
}

#[test]
fn css_dialog_title_margin_matches_yaml_title_to_body_margin() {
    let root = parse_yaml();
    let yaml_margin = parse_px(&root.dialogs.layout.title_to_body_margin);
    // .dialog-title uses shorthand `margin: 0 0 16px 0;`. Pull the
    // bottom value (3rd component) by parsing the shorthand directly.
    let needle_open = ".dialog-title {";
    let start = DIALOG_CSS_SRC
        .find(needle_open)
        .expect(".dialog-title rule missing in dialog-shell.css");
    let body_start = DIALOG_CSS_SRC[start..].find('{').unwrap() + start + 1;
    let body_end = DIALOG_CSS_SRC[body_start..].find('}').unwrap() + body_start;
    let body = &DIALOG_CSS_SRC[body_start..body_end];
    let margin_pos = body
        .find("margin:")
        .expect(".dialog-title rule missing margin shorthand");
    let after = &body[margin_pos + "margin:".len()..];
    let semi = after.find(';').unwrap();
    let value = after[..semi].trim();
    let parts: Vec<&str> = value.split_whitespace().collect();
    assert_eq!(
        parts.len(),
        4,
        ".dialog-title margin expected 4-part shorthand, got {value:?}"
    );
    let css_bottom = parse_px(parts[2]);
    assert!(
        (yaml_margin - css_bottom).abs() < 1e-4,
        ".dialog-title margin-bottom {css_bottom}px != yaml title-to-body-margin {yaml_margin}px"
    );
}

#[test]
fn yaml_elevation_3_matches_constants() {
    // box-shadow: "0 8px 32px rgba(0,0,0,0.30)"
    //                |   |    |    └─ alpha
    //                |   |    └────── blur
    //                |   └─────────── offset-y
    //                └─────────────── offset-x (spread implied 0)
    let root = parse_yaml();
    let shadow = &root.tokens.elevation.elevation_3.box_shadow;
    let trimmed = shadow.trim();
    // Split once at the first `rgba(` so the comma-separated rgba payload
    // doesn't conflict with the pixel triplet's whitespace separation.
    let (pixels, rgba) = trimmed
        .split_once("rgba(")
        .unwrap_or_else(|| panic!("box-shadow missing rgba(): {shadow:?}"));
    let rgba = format!("rgba({rgba}");
    let px_parts: Vec<&str> = pixels.split_whitespace().collect();
    assert_eq!(
        px_parts.len(),
        3,
        "expected 3 pixel components in box-shadow, got {pixels:?}"
    );
    let offset_y = parse_px(px_parts[1]);
    let blur = parse_px(px_parts[2]);
    let alpha = parse_rgba_alpha(&rgba);
    assert!(
        (offset_y - tokens::ELEVATION_SHADOW_OFFSET_Y).abs() < 1e-4,
        "elevation-3 offset-y {offset_y} != tokens {}",
        tokens::ELEVATION_SHADOW_OFFSET_Y
    );
    assert!(
        (blur - tokens::ELEVATION_SHADOW_BLUR).abs() < 1e-4,
        "elevation-3 blur {blur} != tokens {}",
        tokens::ELEVATION_SHADOW_BLUR
    );
    // The yaml shorthand has no explicit spread, so the implied 0 must
    // match the Rust constant.
    assert!(
        (tokens::ELEVATION_SHADOW_SPREAD - 0.0).abs() < 1e-4,
        "elevation-3 spread (Rust) {} != implied 0 from yaml shorthand",
        tokens::ELEVATION_SHADOW_SPREAD
    );
    // alpha is stored as u8 in Rust; compare against the float yaml value
    // by mapping yaml's [0.0, 1.0] alpha to a u8.
    let alpha_u8 = (alpha * 255.0).round() as u8;
    assert_eq!(
        alpha_u8,
        tokens::ELEVATION_SHADOW_ALPHA,
        "elevation-3 alpha (yaml u8) {alpha_u8} != tokens::ELEVATION_SHADOW_ALPHA {}",
        tokens::ELEVATION_SHADOW_ALPHA
    );
}

#[test]
fn yaml_color_roles_defined_in_styles_css() {
    let root = parse_yaml();
    let mut missing: BTreeSet<String> = BTreeSet::new();
    for role in root.tokens.color_roles.keys() {
        let needle = format!("--md-sys-color-{role}:");
        if !STYLES_CSS_SRC.contains(&needle) {
            missing.insert(role.clone());
        }
    }
    assert!(
        missing.is_empty(),
        "tokens.color-roles missing in styles.css :root: {missing:?}"
    );
}

#[test]
fn yaml_known_issues_does_not_reference_surface_variant() {
    // Once `surface-variant` is promoted to a first-class color role, the
    // `--md-sys-color-surface-variant` known-issue must be gone. The
    // entry can take any shape (list / mapping / scalar) so we walk the
    // tree as `serde_yml::Value` and look for the role name as a string
    // anywhere inside.
    let root = parse_yaml();
    let serialized = serde_yml::to_string(&root.known_issues).unwrap_or_default();
    assert!(
        !serialized.contains("--md-sys-color-surface-variant"),
        "known-issues still references --md-sys-color-surface-variant: {serialized}"
    );
}

#[test]
fn destructive_confirm_initial_focus_is_cancel() {
    assert_eq!(
        kinds::initial_focus(DialogKind::DestructiveConfirm),
        kinds::Target::Cancel
    );
}

#[test]
fn destructive_confirm_enter_targets_cancel() {
    assert_eq!(
        kinds::enter_target(DialogKind::DestructiveConfirm),
        kinds::Target::Cancel
    );
}

#[test]
fn input_initial_focus_is_primary() {
    // For input dialogs the "primary" placeholder means "fall back to
    // the registered first-frame focus widget if one was provided".
    assert_eq!(
        kinds::initial_focus(DialogKind::Input),
        kinds::Target::Primary
    );
}

#[test]
fn confirm_enter_targets_primary() {
    assert_eq!(
        kinds::enter_target(DialogKind::Confirm),
        kinds::Target::Primary
    );
}

#[test]
fn escape_always_targets_cancel() {
    for kind in [
        DialogKind::Input,
        DialogKind::Confirm,
        DialogKind::DestructiveConfirm,
    ] {
        assert_eq!(kinds::escape_target(kind), kinds::Target::Cancel);
    }
}

#[test]
#[should_panic(expected = "must not be a generic OK")]
fn primary_label_ok_panics_in_debug() {
    let _ = Dialog::<()>::confirm("確認", "Confirm", crate::i18n::Locale::En).primary_button(
        "OK",
        "OK",
        || (),
    );
}

#[test]
#[should_panic(expected = "must not be a generic OK")]
fn primary_label_ok_japanese_locale_panics_in_debug() {
    let _ = Dialog::<()>::input("確認", "Confirm", crate::i18n::Locale::Ja).primary_button(
        " ok ",
        "Save",
        || (),
    );
}
