/// CSI mode handler: DECCKM, IRM, alternate screen, DECTCEM, etc.
use wasm_bindgen::prelude::*;

use crate::terminal_core::*;

const MODE_ACTION_NONE: u8 = 0;
const MODE_ACTION_SWITCH_TO_ALT: u8 = 1;
const MODE_ACTION_SAVE_AND_SWITCH_TO_ALT: u8 = 2;
const MODE_ACTION_SWITCH_TO_MAIN: u8 = 3;
const MODE_ACTION_SAVE_CURSOR: u8 = 4;
const MODE_ACTION_RESTORE_CURSOR: u8 = 5;
const MODE_ACTION_TS_FALLBACK: u8 = 0xFF;

#[wasm_bindgen]
impl TerminalCore {
    /// CSI ? Pm h/l - Set/Reset DEC Private Mode.
    /// Returns action code for TS-side execution.
    pub fn handle_set_mode(&mut self, mode: u16, enable: bool) -> u8 {
        match mode {
            // Boolean modes: set directly in WASM bitfield
            3 => {
                self.set_mode(MODE_COLUMN_132, enable);
                MODE_ACTION_NONE
            }
            5 => {
                self.set_mode(MODE_REVERSE_SCREEN, enable);
                MODE_ACTION_NONE
            }
            6 => {
                self.set_mode(MODE_ORIGIN, enable);
                MODE_ACTION_NONE
            }
            7 => {
                self.set_mode(MODE_AUTO_WRAP, enable);
                MODE_ACTION_NONE
            }
            12 => {
                self.set_mode(MODE_CURSOR_BLINK, enable);
                MODE_ACTION_NONE
            }
            25 => {
                self.set_mode(MODE_CURSOR_VISIBLE, enable);
                MODE_ACTION_NONE
            }

            // Buffer switch modes: return action code
            47 | 1047 => {
                if enable {
                    MODE_ACTION_SWITCH_TO_ALT
                } else {
                    MODE_ACTION_SWITCH_TO_MAIN
                }
            }
            1048 => {
                if enable {
                    MODE_ACTION_SAVE_CURSOR
                } else {
                    MODE_ACTION_RESTORE_CURSOR
                }
            }
            1049 => {
                if enable {
                    MODE_ACTION_SAVE_AND_SWITCH_TO_ALT
                } else {
                    MODE_ACTION_SWITCH_TO_MAIN
                }
            }

            // Boolean modes handled via TS fallback for multi-valued side effects
            1004 => {
                self.set_mode(MODE_FOCUS_TRACKING, enable);
                MODE_ACTION_NONE
            }
            2004 => {
                self.set_mode(MODE_BRACKETED_PASTE, enable);
                MODE_ACTION_NONE
            }

            // Multi-valued modes: TS fallback
            1 | 1000 | 1002 | 1003 | 1005 | 1006 => MODE_ACTION_TS_FALLBACK,

            // Unknown mode: no-op
            _ => MODE_ACTION_NONE,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::terminal_core::*;

    // ── Sprint 4: Mode Tests ────────────────────────────────

    #[test]
    fn test_mode_boolean_autowrap() {
        let mut core = TerminalCore::new(80, 24);
        let code = core.handle_set_mode(7, true);
        assert_eq!(code, 0);
        assert!(core.get_mode(MODE_AUTO_WRAP));
        let code = core.handle_set_mode(7, false);
        assert_eq!(code, 0);
        assert!(!core.get_mode(MODE_AUTO_WRAP));
    }

    #[test]
    fn test_mode_boolean_cursor_visible() {
        let mut core = TerminalCore::new(80, 24);
        let code = core.handle_set_mode(25, false);
        assert_eq!(code, 0);
        assert!(!core.get_mode(MODE_CURSOR_VISIBLE));
    }

    #[test]
    fn test_mode_boolean_origin() {
        let mut core = TerminalCore::new(80, 24);
        let code = core.handle_set_mode(6, true);
        assert_eq!(code, 0);
        assert!(core.get_mode(MODE_ORIGIN));
    }

    #[test]
    fn test_mode_buffer_switch_47() {
        let mut core = TerminalCore::new(80, 24);
        assert_eq!(core.handle_set_mode(47, true), 1); // switchToAlt
        assert_eq!(core.handle_set_mode(47, false), 3); // switchToMain
    }

    #[test]
    fn test_mode_buffer_switch_1049() {
        let mut core = TerminalCore::new(80, 24);
        assert_eq!(core.handle_set_mode(1049, true), 2); // saveAndSwitchToAlt
        assert_eq!(core.handle_set_mode(1049, false), 3); // switchToMain
    }

    #[test]
    fn test_mode_save_restore_cursor_1048() {
        let mut core = TerminalCore::new(80, 24);
        assert_eq!(core.handle_set_mode(1048, true), 4); // saveCursor
        assert_eq!(core.handle_set_mode(1048, false), 5); // restoreCursor
    }

    #[test]
    fn test_mode_ts_fallback() {
        let mut core = TerminalCore::new(80, 24);
        for mode in [1, 1000, 1002, 1003, 1005, 1006] {
            assert_eq!(
                core.handle_set_mode(mode, true),
                0xFF,
                "Mode {} should fallback",
                mode
            );
        }
        // 1004 and 2004 are boolean modes handled in WASM
        assert_eq!(core.handle_set_mode(1004, true), 0);
        assert!(core.get_mode(MODE_FOCUS_TRACKING));
        assert_eq!(core.handle_set_mode(2004, true), 0);
        assert!(core.get_mode(MODE_BRACKETED_PASTE));
    }

    #[test]
    fn test_mode_unknown() {
        let mut core = TerminalCore::new(80, 24);
        assert_eq!(core.handle_set_mode(9999, true), 0);
    }
}
