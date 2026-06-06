fn main() {
    use swash::scale::{Render, ScaleContext, Source};
    use swash::zeno::{Format, Vector};
    use swash::FontRef;

    let size_px = 17.333f32;
    let bytes =
        std::fs::read("/home/sakura/workspace/Inconsolata/fonts/otf/Inconsolata-Regular.otf")
            .unwrap();
    let face = FontRef::from_index(&bytes, 0).unwrap();
    for ch in ['m', 'M', 'w', 'd', '/'] {
        let glyph_id = face.charmap().map(ch);
        for (label, format) in [("alpha", Format::Alpha), ("subpixel", Format::Subpixel)] {
            let mut sctx = ScaleContext::new();
            let mut scaler = sctx.builder(face).size(size_px).hint(true).build();
            let image = Render::new(&[Source::Outline])
                .format(format)
                .offset(Vector::ZERO)
                .render(&mut scaler, glyph_id)
                .unwrap();
            let p = image.placement;
            println!(
                "{ch:?} {label}: left={} top={} w={} h={} | right_edge=left+w={} (cell=9)",
                p.left,
                p.top,
                p.width,
                p.height,
                p.left + p.width as i32
            );
        }
    }
}
