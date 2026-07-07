/// CSI internal dispatch: routes ParsedAction::CsiDispatch to handler methods.
use crate::parser_params::ParamParser;
use crate::terminal_core::TerminalCore;

const MODE_ACTION_TS_FALLBACK: u8 = 0xFF;

impl TerminalCore {
    pub(crate) fn handle_csi_internal(
        &mut self,
        params: &[u16],
        intermediates: &[u8],
        final_byte: u8,
    ) {
        match (intermediates.first(), final_byte) {
            // Cursor movement
            (None, b'A') => {
                self.handle_cursor_up(ParamParser::get_first_or_one(params));
            }
            (None, b'B') => {
                self.handle_cursor_down(ParamParser::get_first_or_one(params));
            }
            (None, b'C') => {
                self.handle_cursor_forward(ParamParser::get_first_or_one(params));
            }
            (None, b'D') => {
                self.handle_cursor_back(ParamParser::get_first_or_one(params));
            }
            (None, b'E') => {
                self.handle_cursor_next_line(ParamParser::get_first_or_one(params));
            }
            (None, b'F') => {
                self.handle_cursor_previous_line(ParamParser::get_first_or_one(params));
            }
            (None, b'G') => {
                self.handle_cursor_horizontal_absolute(ParamParser::get_first_or_one(params));
            }
            (None, b'H') | (None, b'f') => {
                let row = ParamParser::get_param(params, 0, 1);
                let col = ParamParser::get_param(params, 1, 1);
                self.handle_cursor_position(row, col);
            }
            (None, b'd') => {
                self.handle_cursor_vertical_absolute(ParamParser::get_first_or_one(params));
            }

            // DECSCUSR - Set Cursor Style: CSI Ps SP q
            (Some(b' '), b'q') => {
                self.handle_decscusr(ParamParser::get_first_or_zero(params));
            }

            // Erase operations
            (None, b'J') => {
                // ED 3 (Erase Scrollback) returns a sentinel from the screen
                // handler; perform the actual scrollback clear here. Other
                // modes return 0 (handled in place).
                if self.handle_erase_in_display(ParamParser::get_first_or_zero(params) as u8)
                    == crate::csi_screen::SCROLLBACK_SENTINEL
                {
                    self.clear_scrollback();
                }
            }
            (None, b'K') => {
                self.handle_erase_in_line(ParamParser::get_first_or_zero(params) as u8);
            }
            (None, b'X') => {
                self.handle_erase_characters(ParamParser::get_first_or_one(params));
            }

            // Insert/Delete operations
            (None, b'@') => {
                self.handle_insert_characters(ParamParser::get_first_or_one(params));
            }
            (None, b'P') => {
                self.handle_delete_characters(ParamParser::get_first_or_one(params));
            }
            (None, b'L') => {
                self.handle_insert_lines(ParamParser::get_first_or_one(params));
            }
            (None, b'M') => {
                self.handle_delete_lines(ParamParser::get_first_or_one(params));
            }

            // Scroll operations
            (None, b'S') => {
                self.handle_scroll_up(ParamParser::get_first_or_one(params));
            }
            (None, b'T') => {
                self.handle_scroll_down(ParamParser::get_first_or_one(params));
            }
            (None, b'r') => {
                let top = ParamParser::get_param(params, 0, 1);
                let bottom = ParamParser::get_param(params, 1, 0);
                self.handle_decstbm(top, bottom);
            }

            // SGR
            (None, b'm') => self.handle_sgr(params),

            // DEC private modes
            (Some(b'?'), b'h') => {
                for &p in params {
                    let action = self.handle_set_mode(p, true);
                    if action != 0 {
                        if action == MODE_ACTION_TS_FALLBACK {
                            self.mode_actions.push(0xFF); // TS_FALLBACK set
                            self.mode_actions.push((p & 0xFF) as u8);
                            self.mode_actions.push(((p >> 8) & 0xFF) as u8);
                        } else {
                            self.mode_actions.push(action);
                        }
                    }
                }
            }
            (Some(b'?'), b'l') => {
                for &p in params {
                    let action = self.handle_set_mode(p, false);
                    if action != 0 {
                        if action == MODE_ACTION_TS_FALLBACK {
                            self.mode_actions.push(0xFE); // TS_FALLBACK reset
                            self.mode_actions.push((p & 0xFF) as u8);
                            self.mode_actions.push(((p >> 8) & 0xFF) as u8);
                        } else {
                            self.mode_actions.push(action);
                        }
                    }
                }
            }

            // Device status
            (None, b'n') => {
                let len =
                    self.handle_device_status_report(ParamParser::get_first_or_zero(params) as u8);
                if len > 0 {
                    self.fire_device_response_callback();
                }
            }

            // Device attributes
            (None, b'c') | (Some(b'?'), b'c') => {
                let len = self.handle_primary_device_attributes();
                if len > 0 {
                    self.fire_device_response_callback();
                }
            }
            (Some(b'>'), b'c') => {
                let len = self.handle_secondary_device_attributes();
                if len > 0 {
                    self.fire_device_response_callback();
                }
            }
            (Some(b'='), b'c') => {
                // TertiaryDeviceAttributes - currently ignored
            }

            // XTWINOPS (window operations / size reports)
            (None, b't') => {
                let ps = ParamParser::get_first_or_zero(params);
                let len = match ps {
                    14 => self.handle_xtwinops_text_area_px(),
                    16 => self.handle_xtwinops_cell_size(),
                    18 => self.handle_xtwinops_text_area_chars(),
                    _ => 0,
                };
                if len > 0 {
                    self.fire_device_response_callback();
                }
            }

            // DECRPM - DEC Private Mode Report: CSI ? Ps $ p
            // intermediates: [b'?', b'$'], final: b'p'
            (Some(b'?'), b'p') if intermediates.get(1) == Some(&b'$') => {
                let mode = ParamParser::get_first_or_zero(params);
                let len = self.handle_decrpm(mode);
                if len > 0 {
                    self.fire_device_response_callback();
                }
            }

            _ => { /* Unknown CSI - ignore */ }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::terminal_core::TerminalCore;

    #[test]
    fn test_csi_internal_cursor_up() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(0, 10);
        core.handle_csi_internal(&[5], &[], b'A');
        assert_eq!(core.get_cursor_row(), 5);
    }

    #[test]
    fn test_csi_internal_cursor_down() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(0, 10);
        core.handle_csi_internal(&[3], &[], b'B');
        assert_eq!(core.get_cursor_row(), 13);
    }

