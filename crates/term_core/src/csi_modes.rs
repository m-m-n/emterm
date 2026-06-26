/// CSI mode handler: DECCKM, IRM, alternate screen, DECTCEM, etc.
use crate::terminal_core::*;

const MODE_ACTION_NONE: u8 = 0;
const MODE_ACTION_SWITCH_TO_ALT: u8 = 1;
const MODE_ACTION_SAVE_AND_SWITCH_TO_ALT: u8 = 2;
const MODE_ACTION_SWITCH_TO_MAIN: u8 = 3;
// MODE_ACTION_SAVE_CURSOR (4) and MODE_ACTION_RESTORE_CURSOR (5) are no longer
// used: DEC mode 1048h/l now calls save_cursor()/restore_cursor() immediately
// in WASM instead of deferring to TS via mode actions.
const MODE_ACTION_TS_FALLBACK: u8 = 0xFF;

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
                // Track hidden→visible transition to allow render of intermediate state
                if self.cursor_show_interrupt && enable && !self.get_mode(MODE_CURSOR_VISIBLE) {
                    self.cursor_just_shown = true;
                }
                self.set_mode(MODE_CURSOR_VISIBLE, enable);
                MODE_ACTION_NONE
            }

            // Buffer switch modes: return action code
            // Also reset synchronized output to prevent orphaned suppression
            47 | 1047 => {
                self.set_mode(MODE_SYNCHRONIZED_OUTPUT, false);
                // Track the alt-screen state core-side so parse-time
                // consumers (OSC 133 prompt-mark capture) see the switch
                // at the exact byte it happens, not a chunk later.
                self.set_mode(MODE_ALT_SCREEN, enable);
                if enable {
                    MODE_ACTION_SWITCH_TO_ALT
                } else {
                    MODE_ACTION_SWITCH_TO_MAIN
                }
            }
            1048 => {
                // Handle cursor save/restore immediately in WASM (same as ESC 7/8).
                // Previously deferred to TS via mode actions, which caused:
                // 1. Timing bug: save/restore happened after the entire data chunk
                //    was processed, not at the point the sequence appeared
                // 2. Dual-slot bug: ESC 7/8 used WASM saved_cursor while 1048h/l
                //    used a separate TS saved cursor, causing mismatches
                if enable {
                    self.save_cursor();
                } else {
                    self.restore_cursor();
                }
                MODE_ACTION_NONE
            }
            1049 => {
                self.set_mode(MODE_SYNCHRONIZED_OUTPUT, false);
                self.set_mode(MODE_ALT_SCREEN, enable);
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
            2026 => {
                self.set_mode(MODE_SYNCHRONIZED_OUTPUT, enable);
                MODE_ACTION_NONE
            }

            // DECSET 1007 (alternate_scroll): AltScreen wheel→arrow
            // translation. Track the bit core-side so the host can read
            // it via `get_mode(MODE_ALTERNATE_SCROLL)` before deciding
            // whether to emit arrow bytes. The host also gates on its
            // own user setting; this arm only carries the application's
            // runtime opt-in/out.
            1007 => {
                self.set_mode(MODE_ALTERNATE_SCROLL, enable);
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
        let mut core = TerminalCore::new(80, 24, 0);
        let code = core.handle_set_mode(7, true);
        assert_eq!(code, 0);
        assert!(core.get_mode(MODE_AUTO_WRAP));
        let code = core.handle_set_mode(7, false);
        assert_eq!(code, 0);
        assert!(!core.get_mode(MODE_AUTO_WRAP));
    }

    #[test]
    fn test_mode_boolean_cursor_visible() {
        let mut core = TerminalCore::new(80, 24, 0);
        let code = core.handle_set_mode(25, false);
        assert_eq!(code, 0);
        assert!(!core.get_mode(MODE_CURSOR_VISIBLE));
    }

    #[test]
    fn test_mode_boolean_origin() {
        let mut core = TerminalCore::new(80, 24, 0);
        let code = core.handle_set_mode(6, true);
        assert_eq!(code, 0);
        assert!(core.get_mode(MODE_ORIGIN));
    }

    #[test]
    fn test_mode_buffer_switch_47() {
        let mut core = TerminalCore::new(80, 24, 0);
        assert_eq!(core.handle_set_mode(47, true), 1); // switchToAlt
        assert_eq!(core.handle_set_mode(47, false), 3); // switchToMain
    }

    #[test]
    fn test_mode_buffer_switch_1049() {
        let mut core = TerminalCore::new(80, 24, 0);
        assert_eq!(core.handle_set_mode(1049, true), 2); // saveAndSwitchToAlt
        assert_eq!(core.handle_set_mode(1049, false), 3); // switchToMain
    }

    #[test]
    fn test_mode_save_restore_cursor_1048() {
        let mut core = TerminalCore::new(80, 24, 0);
        // 1048h/l now handled immediately in WASM (returns NONE)
        core.set_cursor(10, 5);
        assert_eq!(core.handle_set_mode(1048, true), 0); // saveCursor (immediate)
        core.set_cursor(20, 10);
        assert_eq!(core.handle_set_mode(1048, false), 0); // restoreCursor (immediate)
        assert_eq!(core.get_cursor_col(), 10);
        assert_eq!(core.get_cursor_row(), 5);
    }

    #[test]
    fn test_mode_ts_fallback() {
        let mut core = TerminalCore::new(80, 24, 0);
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
        let mut core = TerminalCore::new(80, 24, 0);
        assert_eq!(core.handle_set_mode(9999, true), 0);
    }

    // ── Synchronized Output (Mode 2026) Tests ─────────────

    #[test]
    fn test_mode_synchronized_output_set_reset() {
        let mut core = TerminalCore::new(80, 24, 0);
        assert!(!core.get_mode(MODE_SYNCHRONIZED_OUTPUT));
        let code = core.handle_set_mode(2026, true);
        assert_eq!(code, 0); // MODE_ACTION_NONE
        assert!(core.get_mode(MODE_SYNCHRONIZED_OUTPUT));
        let code = core.handle_set_mode(2026, false);
        assert_eq!(code, 0);
        assert!(!core.get_mode(MODE_SYNCHRONIZED_OUTPUT));
    }

    #[test]
    fn test_mode_synchronized_output_default_off() {
        let core = TerminalCore::new(80, 24, 0);
        assert!(!core.get_mode(MODE_SYNCHRONIZED_OUTPUT));
    }

    #[test]
    fn test_mode_synchronized_output_reset_on_buffer_switch_47() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_set_mode(2026, true);
        assert!(core.get_mode(MODE_SYNCHRONIZED_OUTPUT));
        core.handle_set_mode(47, true); // switch to alt
        assert!(!core.get_mode(MODE_SYNCHRONIZED_OUTPUT));
    }

    #[test]
    fn test_mode_synchronized_output_reset_on_buffer_switch_1049() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_set_mode(2026, true);
        assert!(core.get_mode(MODE_SYNCHRONIZED_OUTPUT));
        core.handle_set_mode(1049, true); // save + switch to alt
        assert!(!core.get_mode(MODE_SYNCHRONIZED_OUTPUT));
    }

    // ── DECSET 1007 (alternate_scroll) ──────────────────────

    /// TS-1: a fresh `TerminalCore` has `MODE_ALTERNATE_SCROLL` set so
    /// AltScreen wheel translation is on by default (matching xterm /
    /// WezTerm). The host then layers its own user-setting gate on top.
    #[test]
    fn alternate_scroll_default_on() {
        let core = TerminalCore::new(80, 24, 0);
        assert!(core.get_mode(MODE_ALTERNATE_SCROLL));
    }

    /// TS-2: `ESC[?1007h` / `ESC[?1007l` toggle the bit and both return
    /// `MODE_ACTION_NONE` (no TS fallback, no buffer switch).
    #[test]
    fn decset_1007_toggles_alternate_scroll_bit() {
        let mut core = TerminalCore::new(80, 24, 0);
        assert_eq!(core.handle_set_mode(1007, false), 0);
        assert!(!core.get_mode(MODE_ALTERNATE_SCROLL));
        assert_eq!(core.handle_set_mode(1007, true), 0);
        assert!(core.get_mode(MODE_ALTERNATE_SCROLL));
    }

    #[test]
    fn test_mode_synchronized_output_nested_set() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_set_mode(2026, true);
        core.handle_set_mode(2026, true); // second set is no-op
        assert!(core.get_mode(MODE_SYNCHRONIZED_OUTPUT));
        core.handle_set_mode(2026, false); // single reset clears
        assert!(!core.get_mode(MODE_SYNCHRONIZED_OUTPUT));
    }
}
