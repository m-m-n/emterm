//! Shared CSD (client-side decoration) chrome primitives.
//!
//! Owned by `ui/` so every native window — the main terminal
//! (`window_host`) and child viewers like the image viewer
//! (`viewer::image_window`) — draws its frameless-window chrome from the
//! same definitions: the edge/corner drag-resize hot zones and the egui
//! font stack for the chrome text. The title-bar widget itself lives in
//! [`super::title_bar`].

use winit::window::ResizeDirection;

/// Hit-zone width for CSD edge / corner resize, expressed in egui logical
/// points. 8 pt is the smallest band that's reliably grabbable with a
/// mouse — the user's hand can overshoot the edge band before the
/// cursor icon flips, so anything narrower than this manifested as
/// "上下リサイズが効かない" because the pointer ended up in the
/// title-bar / status-bar interior before reaching the resize zone.
pub(crate) const RESIZE_EDGE_PX: f32 = 8.0;

/// Classify a pointer position against the eight CSD resize hot zones.
///
/// Pure over the inputs so the resize hot-zone math can be unit-tested
/// without instantiating a real `winit::Window`. The caller layers the
/// "maximized → never resize" rule on top — that condition is not
/// expressible in terms of the geometry alone.
pub(crate) fn classify_resize_edge(
    width: f32,
    height: f32,
    x: f32,
    y: f32,
    edge_px: f32,
) -> Option<ResizeDirection> {
    // Reject negative coords (Wayland delivers them briefly on pointer
    // leave) and positions past the far edge so we never latch a
    // phantom direction with the pointer outside the window.
    if x < 0.0 || y < 0.0 || x > width || y > height {
        return None;
    }
    let near_left = x < edge_px;
    let near_right = x > width - edge_px;
    let near_top = y < edge_px;
    let near_bottom = y > height - edge_px;
    use ResizeDirection::*;
    match (near_top, near_bottom, near_left, near_right) {
        (true, _, true, _) => Some(NorthWest),
        (true, _, _, true) => Some(NorthEast),
        (_, true, true, _) => Some(SouthWest),
        (_, true, _, true) => Some(SouthEast),
        (true, _, _, _) => Some(North),
        (_, true, _, _) => Some(South),
        (_, _, true, _) => Some(West),
        (_, _, _, true) => Some(East),
        _ => None,
    }
}

/// Configure egui's font stack for the chrome (tab bar / title bar /
/// status bar): the user's `ui_font_family` on `Proportional`, the
/// user's terminal `font_family_primary` on `Monospace`, plus bundled
/// CJK and outline-emoji fallbacks on both.
///
/// Three problems are addressed:
///
/// 1. egui's `FontDefinitions::default()` ships only `Hack` on the
///    `Monospace` family, which covers ASCII + Latin extensions but
///    no CJK. Any 日本語 in a `{cmd:…}` script's output would
///    therefore render as tofu.
/// 2. The same default registers BW emoji fonts on `Proportional`
///    only; pictographs (✅ 🟢 ☂ etc.) fall off the end of the
///    Monospace chain.
/// 3. `settings.ui_font_family` mirrors the WebView build's
///    `--ui-font-family` CSS variable and skins the chrome's
///    proportional text (tab bar, title bar). It is prepended to
///    `Proportional` only. Analogously `settings.font_family_primary`
///    mirrors `--terminal-font-family` — the status bar renders on
///    the `Monospace` chain, so the terminal font is prepended there
///    so its glyph shape matches the terminal grid instead of egui's
///    bundled Hack.
///
/// We register the bundled `NotoSansCJK-JP` and `NotoColorEmoji.ttf`
/// (already linked in via [`crate::render::font::resolver`] for the
/// terminal grid) and append them to both `Monospace` and
/// `Proportional` so other egui surfaces (tab bar, title bar)
/// inherit the same coverage.
///
/// Caveat: egui 0.29 / ab_glyph cannot raster color emoji
/// (CBDT / COLR v1) — for the emoji font only the *monochrome
/// outline* layer is reachable. Full color-emoji parity with the
/// WebView build requires switching the status-bar text path to a
/// swash-based custom painter, which is out of scope here.
pub(crate) fn configure_egui_fonts(
    ctx: &egui::Context,
    ui_font_family: &str,
    terminal_font_family: &str,
) {
    ctx.set_fonts(build_egui_fonts(ui_font_family, terminal_font_family));
}

