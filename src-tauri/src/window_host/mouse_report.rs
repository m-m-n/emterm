//! Mouse-report decision / encoding layer: button-code composition, byte
//! encoding, the motion gate and the cell-change filter (IMPLEMENTATION.md
//! shared components SC-2 through SC-5).
//!
//! Owned by task0002, which is the only task that may change this
//! contract. Created here per cross-task decision D3: task0003 (this
//! worktree) is a parallel consumer of SC-2 through SC-5 and does not have
//! task0002's own edit, so this module is the minimum needed to satisfy the
//! pinned contract and let the routing integration (`pointer_routing.rs`)
//! compile and be tested. The integration (task0002) side of this file is
//! adopted verbatim on merge — see D3, IMPLEMENTATION.md.
//!
//! Every unit here is a pure function over plain values: no winit type, no
//! `term_core` type, no window/PTY handle. The caller (`pointer_routing.rs`)
//! collects those values from host state and calls in.

/// Button identity carried through the decision layer (SC-2, SC-5).
/// `None` denotes "no button" — the release/motion base under X10, or a
/// 1003 motion report with nothing held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ButtonIdentity {
    Left,
    Middle,
    Right,
    None,
}

/// Event kind for SC-2 button-code composition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EventKind {
    Press,
    Release,
    Motion,
    WheelUp,
    WheelDown,
}

/// Report encoding (SC-2 base selection, SC-3 byte layout).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Encoding {
    X10,
    Sgr,
}

/// SC-2: compose the numeric button code both encodings carry.
///
/// Post: base is left 0, middle 1, right 2, none 3; a wheel-up event is
/// base 64 and a wheel-down event is base 65 regardless of button identity;
/// a release under X10 replaces the base with 3 while a release under SGR
/// retains the press base; a motion event adds 32; ctrl adds 16; alt adds
/// 8. Shift is never added — the caller consumes it locally before an
/// event can reach this stage (D4), so no input combination ever
/// contributes the value 4.
pub(super) fn button_code(
    kind: EventKind,
    button: ButtonIdentity,
    encoding: Encoding,
    ctrl: bool,
    alt: bool,
) -> u8 {
    let base: u8 = match kind {
        EventKind::WheelUp => 64,
        EventKind::WheelDown => 65,
        EventKind::Release if encoding == Encoding::X10 => 3,
        _ => match button {
            ButtonIdentity::Left => 0,
            ButtonIdentity::Middle => 1,
            ButtonIdentity::Right => 2,
            ButtonIdentity::None => 3,
        },
    };
    let motion_bit = if kind == EventKind::Motion { 32 } else { 0 };
    let ctrl_bit = if ctrl { 16 } else { 0 };
    let alt_bit = if alt { 8 } else { 0 };
    base + motion_bit + ctrl_bit + alt_bit
}

/// SC-3: turn a button code plus a 1-based cell coordinate into the
/// emitted byte sequence.
///
/// Pre: `col` and `row` are 1-based and at least 1 (the caller's
/// responsibility). Post (X10): CSI, `M`, then three bytes — the button
/// code, the column and the row, each biased by 32 — absent (no bytes at
/// all, no clamping) when either coordinate exceeds 223. Post (SGR): CSI
/// `<`, the unbiased decimal button code, `;`, the decimal column, `;`,
/// the decimal row, then `M` for a press/motion or `m` for a release; no
/// coordinate limit.
pub(super) fn encode_report(
    code: u8,
    col: u32,
    row: u32,
    encoding: Encoding,
    release: bool,
) -> Option<Vec<u8>> {
    match encoding {
        Encoding::X10 => {
            if col > 223 || row > 223 {
                return None;
            }
            let mut buf = Vec::with_capacity(6);
            buf.extend_from_slice(b"\x1b[M");
            buf.push(code.wrapping_add(32));
            buf.push((col as u8).wrapping_add(32));
            buf.push((row as u8).wrapping_add(32));
            Some(buf)
        }
        Encoding::Sgr => {
            let letter = if release { 'm' } else { 'M' };
            Some(format!("\x1b[<{code};{col};{row}{letter}").into_bytes())
        }
    }
}

/// SC-4: cap motion report volume at grid resolution by suppressing a
/// repeat of the same cell.
///
/// Holds the last-reported cell. `should_report` answers true (and caches
/// the new cell) when no cell is cached or the cached cell differs; false
/// (cache unchanged) otherwise. `reset` empties the cache so the next cell
/// always reports — the host calls it whenever it observes that no
/// tracking mode is active, and on active-tab change (D7).
#[derive(Debug, Default)]
pub(super) struct CellChangeFilter {
    last: Option<(u32, u32)>,
}

