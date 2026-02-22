/// SGR (Select Graphic Rendition) handler.
use wasm_bindgen::prelude::*;

use crate::cell::*;
use crate::terminal_core::TerminalCore;

#[wasm_bindgen]
impl TerminalCore {
    /// Handle SGR (Select Graphic Rendition) parameters.
    /// Parses the raw parameter array and applies attributes to cursor.
    pub fn handle_sgr(&mut self, params: &[u16]) {
        if params.is_empty() {
            // Empty params = Reset
            self.cursor.fg = PackedColor::DEFAULT;
            self.cursor.bg = PackedColor::DEFAULT;
            self.cursor.flags = 0;
            return;
        }

        let mut i = 0;
        while i < params.len() {
            let p = params[i];
            match p {
                0 => {
                    self.cursor.fg = PackedColor::DEFAULT;
                    self.cursor.bg = PackedColor::DEFAULT;
                    self.cursor.flags = 0;
                }
                1 => self.cursor.flags |= STYLE_BOLD,
                2 => self.cursor.flags |= STYLE_DIM,
                3 => self.cursor.flags |= STYLE_ITALIC,
                4 => self.cursor.flags |= STYLE_UNDERLINE,
                5 => self.cursor.flags |= STYLE_BLINK,
                7 => self.cursor.flags |= STYLE_REVERSE,
                8 => self.cursor.flags |= STYLE_HIDDEN,
                9 => self.cursor.flags |= STYLE_STRIKETHROUGH,
                22 => self.cursor.flags &= !(STYLE_BOLD | STYLE_DIM),
                23 => self.cursor.flags &= !STYLE_ITALIC,
                24 => self.cursor.flags &= !STYLE_UNDERLINE,
                25 => self.cursor.flags &= !STYLE_BLINK,
                27 => self.cursor.flags &= !STYLE_REVERSE,
                28 => self.cursor.flags &= !STYLE_HIDDEN,
                29 => self.cursor.flags &= !STYLE_STRIKETHROUGH,
                30..=37 => self.cursor.fg = PackedColor::indexed((p - 30) as u8),
                38 => {
                    // Extended foreground color
                    i += 1;
                    if i >= params.len() {
                        break;
                    }
                    match params[i] {
                        5 => {
                            // 38;5;n - Indexed color
                            i += 1;
                            if i < params.len() {
                                self.cursor.fg = PackedColor::indexed(params[i] as u8);
                            }
                        }
                        2 => {
                            // 38;2;r;g;b - RGB color
                            if i + 3 < params.len() {
                                self.cursor.fg = PackedColor::rgb(
                                    params[i + 1] as u8,
                                    params[i + 2] as u8,
                                    params[i + 3] as u8,
                                );
                                i += 3;
                            }
                        }
                        _ => {}
                    }
                }
                39 => self.cursor.fg = PackedColor::DEFAULT,
                40..=47 => self.cursor.bg = PackedColor::indexed((p - 40) as u8),
                48 => {
                    // Extended background color
                    i += 1;
                    if i >= params.len() {
                        break;
                    }
                    match params[i] {
                        5 => {
                            // 48;5;n - Indexed color
                            i += 1;
                            if i < params.len() {
                                self.cursor.bg = PackedColor::indexed(params[i] as u8);
                            }
                        }
                        2 => {
                            // 48;2;r;g;b - RGB color
                            if i + 3 < params.len() {
                                self.cursor.bg = PackedColor::rgb(
                                    params[i + 1] as u8,
                                    params[i + 2] as u8,
                                    params[i + 3] as u8,
                                );
                                i += 3;
                            }
                        }
                        _ => {}
                    }
                }
                49 => self.cursor.bg = PackedColor::DEFAULT,
                90..=97 => self.cursor.fg = PackedColor::indexed((p - 90 + 8) as u8),
                100..=107 => self.cursor.bg = PackedColor::indexed((p - 100 + 8) as u8),
                _ => {} // Unknown: ignore
            }
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::cell::*;
    use crate::terminal_core::TerminalCore;

    // ── Sprint 4: SGR Tests ─────────────────────────────────

    #[test]
    fn test_sgr_empty_resets() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.cursor.fg = PackedColor::indexed(1);
        core.cursor.flags = STYLE_BOLD;
        core.handle_sgr(&[]);
        assert_eq!(core.cursor.fg, PackedColor::DEFAULT);
        assert_eq!(core.cursor.bg, PackedColor::DEFAULT);
        assert_eq!(core.cursor.flags, 0);
    }

    #[test]
    fn test_sgr_reset_param0() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.cursor.fg = PackedColor::indexed(1);
        core.cursor.flags = STYLE_BOLD | STYLE_ITALIC;
        core.handle_sgr(&[0]);
        assert_eq!(core.cursor.fg, PackedColor::DEFAULT);
        assert_eq!(core.cursor.flags, 0);
    }

    #[test]
    fn test_sgr_style_flags() {
        let cases: &[(u16, u16)] = &[
            (1, STYLE_BOLD),
            (2, STYLE_DIM),
            (3, STYLE_ITALIC),
            (4, STYLE_UNDERLINE),
            (5, STYLE_BLINK),
            (7, STYLE_REVERSE),
            (8, STYLE_HIDDEN),
            (9, STYLE_STRIKETHROUGH),
        ];
        for &(param, flag) in cases {
            let mut core = TerminalCore::new(80, 24, 0);
            core.handle_sgr(&[param]);
            assert_ne!(
                core.cursor.flags & flag,
                0,
                "SGR {} should set flag 0x{:04x}",
                param,
                flag
            );
        }
    }