/// Build the `FontDefinitions` for [`configure_egui_fonts`]. Split out
/// so tests can inspect the resulting chains without an egui `Context`.
pub(crate) fn build_egui_fonts(
    ui_font_family: &str,
    terminal_font_family: &str,
) -> egui::FontDefinitions {
    use crate::render::font::resolver::{
        BUNDLED_CJK_FONT, BUNDLED_EMOJI_COLOR_FONT, BUNDLED_SYMBOLS_FONT,
    };

    const CJK_KEY: &str = "EmtermBundledCJK";
    const EMOJI_KEY: &str = "EmtermBundledEmoji";
    const SYMBOLS_KEY: &str = "EmtermBundledSymbols";
    const UI_FONT_KEY: &str = "EmtermUiFont";
    const TERMINAL_FONT_KEY: &str = "EmtermTerminalFont";

    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        CJK_KEY.to_string(),
        egui::FontData::from_static(BUNDLED_CJK_FONT),
    );
    fonts.font_data.insert(
        EMOJI_KEY.to_string(),
        egui::FontData::from_static(BUNDLED_EMOJI_COLOR_FONT),
    );
    // Bundled Noto Sans Symbols 2: prompt arrows / math symbols /
    // geometric shapes / braille dots that the CJK + emoji fonts miss
    // (e.g. `✻` U+273B and `⠋` U+2807 used in Claude Code's OSC title
    // spinner, `❯` U+276F shown by starship prompts). Mirrors the
    // terminal-grid `FontRole::Secondary` registration in `app.rs` so
    // chrome surfaces (tab bar, title bar, status bar) inherit the same
    // glyph coverage as the grid — otherwise Windows tofus on
    // characters Linux happens to cover via system-installed
    // `Noto Sans Symbols2`.
    fonts.font_data.insert(
        SYMBOLS_KEY.to_string(),
        egui::FontData::from_static(BUNDLED_SYMBOLS_FONT),
    );

    for family in [egui::FontFamily::Monospace, egui::FontFamily::Proportional] {
        let chain = fonts.families.entry(family).or_default();
        for name in [CJK_KEY, EMOJI_KEY, SYMBOLS_KEY] {
            if !chain.iter().any(|n| n == name) {
                chain.push(name.to_string());
            }
        }
    }

    // User-configured UI font: resolve the family through the same
    // fontdb lookup the terminal grid uses, then make it the first
    // Proportional candidate. Unknown families warn and keep egui's
    // default — matching the WebView CSS fallback (`var(--ui-font-family,
    // sans-serif)`).
    let family = ui_font_family.trim();
    if !family.is_empty() {
        match crate::render::font::resolver::load_system_family_bytes(family, 400, None) {
            Some((bytes, index)) => {
                let mut data = egui::FontData::from_owned(bytes.to_vec());
                // `.ttc` collection member — egui can address faces by
                // index directly (unlike the swash ingest path).
                data.index = index;
                fonts.font_data.insert(UI_FONT_KEY.to_string(), data);
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .insert(0, UI_FONT_KEY.to_string());
                log::info!("settings: ui_font_family={family:?} applied to UI chrome");
            }
            None => {
                log::warn!(
                    "settings.ui_font_family={family:?}: family not found on this host; using egui default"
                );
            }
        }
    }

    // User-configured terminal font: same resolution as the UI font,
    // but prepended to `Monospace` so status-bar text picks up the
    // user's chosen terminal typeface (e.g. Inconsolata) instead of
    // egui's bundled Hack. Empty / unknown families keep egui's default
    // Monospace head — matching the WebView CSS fallback for
    // `var(--terminal-font-family, monospace)`.
    let terminal_family = terminal_font_family.trim();
    if !terminal_family.is_empty() {
        match crate::render::font::resolver::load_system_family_bytes(terminal_family, 400, None) {
            Some((bytes, index)) => {
                let mut data = egui::FontData::from_owned(bytes.to_vec());
                data.index = index;
                fonts.font_data.insert(TERMINAL_FONT_KEY.to_string(), data);
                fonts
                    .families
                    .entry(egui::FontFamily::Monospace)
                    .or_default()
                    .insert(0, TERMINAL_FONT_KEY.to_string());
                log::info!(
                    "settings: font_family_primary={terminal_family:?} applied to status-bar chrome"
                );
            }
            None => {
                log::warn!(
                    "settings.font_family_primary={terminal_family:?}: family not found on this host; using egui default Monospace"
                );
            }
        }
    }

    fonts
}
