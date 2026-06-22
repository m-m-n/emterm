//! Shared window-icon loader.
//!
//! Loads the bundled `128x128.png` asset, decodes it to RGBA, and hands
//! back a [`winit::window::Icon`] ready for `WindowAttributes::with_window_icon`.
//! The same helper feeds both the winit main window
//! ([`crate::window_host::WindowHost::new`]) and the wry child WebView
//! windows ([`crate::webview_host::windows::WebViewApp::resumed`]), so the
//! decode path is implemented in exactly one place.
//!
//! Fail-soft semantics (FR5): any decode failure logs `log::warn!` and
//! returns `None`. The caller passes the `Option` straight into
//! `with_window_icon`, so a `None` is treated as "do not attach an icon"
//! and the window is still created.
//!
//! This module is gated behind `#[cfg(feature = "gui")]` because
//! `winit::window::Icon` only exists in the GUI build. The CLI-only build
//! (`--no-default-features`) never reaches this code.

use winit::window::Icon;

/// Embedded PNG payload used for the window icon.
///
/// We pick `128x128.png` over `32x32.png` so HiDPI title bars and Alt+Tab
/// thumbnails have headroom; winit downsizes as needed. The asset is
/// pulled in via `include_bytes!` so it lives in the binary's read-only
/// section — no filesystem access at runtime (NFR2/NFR3).
const ICON_PNG_BYTES: &[u8] = include_bytes!("../icons/128x128.png");

/// Load the bundled app icon. Returns `Some(Icon)` on success, `None` (with
/// a `log::warn!`) on decode failure.
///
/// Cheap to call — the embedded PNG decode + `Icon::from_rgba` together
/// fit comfortably under NFR3's 10 ms budget on a modern host. Callers
/// should still treat the helper as a one-shot startup cost; nothing in
/// this module caches the result.
pub fn app_icon() -> Option<Icon> {
    decode_icon(ICON_PNG_BYTES)
}

/// Private decode pipeline shared between [`app_icon`] and the unit tests.
///
/// Kept off the public surface because the only legitimate runtime caller
/// is [`app_icon`]; tests use it to exercise the failure path on a
/// deliberately-broken byte slice without corrupting the bundled asset.
fn decode_icon(bytes: &[u8]) -> Option<Icon> {
    let img = match image::load_from_memory(bytes) {
        Ok(img) => img,
        Err(e) => {
            log::warn!("window_icon: PNG decode failed: {e}");
            return None;
        }
    };
    let rgba = img.into_rgba8();
    let (w, h) = rgba.dimensions();
    match Icon::from_rgba(rgba.into_raw(), w, h) {
        Ok(icon) => Some(icon),
        Err(e) => {
            log::warn!("window_icon: Icon::from_rgba failed: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FR4: the bundled PNG must decode into a usable `Icon` on every
    /// supported host (Linux test runners included). This is the success
    /// path that backs both winit and wry call sites.
    #[test]
    fn app_icon_decodes_bundled_asset() {
        let icon = app_icon();
        assert!(
            icon.is_some(),
            "app_icon() should decode the bundled 128x128.png"
        );
    }

    /// FR5: a broken byte slice must NOT panic. The decoder logs a warn
    /// and returns `None`, and the caller (winit / wry) treats `None` as
    /// "no icon" without failing window creation.
    #[test]
    fn decode_icon_returns_none_on_broken_input() {
        // Deliberately non-PNG bytes. We exercise the private entrypoint
        // so we never touch the real bundled asset.
        let garbage = [0u8, 1, 2, 3, 4, 5, 6, 7];
        let icon = decode_icon(&garbage);
        assert!(icon.is_none(), "decode_icon should fail-soft on garbage");
    }
}
