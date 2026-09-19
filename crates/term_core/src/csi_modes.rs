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

            // DECSET 1000/1002/1003/1006 (mouse tracking modes). Track the
            // bits core-side so the host can read them via `get_mode`
            // before deciding whether/how to report a pointer event,
            // exactly as the alternate-scroll mode (1007) already does.
            1000 => {
                self.set_mode(MODE_MOUSE_NORMAL_TRACKING, enable);
                MODE_ACTION_NONE
            }
            1002 => {
                self.set_mode(MODE_MOUSE_BUTTON_EVENT_TRACKING, enable);
                MODE_ACTION_NONE
            }
            1003 => {
                self.set_mode(MODE_MOUSE_ANY_EVENT_TRACKING, enable);
                MODE_ACTION_NONE
            }
            1006 => {
                self.set_mode(MODE_MOUSE_SGR_ENCODING, enable);
                MODE_ACTION_NONE
            }

            // Multi-valued modes: TS fallback
            1 | 1005 => MODE_ACTION_TS_FALLBACK,

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
        // 1000, 1002, 1003 and 1006 moved off the TS-fallback arm onto
        // their own bits (SC-1); only the modes still on that arm are
        // asserted here (AC-5).
        for mode in [1, 1005] {
            assert_eq!(
                core.handle_set_mode(mode, true),
                0xFF,
                "Mode {} should fallback",
                mode
            );
        }
        // Positive bit assertions replace the fallback assertion removed
        // above for the four moved modes (AC-5).
        assert_eq!(core.handle_set_mode(1000, true), 0);
        assert!(core.get_mode(MODE_MOUSE_NORMAL_TRACKING));
        assert_eq!(core.handle_set_mode(1002, true), 0);
        assert!(core.get_mode(MODE_MOUSE_BUTTON_EVENT_TRACKING));
        assert_eq!(core.handle_set_mode(1003, true), 0);
        assert!(core.get_mode(MODE_MOUSE_ANY_EVENT_TRACKING));
        assert_eq!(core.handle_set_mode(1006, true), 0);
        assert!(core.get_mode(MODE_MOUSE_SGR_ENCODING));
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

    // ── DECSET 1000/1002/1003/1006 (mouse tracking modes) ───

    /// TS-1: DECSET 1000/1002/1003/1006 each set and clear their own bit,
    /// returning `MODE_ACTION_NONE` in both directions, independently of
    /// the other three mouse-mode bits (AC-1, AC-2, AC-3).
    #[test]
    fn decset_mouse_modes_set_and_reset_toggle_their_own_bit() {
        let modes: [(u16, u8); 4] = [
            (1000, MODE_MOUSE_NORMAL_TRACKING),
            (1002, MODE_MOUSE_BUTTON_EVENT_TRACKING),
            (1003, MODE_MOUSE_ANY_EVENT_TRACKING),
            (1006, MODE_MOUSE_SGR_ENCODING),
        ];

        for (mode, bit) in modes {
            let mut core = TerminalCore::new(80, 24, 0);

            // AC-1: set returns no-action and the bit reports active
            assert_eq!(
                core.handle_set_mode(mode, true),
                0,
                "mode {mode} set returns no-action"
            );
            assert!(core.get_mode(bit), "mode {mode} bit active after set");

            // AC-3: the other three mouse bits stay inactive
            for (other_mode, other_bit) in modes {
                if other_mode != mode {
                    assert!(
                        !core.get_mode(other_bit),
                        "mode {mode} set leaves mode {other_mode} inactive"
                    );
                }
            }

            // AC-2: reset returns no-action and the bit reports inactive
            assert_eq!(
                core.handle_set_mode(mode, false),
                0,
                "mode {mode} reset returns no-action"
            );
            assert!(!core.get_mode(bit), "mode {mode} bit inactive after reset");
        }
    }

    /// AC-3: clearing one of the four already-active mouse-mode bits
    /// leaves the other three untouched.
    #[test]
    fn decset_mouse_modes_clearing_one_leaves_others_untouched() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_set_mode(1000, true);
        core.handle_set_mode(1002, true);
        core.handle_set_mode(1003, true);
        core.handle_set_mode(1006, true);

        core.handle_set_mode(1002, false);

        assert!(core.get_mode(MODE_MOUSE_NORMAL_TRACKING));
        assert!(!core.get_mode(MODE_MOUSE_BUTTON_EVENT_TRACKING));
        assert!(core.get_mode(MODE_MOUSE_ANY_EVENT_TRACKING));
        assert!(core.get_mode(MODE_MOUSE_SGR_ENCODING));
    }

    /// Edge case: setting an already-active mouse mode again, and
    /// clearing one that was never set, are both no-ops that still
    /// return `MODE_ACTION_NONE`.
    #[test]
    fn decset_mouse_mode_double_set_and_clear_of_unset_are_no_ops() {
        let mut core = TerminalCore::new(80, 24, 0);

        assert_eq!(core.handle_set_mode(1000, true), 0);
        assert_eq!(core.handle_set_mode(1000, true), 0);
        assert!(core.get_mode(MODE_MOUSE_NORMAL_TRACKING));

        assert_eq!(core.handle_set_mode(1002, false), 0);
        assert!(!core.get_mode(MODE_MOUSE_BUTTON_EVENT_TRACKING));
    }

    /// AC-4: mode 1005 is untouched by this task and still falls back to
    /// TS; mode 1015 is deliberately given no bit and keeps falling
    /// through the unknown-mode arm as a silent no-op.
    #[test]
    fn decset_1005_still_fallback_and_1015_still_unowned_no_op() {
        let mut core = TerminalCore::new(80, 24, 0);
        assert_eq!(core.handle_set_mode(1005, true), 0xFF);
        assert_eq!(core.handle_set_mode(1015, true), 0);
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
