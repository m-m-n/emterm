//! Color and font resolution.
//!
//! Phase 4 populates this with a default 16-color palette + 256-color +
//! truecolor mapping. Phase 7 applies overrides from `settings.json`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const BLACK: Rgb = Rgb(0, 0, 0);
    pub const WHITE: Rgb = Rgb(0xee, 0xee, 0xee);
}

#[derive(Debug, Clone)]
pub struct Theme {
    pub fg: Rgb,
    pub bg: Rgb,
    pub palette16: [Rgb; 16],
    pub font_family: String,
    pub font_size_pt: f32,
}

impl Default for Theme {
    fn default() -> Self {
        // Standard xterm-like 16-color palette.
        let palette16 = [
            Rgb(0x00, 0x00, 0x00), // 0  black
            Rgb(0xcd, 0x00, 0x00), // 1  red
            Rgb(0x00, 0xcd, 0x00), // 2  green
            Rgb(0xcd, 0xcd, 0x00), // 3  yellow
            Rgb(0x00, 0x00, 0xee), // 4  blue
            Rgb(0xcd, 0x00, 0xcd), // 5  magenta
            Rgb(0x00, 0xcd, 0xcd), // 6  cyan
            Rgb(0xe5, 0xe5, 0xe5), // 7  white
            Rgb(0x7f, 0x7f, 0x7f), // 8  bright black
            Rgb(0xff, 0x00, 0x00), // 9  bright red
            Rgb(0x00, 0xff, 0x00), // 10 bright green
            Rgb(0xff, 0xff, 0x00), // 11 bright yellow
            Rgb(0x5c, 0x5c, 0xff), // 12 bright blue
            Rgb(0xff, 0x00, 0xff), // 13 bright magenta
            Rgb(0x00, 0xff, 0xff), // 14 bright cyan
            Rgb(0xff, 0xff, 0xff), // 15 bright white
        ];
        Self {
            fg: Rgb::WHITE,
            bg: Rgb::BLACK,
            palette16,
            font_family: "monospace".into(),
            font_size_pt: 13.0,
        }
    }
}
