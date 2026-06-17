//! PoC: load Noto Color Emoji, rasterize one emoji glyph via swash + zeno,
//! and write an RGBA PNG to disk.
//!
//! This is the FR1 gate for the font-swash-migration plan
//! (`doc/tasks/font-swash-migration/IMPLEMENTATION.md` Phase 1). Run with:
//!
//! ```sh
//! cargo run -p emterm --example swash_emoji --features gui
//! ```
//!
//! Default output path: `src-tauri/target/swash_emoji.png`. Override via
//! the first CLI argument.

use std::path::PathBuf;

use swash::scale::image::Content;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::{Format, Vector};
use swash::{CacheKey, FontRef};

/// Bundled emoji font bytes. Embedded at compile time per FR11.
const EMOJI_FONT: &[u8] = include_bytes!("../assets/fonts/NotoColorEmoji.ttf");

/// The PoC codepoint: 😀 (U+1F600 GRINNING FACE).
const EMOJI_CODEPOINT: char = '\u{1F600}';

fn main() {
    // Output path (override via CLI arg 1).
    let out_path: PathBuf = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            p.push("target");
            p.push("swash_emoji.png");
            p
        });

    // Parse the bundled font.
    let font = FontRef::from_index(EMOJI_FONT, 0).expect("parse Noto Color Emoji");
    let _ = CacheKey::new(); // verify cache-key API is wired (suppresses unused-import via use above if applicable)

    // Resolve the emoji codepoint to a glyph id.
    let charmap = font.charmap();
    let glyph_id = charmap.map(EMOJI_CODEPOINT);
    if glyph_id == 0 {
        eprintln!(
            "[swash_emoji] emoji codepoint U+{:04X} not in font; .notdef returned",
            EMOJI_CODEPOINT as u32
        );
    }

    // Build a scaler. Noto Color Emoji is a CBDT font; the strike size on
    // disk is usually 109px or 128px, but Render picks the best strike via
    // `StrikeWith::BestFit`, so the requested size below acts as a hint.
    let mut ctx = ScaleContext::new();
    let mut scaler = ctx.builder(font).size(128.0).hint(false).build();

    // Compose color (CBDT bitmap) and outline sources so the emoji glyph
    // comes back as RGBA. ColorBitmap is the primary source for Noto Color
    // Emoji; the others are fallbacks if a particular glyph lacks the
    // bitmap strike.
    let image = Render::new(&[
        Source::ColorBitmap(StrikeWith::BestFit),
        Source::ColorOutline(0),
        Source::Outline,
    ])
    .format(Format::Alpha) // ignored when ColorBitmap is selected — the
    // resulting Content tag tells us which path won.
    .offset(Vector::ZERO)
    .render(&mut scaler, glyph_id)
    .expect("rasterize emoji glyph");

    let w = image.placement.width;
    let h = image.placement.height;
    let bytes = image.data;
    assert!(w > 0 && h > 0, "empty bitmap from swash for U+1F600");

    // Convert to RGBA8.
    let rgba: Vec<u8> = match image.content {
        Content::Mask => {
            // Alpha-only fallback — expand to white-on-transparent RGBA.
            let mut out = Vec::with_capacity((w * h) as usize * 4);
            for a in bytes {
                out.push(0xFF);
                out.push(0xFF);
                out.push(0xFF);
                out.push(a);
            }
            out
        }
        Content::Color => bytes.clone(),
        Content::SubpixelMask => bytes.clone(),
    };
    assert_eq!(
        rgba.len(),
        (w * h * 4) as usize,
        "unexpected pixel count for w={} h={}",
        w,
        h,
    );

    // Ensure the output directory exists.
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).expect("create output dir");
    }

    // PNG encode.
    let file = std::fs::File::create(&out_path).expect("create output PNG");
    let buf = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(buf, w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("write PNG header");
    writer
        .write_image_data(&rgba)
        .expect("write PNG image data");

    println!(
        "[swash_emoji] wrote {} ({} x {}, content={:?})",
        out_path.display(),
        w,
        h,
        image.content,
    );
}
