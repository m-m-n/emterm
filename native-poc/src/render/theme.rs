//! Color and font resolution.
//!
//! Phase 4 populates this with a default 16-color palette + 256-color +
//! truecolor mapping. Phase 7 applies overrides from `settings.json`.
//!
//! Phase 6 adds `apply_osc` so that OSC 4/10/11/12/22/104/110/111/112
//! updates received by `NativeCallbacks` can be reflected into the live
//! theme. Color spec parsing accepts `rgb:RR/GG/BB`, `rgb:RRRR/GGGG/BBBB`,
//! and `#RRGGBB`, matching the legacy TS handler in
//! `src/terminal/osc-colors.ts`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const BLACK: Rgb = Rgb(0, 0, 0);
    pub const WHITE: Rgb = Rgb(0xee, 0xee, 0xee);
}

/// Cursor visual style, mirroring DECSCUSR / OSC 22 cursor-style values.
///
/// OSC 22 in the legacy code path drives the *mouse* cursor shape, but the
/// design table in IMPLEMENTATION.md repurposes it as a per-tab terminal
/// cursor style hint so that the renderer can react. We keep this enum
/// minimal — only what the action_type dispatch needs to record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorStyle {
    #[default]
    Block,
    Underline,
    Bar,
}

#[derive(Debug, Clone)]
pub struct Theme {
    pub fg: Rgb,
    pub bg: Rgb,
    pub cursor_fg: Rgb,
    pub palette16: [Rgb; 16],
    /// 256-entry sparse overlay onto the indexed palette. `None` means
    /// "use the default" (which for slots < 16 is `palette16[i]` and for
    /// 16..255 is the xterm 256-color cube/grayscale formula).
    pub palette256: Box<[Option<Rgb>; 256]>,
    pub cursor_style: CursorStyle,
    /// Theme-requested font family. Currently informational only after
    /// Phase 4-H: the `TerminalGridPass` resolves fonts through the
    /// `Resolver` + `FallbackChain` registered at startup, not via the
    /// Theme. A follow-up will plumb this string into
    /// `Resolver::by_family` so the user's theme choice influences the
    /// fallback chain root.
    #[allow(dead_code)]
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
            cursor_fg: Rgb::WHITE,
            palette16,
            palette256: Box::new([None; 256]),
            cursor_style: CursorStyle::default(),
            font_family: "monospace".into(),
            font_size_pt: 13.0,
        }
    }
}

/// Parse a single color specification token.
///
/// Accepted forms (mirroring `src/terminal/osc-colors.ts`):
///
/// - `rgb:RR/GG/BB` — 8-bit components (1 or 2 hex digits each).
/// - `rgb:RRRR/GGGG/BBBB` — 16-bit components, downscaled to 8 bits.
/// - `#RGB` — 4-bit components, scaled (`0xF` → `0xFF`).
/// - `#RRGGBB` — 8-bit components.
/// - `#RRRRGGGGBBBB` — 16-bit components, downscaled to 8 bits.
///
/// Returns `None` for unrecognized forms (including a `?` query, which is
/// the caller's responsibility to detect before this function runs).
pub fn parse_color_spec(spec: &str) -> Option<Rgb> {
    let spec = spec.trim();
    if let Some(rest) = spec.strip_prefix("rgb:") {
        parse_rgb_colon(rest)
    } else if let Some(rest) = spec.strip_prefix('#') {
        parse_hash(rest)
    } else {
        None
    }
}

fn parse_component(s: &str) -> Option<u8> {
    if s.is_empty() {
        return None;
    }
    let val = u32::from_str_radix(s, 16).ok()?;
    match s.len() {
        1 => Some(((val * 17) & 0xFF) as u8), // 0xF -> 0xFF
        2 => Some(val as u8),
        3 => Some((val >> 4) as u8),
        4 => Some((val >> 8) as u8),
        _ => None,
    }
}

