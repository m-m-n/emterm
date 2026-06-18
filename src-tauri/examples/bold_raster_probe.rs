//! Probe: rasterize a glyph from Inconsolata Regular / Bold via the
//! production SwashRasterizer and dump per-column coverage so the result
//! can be compared against WebView screenshots pixel by pixel.
//!
//! Run: CARGO_TARGET_DIR=native-poc/target cargo run --manifest-path \
//!      native-poc/Cargo.toml --example bold_raster_probe

use std::sync::Arc;

fn main() {
    // Duplicates the adapter's swash calls (examples cannot link the
    // binary crate's private modules); keep in sync with
    // src/render/font/swash_adapter.rs `raster`.
    use swash::FontRef;
    use swash::scale::{Render, ScaleContext, Source, StrikeWith};
    use swash::zeno::{Format, Vector};

    let size_px = 17.333f32;
    for (label, path) in [
        (
            "Regular",
            "/home/sakura/workspace/Inconsolata/fonts/otf/Inconsolata-Regular.otf",
        ),
        (
            "Bold",
            "/home/sakura/workspace/Inconsolata/fonts/otf/Inconsolata-Bold.otf",
        ),
    ] {
        let bytes = std::fs::read(path).expect("font file");
        let bytes: Arc<[u8]> = Arc::from(bytes.as_slice());
        let face = FontRef::from_index(&bytes, 0).expect("parse font");
        let weight = face.attributes().weight().0;
        let glyph_id = face.charmap().map('o');
        for embolden in [0.0f32, 0.251f32] {
            let mut sctx = ScaleContext::new();
            let mut scaler = sctx.builder(face).size(size_px).hint(true).build();
            let image = Render::new(&[
                Source::ColorBitmap(StrikeWith::BestFit),
                Source::ColorOutline(0),
                Source::Outline,
            ])
            .format(Format::Subpixel)
            .offset(Vector::ZERO)
            .embolden(embolden)
            .render(&mut scaler, glyph_id)
            .expect("raster");
            let w = image.placement.width as usize;
            let h = image.placement.height as usize;
            let total: u64 = image.data.iter().map(|&b| b as u64).sum();
            println!(
                "{label} (weight={weight}) embolden={embolden}: 'o' {w}x{h}px, coverage_sum={total}"
            );
            for row in 0..h {
                let mut line = String::new();
                for col in 0..w {
                    // G channel = center sample
                    let g = image.data[(row * w + col) * 4 + 1];
                    line.push(match g {
                        0..=31 => ' ',
                        32..=95 => '.',
                        96..=159 => 'o',
                        160..=223 => 'O',
                        _ => '#',
                    });
                }
                println!("    |{line}|");
            }
        }
    }
}
