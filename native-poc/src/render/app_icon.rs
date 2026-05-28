//! App icon asset loading for the window chrome.
//!
//! Owns embedding, rasterizing, and caching the application icon so the
//! title-bar widget stays a pure layout/event widget that merely receives
//! the prepared texture id. The icon source is the git-tracked vector
//! logo (the PNG variants under `src-tauri/icons/` are gitignored build
//! artifacts).

use egui::{ColorImage, Context, TextureHandle, TextureId, TextureOptions};

/// App icon source — the git-tracked vector logo, rasterized once at
/// [`ICON_RASTER_PX`] and cached for the lifetime of the egui context.
const APP_ICON_SVG: &[u8] = include_bytes!("../../../assets/eMterm.svg");
/// Pixel side length the SVG is rasterized at. 64 px gives a crisp
/// downscale to the on-screen title-bar icon even at 2.0× HiDPI.
const ICON_RASTER_PX: u32 = 64;

/// Cache entry stored in the egui temp store: `Some(handle)` once the
/// texture is uploaded, `None` if rasterization failed. Caching the
/// failure too means a permanent parse error (the SVG is a compile-time
/// constant) is attempted at most once instead of every frame.
#[derive(Clone)]
struct IconCache(Option<TextureHandle>);

fn cache_id() -> egui::Id {
    egui::Id::new("native-poc-app-icon")
}

/// Return the app-icon texture id, rasterizing + uploading the embedded
/// SVG on first call and caching the result (success or failure) for the
/// lifetime of the `Context`. Returns `None` when rasterization failed,
/// in which case the title bar simply renders without an icon.
pub fn texture_id(ctx: &Context) -> Option<TextureId> {
    let id = cache_id();
    if let Some(IconCache(cached)) = ctx.data(|d| d.get_temp::<IconCache>(id)) {
        return cached.map(|h| h.id());
    }
    let handle = rasterize()
        .map(|image| ctx.load_texture("native-poc-app-icon", image, TextureOptions::LINEAR));
    let tid = handle.as_ref().map(|h| h.id());
    ctx.data_mut(|d| d.insert_temp(id, IconCache(handle)));
    tid
}

/// Rasterize the embedded SVG logo into a square [`ColorImage`] at
/// [`ICON_RASTER_PX`]. The SVG is scaled to fit on its longer side and
/// centered, then rendered onto a transparent premultiplied-alpha pixmap
/// (the format egui expects).
fn rasterize() -> Option<ColorImage> {
    use resvg::{tiny_skia, usvg};

    let tree = usvg::Tree::from_data(APP_ICON_SVG, &usvg::Options::default()).ok()?;
    let svg_size = tree.size();
    let scale = ICON_RASTER_PX as f32 / svg_size.width().max(svg_size.height());
    // Center the scaled content so a non-square source isn't anchored to
    // the top-left corner of the square pixmap.
    let tx = (ICON_RASTER_PX as f32 - svg_size.width() * scale) / 2.0;
    let ty = (ICON_RASTER_PX as f32 - svg_size.height() * scale) / 2.0;

    let mut pixmap = tiny_skia::Pixmap::new(ICON_RASTER_PX, ICON_RASTER_PX)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_translate(tx, ty).pre_scale(scale, scale),
        &mut pixmap.as_mut(),
    );

    let size = [ICON_RASTER_PX as usize, ICON_RASTER_PX as usize];
    Some(ColorImage::from_rgba_premultiplied(size, pixmap.data()))
}