fn parse_rgb_colon(s: &str) -> Option<Rgb> {
    let mut parts = s.split('/');
    let r = parse_component(parts.next()?)?;
    let g = parse_component(parts.next()?)?;
    let b = parse_component(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some(Rgb(r, g, b))
}

fn parse_hash(s: &str) -> Option<Rgb> {
    match s.len() {
        3 => {
            let r = u8::from_str_radix(&s[0..1], 16).ok()?;
            let g = u8::from_str_radix(&s[1..2], 16).ok()?;
            let b = u8::from_str_radix(&s[2..3], 16).ok()?;
            Some(Rgb(r * 17, g * 17, b * 17))
        }
        6 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            Some(Rgb(r, g, b))
        }
        12 => {
            let r = u16::from_str_radix(&s[0..4], 16).ok()?;
            let g = u16::from_str_radix(&s[4..8], 16).ok()?;
            let b = u16::from_str_radix(&s[8..12], 16).ok()?;
            Some(Rgb((r >> 8) as u8, (g >> 8) as u8, (b >> 8) as u8))
        }
        _ => None,
    }
}

impl Theme {
    /// Default cursor foreground color (used by OSC 112).
    pub const DEFAULT_CURSOR_FG: Rgb = Rgb::WHITE;

    /// Apply an OSC color/style mutation.
    ///
    /// `action_type` matches the value emitted by `term_core::osc_handler`:
    ///
    /// | code | semantic                                |
    /// |------|-----------------------------------------|
    /// | 4    | set palette entry: `index;spec[;...]`   |
    /// | 10   | set default foreground                  |
    /// | 11   | set default background                  |
    /// | 12   | set cursor foreground                   |
    /// | 22   | set cursor style ("block"/"underline"/"bar"/"") |
    /// | 104  | reset palette entries (empty = all)     |
    /// | 110  | reset default foreground                |
    /// | 111  | reset default background                |
    /// | 112  | reset cursor foreground                 |
    ///
    /// Returns `true` if any visible state changed (caller uses this to
    /// decide whether to `mark_all_dirty` on the terminal core).
    pub fn apply_osc(&mut self, action_type: u8, data: &str) -> bool {
        match action_type {
            4 => self.apply_palette_set(data),
            10 => self.apply_default_color_set(10, data),
            11 => self.apply_default_color_set(11, data),
            12 => self.apply_default_color_set(12, data),
            22 => self.apply_cursor_style(data),
            104 => self.apply_palette_reset(data),
            110 => {
                self.fg = Rgb::WHITE;
                true
            }
            111 => {
                self.bg = Rgb::BLACK;
                true
            }
            112 => {
                self.cursor_fg = Self::DEFAULT_CURSOR_FG;
                true
            }
            _ => false,
        }
    }

    fn apply_palette_set(&mut self, data: &str) -> bool {
        // Pairs of "index;spec[;index;spec...]"
        let mut tokens = data.split(';');
        let mut changed = false;
        while let Some(index_str) = tokens.next() {
            let Some(spec_str) = tokens.next() else { break };
            let Ok(index) = index_str.trim().parse::<usize>() else {
                continue;
            };
            if index >= 256 {
                continue;
            }
            // Query (`?`) responses are handled elsewhere; ignore here.
            if spec_str.trim() == "?" {
                continue;
            }
            if let Some(rgb) = parse_color_spec(spec_str) {
                self.palette256[index] = Some(rgb);
                if index < 16 {
                    self.palette16[index] = rgb;
                }
                changed = true;
            }
        }
        changed
    }

    fn apply_default_color_set(&mut self, osc_num: u8, data: &str) -> bool {
        // Chained: data may contain N specs, each advances osc_num by 1.
        // (Matches `handleOscDefaultColor` in the TS reference.)
        let mut changed = false;
        for (offset, spec) in data.split(';').enumerate() {
            let target = osc_num + offset as u8;
            if target > 12 {
                break;
            }
            let trimmed = spec.trim();
            if trimmed == "?" || trimmed.is_empty() {
                continue;
            }
            if let Some(rgb) = parse_color_spec(spec) {
                match target {
                    10 => {
                        self.fg = rgb;
                        changed = true;
                    }
                    11 => {
                        self.bg = rgb;
                        changed = true;
                    }
                    12 => {
                        self.cursor_fg = rgb;
                        changed = true;
                    }
                    _ => {}
                }
            }
        }
        changed
    }