    #[test]
    fn test_csi_internal_cursor_position() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_csi_internal(&[5, 10], &[], b'H');
        assert_eq!(core.get_cursor_row(), 4); // 1-indexed → 0-indexed
        assert_eq!(core.get_cursor_col(), 9);
    }

    #[test]
    fn test_csi_internal_sgr() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_csi_internal(&[1], &[], b'm'); // Bold
        // SGR applied (no crash)
    }

    #[test]
    fn test_csi_internal_erase_display() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_csi_internal(&[2], &[], b'J'); // Erase all
    }

    #[test]
    fn test_csi_internal_mode_set_boolean() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_csi_internal(&[25], &[b'?'], b'h'); // Show cursor
        assert!(core.mode_actions.is_empty()); // Boolean mode, no action queued
    }

    #[test]
    fn test_csi_internal_mode_set_buffer_switch() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_csi_internal(&[47], &[b'?'], b'h'); // Switch to alt
        assert_eq!(core.mode_actions, vec![1]); // MODE_ACTION_SWITCH_TO_ALT
    }

    #[test]
    fn test_csi_internal_mode_set_1049() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_csi_internal(&[1049], &[b'?'], b'h');
        assert_eq!(core.mode_actions, vec![2]); // MODE_ACTION_SAVE_AND_SWITCH_TO_ALT
    }

    #[test]
    fn test_csi_internal_mode_ts_fallback_set() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_csi_internal(&[1000], &[b'?'], b'h');
        assert_eq!(core.mode_actions, vec![0xFF, 0xE8, 0x03]); // TS_FALLBACK set, 1000 = 0x03E8
    }

    #[test]
    fn test_csi_internal_mode_ts_fallback_reset() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_csi_internal(&[1000], &[b'?'], b'l');
        assert_eq!(core.mode_actions, vec![0xFE, 0xE8, 0x03]); // TS_FALLBACK reset
    }

    #[test]
    fn test_csi_internal_mode_decckm() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_csi_internal(&[1], &[b'?'], b'h'); // DECCKM set
        assert_eq!(core.mode_actions, vec![0xFF, 0x01, 0x00]);
    }

    #[test]
    fn test_take_mode_actions() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_csi_internal(&[47], &[b'?'], b'h');
        let actions = core.take_mode_actions();
        assert_eq!(actions, vec![1]);
        assert!(core.mode_actions.is_empty()); // Cleared
    }

    #[test]
    fn test_csi_internal_multiple_modes() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_csi_internal(&[47, 25], &[b'?'], b'h');
        // 47 → action 1 (switch to alt), 25 → boolean (no action)
        assert_eq!(core.mode_actions, vec![1]);
    }

    #[test]
    fn test_csi_internal_device_attributes() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_csi_internal(&[], &[], b'c'); // DA1
        assert!(core.response_len > 0);
    }

    #[test]
    fn test_csi_internal_tertiary_device_attributes_ignored() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_csi_internal(&[], &[b'='], b'c'); // DA3 - ignored
        assert_eq!(core.response_len, 0);
    }

    #[test]
    fn test_csi_internal_scroll_region() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_csi_internal(&[5, 20], &[], b'r');
        assert_eq!(core.get_scroll_region_top(), 4); // 1-indexed → 0-indexed
        assert_eq!(core.get_scroll_region_bottom(), 19);
    }

    #[test]
    fn test_csi_internal_decrpm_mode_2026() {
        let mut core = TerminalCore::new(80, 24, 0);
        // CSI ? 2026 $ p
        core.handle_csi_internal(&[2026], &[b'?', b'$'], b'p');
        assert!(core.response_len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[?2026;2$y"); // reset
    }

    #[test]
    fn test_csi_internal_decrpm_without_dollar_ignored() {
        let mut core = TerminalCore::new(80, 24, 0);
        // CSI ? 2026 p (without $) - should not match DECRPM
        core.handle_csi_internal(&[2026], &[b'?'], b'p');
        assert_eq!(core.response_len, 0); // No response
    }

    #[test]
    fn test_csi_internal_unknown_ignored() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(5, 5);
        core.handle_csi_internal(&[1, 2, 3], &[], b'z'); // Unknown
        assert_eq!(core.get_cursor_col(), 5); // Unchanged
        assert_eq!(core.get_cursor_row(), 5);
    }

    #[test]
    fn test_csi_internal_decscusr_routes_to_handler() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_csi_internal(&[3], &[b' '], b'q'); // DECSCUSR Ps=3
        assert_eq!(core.get_cursor_style(), 1); // underline
        assert!(core.get_cursor_blink());
    }

    /// AC-9: entering and leaving the alternate screen (CSI ?1049h / ?1049l)
    /// must not change the effective cursor shape or blink — the DECSCUSR/
    /// OSC 22 override and the settings default both live at the terminal
    /// level (cursor-settings-fix D1), untouched by the buffer-switch path.
    #[test]
    fn test_alt_screen_round_trip_preserves_cursor_style_and_blink() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor_style(2); // bar (settings default)
        core.set_cursor_blink(false); // steady (settings default)
        core.handle_decscusr(3); // active override: blinking underline

        core.process_pty_data_fully(b"\x1b[?1049h");
        assert_eq!(core.get_cursor_style(), 1, "override survives alt-enter");
        assert!(core.get_cursor_blink(), "override survives alt-enter");

        core.process_pty_data_fully(b"\x1b[?1049l");
        assert_eq!(core.get_cursor_style(), 1, "override survives alt-leave");
        assert!(core.get_cursor_blink(), "override survives alt-leave");
    }
}