    #[test]
    fn test_sgr_style_resets() {
        let cases: &[(u16, u16)] = &[
            (22, STYLE_BOLD | STYLE_DIM),
            (23, STYLE_ITALIC),
            (24, STYLE_UNDERLINE),
            (25, STYLE_BLINK),
            (27, STYLE_REVERSE),
            (28, STYLE_HIDDEN),
            (29, STYLE_STRIKETHROUGH),
        ];
        for &(param, flag) in cases {
            let mut core = TerminalCore::new(80, 24, 0);
            core.cursor.flags = 0xFFFF;
            core.handle_sgr(&[param]);
            assert_eq!(
                core.cursor.flags & flag,
                0,
                "SGR {} should clear flag 0x{:04x}",
                param,
                flag
            );
        }
    }

    #[test]
    fn test_sgr_standard_foreground() {
        for p in 30..=37 {
            let mut core = TerminalCore::new(80, 24, 0);
            core.handle_sgr(&[p]);
            assert_eq!(core.cursor.fg, PackedColor::indexed((p - 30) as u8));
        }
    }

    #[test]
    fn test_sgr_standard_background() {
        for p in 40..=47 {
            let mut core = TerminalCore::new(80, 24, 0);
            core.handle_sgr(&[p]);
            assert_eq!(core.cursor.bg, PackedColor::indexed((p - 40) as u8));
        }
    }

    #[test]
    fn test_sgr_bright_foreground() {
        for p in 90..=97 {
            let mut core = TerminalCore::new(80, 24, 0);
            core.handle_sgr(&[p]);
            assert_eq!(core.cursor.fg, PackedColor::indexed((p - 90 + 8) as u8));
        }
    }

    #[test]
    fn test_sgr_bright_background() {
        for p in 100..=107 {
            let mut core = TerminalCore::new(80, 24, 0);
            core.handle_sgr(&[p]);
            assert_eq!(core.cursor.bg, PackedColor::indexed((p - 100 + 8) as u8));
        }
    }

    #[test]
    fn test_sgr_indexed_fg() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_sgr(&[38, 5, 196]);
        assert_eq!(core.cursor.fg, PackedColor::indexed(196));
    }

    #[test]
    fn test_sgr_indexed_bg() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_sgr(&[48, 5, 21]);
        assert_eq!(core.cursor.bg, PackedColor::indexed(21));
    }

    #[test]
    fn test_sgr_rgb_fg() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_sgr(&[38, 2, 255, 128, 0]);
        assert_eq!(core.cursor.fg, PackedColor::rgb(255, 128, 0));
    }

    #[test]
    fn test_sgr_rgb_bg() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_sgr(&[48, 2, 0, 128, 255]);
        assert_eq!(core.cursor.bg, PackedColor::rgb(0, 128, 255));
    }

    #[test]
    fn test_sgr_default_fg_bg() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.cursor.fg = PackedColor::indexed(5);
        core.cursor.bg = PackedColor::indexed(3);
        core.handle_sgr(&[39]);
        assert_eq!(core.cursor.fg, PackedColor::DEFAULT);
        core.handle_sgr(&[49]);
        assert_eq!(core.cursor.bg, PackedColor::DEFAULT);
    }

    #[test]
    fn test_sgr_multiple_params() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_sgr(&[1, 31, 42]);
        assert_ne!(core.cursor.flags & STYLE_BOLD, 0);
        assert_eq!(core.cursor.fg, PackedColor::indexed(1)); // red
        assert_eq!(core.cursor.bg, PackedColor::indexed(2)); // green
    }

    #[test]
    fn test_sgr_truncated_extended() {
        let mut core = TerminalCore::new(80, 24, 0);
        // 38;5 without index - should not panic
        core.handle_sgr(&[38, 5]);
        // 38 without subtype - should not panic
        core.handle_sgr(&[38]);
    }

    #[test]
    fn test_sgr_unknown_param() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_sgr(&[99]);
        // Should not crash, attrs unchanged
        assert_eq!(core.cursor.fg, PackedColor::DEFAULT);
        assert_eq!(core.cursor.flags, 0);
    }

    #[test]
    fn test_sgr_combined_rgb_fg_and_bg_10_params() {
        // Reproduces the treemd color corruption bug:
        // Combined fg RGB + bg RGB requires 10 params.
        // bg rgb(43, 48, 59) - the r=43 falls in SGR 40-47 range,
        // so if params are truncated it gets misinterpreted as SGR 43 (yellow bg).
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_sgr(&[38, 2, 200, 200, 200, 48, 2, 43, 48, 59]);
        assert_eq!(core.cursor.fg, PackedColor::rgb(200, 200, 200));
        assert_eq!(core.cursor.bg, PackedColor::rgb(43, 48, 59));
    }

    #[test]
    fn test_sgr_combined_rgb_fg_bg_with_styles_13_params() {
        // Maximum realistic SGR: bold + fg RGB + bg RGB = 1 + 5 + 5 + 1 + 1 = 13 params
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_sgr(&[1, 3, 38, 2, 100, 150, 200, 48, 2, 50, 60, 70, 4]);
        assert_ne!(core.cursor.flags & STYLE_BOLD, 0);
        assert_ne!(core.cursor.flags & STYLE_ITALIC, 0);
        assert_ne!(core.cursor.flags & STYLE_UNDERLINE, 0);
        assert_eq!(core.cursor.fg, PackedColor::rgb(100, 150, 200));
        assert_eq!(core.cursor.bg, PackedColor::rgb(50, 60, 70));
    }
}