    fn apply_cursor_style(&mut self, data: &str) -> bool {
        let trimmed = data.trim().trim_start_matches('>').trim_start_matches('<');
        let new_style = match trimmed {
            "" | "default" | "block" => CursorStyle::Block,
            "underline" => CursorStyle::Underline,
            "bar" | "vertical-text" => CursorStyle::Bar,
            _ => return false,
        };
        if self.cursor_style != new_style {
            self.cursor_style = new_style;
            true
        } else {
            false
        }
    }

    fn apply_palette_reset(&mut self, data: &str) -> bool {
        let trimmed = data.trim();
        if trimmed.is_empty() {
            let any_set = self.palette256.iter().any(|e| e.is_some());
            *self.palette256 = [None; 256];
            // palette16 stays at xterm defaults — caller should reset those
            // by re-constructing `Theme::default()` if they were mutated by
            // OSC 4 prior. For now we reset the overlay only.
            any_set
        } else {
            let mut changed = false;
            for part in trimmed.split(';') {
                if let Ok(index) = part.trim().parse::<usize>() {
                    if index < 256 && self.palette256[index].is_some() {
                        self.palette256[index] = None;
                        changed = true;
                    }
                }
            }
            changed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_color_spec ────────────────────────────────────

    #[test]
    fn parse_rgb_colon_8bit() {
        assert_eq!(
            parse_color_spec("rgb:ff/00/80"),
            Some(Rgb(0xff, 0x00, 0x80))
        );
    }

    #[test]
    fn parse_rgb_colon_16bit_downscales() {
        // 16-bit components downscaled by >> 8.
        assert_eq!(
            parse_color_spec("rgb:ffff/0000/8080"),
            Some(Rgb(0xff, 0x00, 0x80))
        );
    }

    #[test]
    fn parse_rgb_colon_single_hex_digit_scales() {
        // Single-hex-digit components scale by 17 (0xF -> 0xFF).
        assert_eq!(parse_color_spec("rgb:f/0/8"), Some(Rgb(0xff, 0x00, 0x88)));
    }

    #[test]
    fn parse_hash_rrggbb() {
        assert_eq!(parse_color_spec("#ff0080"), Some(Rgb(0xff, 0x00, 0x80)));
    }

    #[test]
    fn parse_hash_rgb_short() {
        assert_eq!(parse_color_spec("#f08"), Some(Rgb(0xff, 0x00, 0x88)));
    }

    #[test]
    fn parse_hash_rrrrggggbbbb_long() {
        assert_eq!(
            parse_color_spec("#ffff00008080"),
            Some(Rgb(0xff, 0x00, 0x80))
        );
    }

    #[test]
    fn parse_invalid_returns_none() {
        assert_eq!(parse_color_spec(""), None);
        assert_eq!(parse_color_spec("foo"), None);
        assert_eq!(parse_color_spec("rgb:zz/00/00"), None);
        assert_eq!(parse_color_spec("#zzzzzz"), None);
        assert_eq!(parse_color_spec("rgb:00/00"), None);
    }

    // ── Theme::apply_osc ────────────────────────────────────

    #[test]
    fn apply_osc_10_sets_fg() {
        let mut t = Theme::default();
        assert!(t.apply_osc(10, "rgb:11/22/33"));
        assert_eq!(t.fg, Rgb(0x11, 0x22, 0x33));
    }

    #[test]
    fn apply_osc_11_sets_bg() {
        let mut t = Theme::default();
        assert!(t.apply_osc(11, "#445566"));
        assert_eq!(t.bg, Rgb(0x44, 0x55, 0x66));
    }

    #[test]
    fn apply_osc_12_sets_cursor_fg() {
        let mut t = Theme::default();
        assert!(t.apply_osc(12, "rgb:aa/bb/cc"));
        assert_eq!(t.cursor_fg, Rgb(0xaa, 0xbb, 0xcc));
    }

    #[test]
    fn apply_osc_10_chained_advances_osc_num() {
        // "spec1;spec2;spec3" sets 10, 11, 12 respectively.
        let mut t = Theme::default();
        assert!(t.apply_osc(10, "rgb:01/02/03;rgb:04/05/06;rgb:07/08/09"));
        assert_eq!(t.fg, Rgb(1, 2, 3));
        assert_eq!(t.bg, Rgb(4, 5, 6));
        assert_eq!(t.cursor_fg, Rgb(7, 8, 9));
    }

    #[test]
    fn apply_osc_4_single_palette_entry() {
        let mut t = Theme::default();
        assert!(t.apply_osc(4, "5;rgb:11/22/33"));
        assert_eq!(t.palette256[5], Some(Rgb(0x11, 0x22, 0x33)));
        assert_eq!(t.palette16[5], Rgb(0x11, 0x22, 0x33));
    }

    #[test]
    fn apply_osc_4_chained_entries() {
        let mut t = Theme::default();
        assert!(t.apply_osc(4, "1;rgb:10/00/00;200;rgb:00/aa/00"));
        assert_eq!(t.palette256[1], Some(Rgb(0x10, 0, 0)));
        assert_eq!(t.palette256[200], Some(Rgb(0, 0xaa, 0)));
    }

    #[test]
    fn apply_osc_4_invalid_pair_skipped() {
        let mut t = Theme::default();
        // Index 999 is out-of-range, second pair valid.
        assert!(t.apply_osc(4, "999;rgb:00/00/00;7;rgb:ff/ff/ff"));
        assert_eq!(t.palette256[7], Some(Rgb(0xff, 0xff, 0xff)));
    }

    #[test]
    fn apply_osc_22_block() {
        let mut t = Theme::default();
        t.cursor_style = CursorStyle::Bar;
        assert!(t.apply_osc(22, "block"));
        assert_eq!(t.cursor_style, CursorStyle::Block);
    }

    #[test]
    fn apply_osc_22_underline() {
        let mut t = Theme::default();
        assert!(t.apply_osc(22, "underline"));
        assert_eq!(t.cursor_style, CursorStyle::Underline);
    }

    #[test]
    fn apply_osc_22_bar() {
        let mut t = Theme::default();
        assert!(t.apply_osc(22, "bar"));
        assert_eq!(t.cursor_style, CursorStyle::Bar);
    }

    #[test]
    fn apply_osc_22_empty_resets_to_block() {
        let mut t = Theme::default();
        t.cursor_style = CursorStyle::Underline;
        assert!(t.apply_osc(22, ""));
        assert_eq!(t.cursor_style, CursorStyle::Block);
    }

    #[test]
    fn apply_osc_22_invalid_keeps_state() {
        let mut t = Theme::default();
        assert!(!t.apply_osc(22, "totally-bogus"));
        assert_eq!(t.cursor_style, CursorStyle::Block);
    }

    #[test]
    fn apply_osc_104_empty_resets_all() {
        let mut t = Theme::default();
        t.palette256[5] = Some(Rgb(1, 2, 3));
        t.palette256[200] = Some(Rgb(4, 5, 6));
        assert!(t.apply_osc(104, ""));
        assert!(t.palette256.iter().all(|e| e.is_none()));
    }

    #[test]
    fn apply_osc_104_indexed_resets_only_listed() {
        let mut t = Theme::default();
        t.palette256[5] = Some(Rgb(1, 2, 3));
        t.palette256[6] = Some(Rgb(4, 5, 6));
        assert!(t.apply_osc(104, "5"));
        assert!(t.palette256[5].is_none());
        assert_eq!(t.palette256[6], Some(Rgb(4, 5, 6)));
    }

    #[test]
    fn apply_osc_104_no_change_returns_false() {
        let mut t = Theme::default();
        assert!(!t.apply_osc(104, "5"));
    }

    #[test]
    fn apply_osc_110_resets_fg() {
        let mut t = Theme::default();
        t.fg = Rgb(1, 2, 3);
        assert!(t.apply_osc(110, ""));
        assert_eq!(t.fg, Rgb::WHITE);
    }

    #[test]
    fn apply_osc_111_resets_bg() {
        let mut t = Theme::default();
        t.bg = Rgb(1, 2, 3);
        assert!(t.apply_osc(111, ""));
        assert_eq!(t.bg, Rgb::BLACK);
    }

    #[test]
    fn apply_osc_112_resets_cursor_fg() {
        let mut t = Theme::default();
        t.cursor_fg = Rgb(1, 2, 3);
        assert!(t.apply_osc(112, ""));
        assert_eq!(t.cursor_fg, Theme::DEFAULT_CURSOR_FG);
    }

    #[test]
    fn apply_osc_unknown_returns_false() {
        let mut t = Theme::default();
        assert!(!t.apply_osc(99, "anything"));
    }
}