impl CellChangeFilter {
    pub(super) fn should_report(&mut self, col: u32, row: u32) -> bool {
        if self.last == Some((col, row)) {
            false
        } else {
            self.last = Some((col, row));
            true
        }
    }

    pub(super) fn reset(&mut self) {
        self.last = None;
    }
}

/// Which buttons are currently held, for the SC-5 motion gate. When more
/// than one is held, [`HeldButtons::lowest`] resolves the tie per decision
/// D6 (left before middle before right).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct HeldButtons {
    pub(super) left: bool,
    pub(super) middle: bool,
    pub(super) right: bool,
}

impl HeldButtons {
    pub(super) fn any(&self) -> bool {
        self.left || self.middle || self.right
    }

    /// D6: the lowest-numbered held button, or `None` if nothing is held.
    pub(super) fn lowest(&self) -> ButtonIdentity {
        if self.left {
            ButtonIdentity::Left
        } else if self.middle {
            ButtonIdentity::Middle
        } else if self.right {
            ButtonIdentity::Right
        } else {
            ButtonIdentity::None
        }
    }
}

/// SC-5: decide whether a pointer motion is reportable at all, and with
/// which button identity.
///
/// Post: absent when no tracking mode is active or when only 1000 is
/// active; absent when 1002 is the highest active tracking mode and no
/// button is held; present with the held button when 1002 is active and at
/// least one button is held; present always when 1003 is active, carrying
/// the held button or the "none" identity when nothing is held. 1003 takes
/// precedence over 1002, which takes precedence over 1000.
pub(super) fn motion_gate(
    mode_1000: bool,
    mode_1002: bool,
    mode_1003: bool,
    held: HeldButtons,
) -> Option<ButtonIdentity> {
    if mode_1003 {
        Some(held.lowest())
    } else if mode_1002 {
        held.any().then(|| held.lowest())
    } else {
        // Only 1000 active, or no tracking mode at all: never reports.
        let _ = mode_1000;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── SC-2: button-code composition ───────────────────────────

    #[test]
    fn button_code_press_bases_by_identity() {
        assert_eq!(
            button_code(EventKind::Press, ButtonIdentity::Left, Encoding::X10, false, false),
            0
        );
        assert_eq!(
            button_code(EventKind::Press, ButtonIdentity::Middle, Encoding::X10, false, false),
            1
        );
        assert_eq!(
            button_code(EventKind::Press, ButtonIdentity::Right, Encoding::X10, false, false),
            2
        );
        assert_eq!(
            button_code(EventKind::Press, ButtonIdentity::None, Encoding::X10, false, false),
            3
        );
    }

    #[test]
    fn button_code_release_under_x10_is_always_base_3() {
        assert_eq!(
            button_code(EventKind::Release, ButtonIdentity::Left, Encoding::X10, false, false),
            3
        );
        assert_eq!(
            button_code(EventKind::Release, ButtonIdentity::Right, Encoding::X10, false, false),
            3
        );
    }

    #[test]
    fn button_code_release_under_sgr_retains_press_base() {
        assert_eq!(
            button_code(EventKind::Release, ButtonIdentity::Left, Encoding::Sgr, false, false),
            0
        );
        assert_eq!(
            button_code(EventKind::Release, ButtonIdentity::Right, Encoding::Sgr, false, false),
            2
        );
    }

    #[test]
    fn button_code_wheel_bases_ignore_button_identity() {
        assert_eq!(
            button_code(EventKind::WheelUp, ButtonIdentity::Right, Encoding::Sgr, false, false),
            64
        );
        assert_eq!(
            button_code(EventKind::WheelDown, ButtonIdentity::None, Encoding::X10, false, false),
            65
        );
    }

    #[test]
    fn button_code_motion_adds_32() {
        assert_eq!(
            button_code(EventKind::Motion, ButtonIdentity::Left, Encoding::Sgr, false, false),
            32
        );
    }

    #[test]
    fn button_code_ctrl_and_alt_bits_stack_and_never_produce_shift() {
        let ctrl = button_code(EventKind::Press, ButtonIdentity::Left, Encoding::Sgr, true, false);
        let alt = button_code(EventKind::Press, ButtonIdentity::Left, Encoding::Sgr, false, true);
        let both = button_code(EventKind::Press, ButtonIdentity::Left, Encoding::Sgr, true, true);
        assert_eq!(ctrl, 16);
        assert_eq!(alt, 8);
        assert_eq!(both, 24);
        // No parameter carries shift (D4): every combination avoids 4.
        for kind in [
            EventKind::Press,
            EventKind::Release,
            EventKind::Motion,
            EventKind::WheelUp,
            EventKind::WheelDown,
        ] {
            for button in [
                ButtonIdentity::Left,
                ButtonIdentity::Middle,
                ButtonIdentity::Right,
                ButtonIdentity::None,
            ] {
                for encoding in [Encoding::X10, Encoding::Sgr] {
                    for ctrl in [false, true] {
                        for alt in [false, true] {
                            assert_ne!(button_code(kind, button, encoding, ctrl, alt), 4);
                        }
                    }
                }
            }
        }
    }

    // ── SC-3: report encoding ───────────────────────────────────

    #[test]
    fn encode_report_x10_left_press_at_origin() {
        let code = button_code(EventKind::Press, ButtonIdentity::Left, Encoding::X10, false, false);
        let bytes = encode_report(code, 1, 1, Encoding::X10, false).unwrap();
        assert_eq!(bytes, vec![0x1b, b'[', b'M', 32, 33, 33]);
    }

    #[test]
    fn encode_report_x10_release_at_origin() {
        let code = button_code(EventKind::Release, ButtonIdentity::Left, Encoding::X10, false, false);
        let bytes = encode_report(code, 1, 1, Encoding::X10, true).unwrap();
        assert_eq!(bytes, vec![0x1b, b'[', b'M', 35, 33, 33]);
    }

    #[test]
    fn encode_report_x10_boundary_223_encodes_224_does_not() {
        assert!(encode_report(0, 223, 223, Encoding::X10, false).is_some());
        assert!(encode_report(0, 224, 1, Encoding::X10, false).is_none());
        assert!(encode_report(0, 1, 224, Encoding::X10, false).is_none());
    }

    #[test]
    fn encode_report_sgr_press_and_release_letters() {
        let press = encode_report(0, 1, 1, Encoding::Sgr, false).unwrap();
        assert_eq!(press, b"\x1b[<0;1;1M".to_vec());
        let release = encode_report(0, 1, 1, Encoding::Sgr, true).unwrap();
        assert_eq!(release, b"\x1b[<0;1;1m".to_vec());
    }

    #[test]
    fn encode_report_sgr_has_no_coordinate_limit() {
        let bytes = encode_report(0, 224, 500, Encoding::Sgr, false).unwrap();
        assert_eq!(bytes, b"\x1b[<0;224;500M".to_vec());
    }

    // ── SC-4: cell-change filter ─────────────────────────────────

    #[test]
    fn cell_change_filter_reports_first_cell_then_suppresses_repeat() {
        let mut filter = CellChangeFilter::default();
        assert!(filter.should_report(5, 5));
        assert!(!filter.should_report(5, 5));
    }

    #[test]
    fn cell_change_filter_reports_on_crossing_and_resets() {
        let mut filter = CellChangeFilter::default();
        assert!(filter.should_report(5, 5));
        assert!(filter.should_report(6, 5));
        assert!(!filter.should_report(6, 5));
        filter.reset();
        assert!(filter.should_report(6, 5));
    }

    // ── SC-5: motion gate ─────────────────────────────────────────

    #[test]
    fn motion_gate_no_tracking_or_1000_only_never_reports() {
        assert_eq!(motion_gate(false, false, false, HeldButtons::default()), None);
        assert_eq!(motion_gate(true, false, false, HeldButtons::default()), None);
    }

    #[test]
    fn motion_gate_1002_requires_a_held_button() {
        assert_eq!(motion_gate(false, true, false, HeldButtons::default()), None);
        let held = HeldButtons {
            left: true,
            ..Default::default()
        };
        assert_eq!(motion_gate(false, true, false, held), Some(ButtonIdentity::Left));
    }

    #[test]
    fn motion_gate_1003_always_reports() {
        assert_eq!(
            motion_gate(false, false, true, HeldButtons::default()),
            Some(ButtonIdentity::None)
        );
        let held = HeldButtons {
            right: true,
            ..Default::default()
        };
        assert_eq!(motion_gate(false, false, true, held), Some(ButtonIdentity::Right));
    }

    #[test]
    fn motion_gate_multi_button_carries_the_lowest_numbered() {
        let held = HeldButtons {
            left: true,
            right: true,
            ..Default::default()
        };
        assert_eq!(motion_gate(false, false, true, held), Some(ButtonIdentity::Left));
        let held = HeldButtons {
            middle: true,
            right: true,
            ..Default::default()
        };
        assert_eq!(motion_gate(false, true, false, held), Some(ButtonIdentity::Middle));
    }

    #[test]
    fn motion_gate_precedence_1003_over_1002_over_1000() {
        let held = HeldButtons::default();
        // 1003 wins even with 1000/1002 also on, and reports "none" here.
        assert_eq!(motion_gate(true, true, true, held), Some(ButtonIdentity::None));
    }
}
