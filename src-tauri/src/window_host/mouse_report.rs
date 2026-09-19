//! task0002 (mouse-reporting): SC-2 through SC-5, IMPLEMENTATION.md's
//! window-free mouse-report decision layer — button-code composition, byte
//! encoding, the motion gate, and the cell-change filter.
//!
//! task0004 adds SC-8 (the grid-ownership decision) and SC-9 (the gesture-
//! ownership record) to this same window-free layer — see D10/D11.
//!
//! Every function/type here takes and returns plain values: no window
//! handle, no GPU surface, no PTY, no `term_core` mode type, no winit type
//! in any signature. That is what makes every unit exercisable from a bare
//! `#[test]` with no window, GPU surface or PTY constructed (AC-8). The L3
//! routing layer (task0003, task0004) is responsible for collecting
//! winit/egui/core state into these plain values and performing the side
//! effect with the result.
//!
//! `#![allow(dead_code)]`: this module is a decision layer consumed by the
//! routing task (task0003), which lands in a separate parallel worktree
//! (IMPLEMENTATION.md D3) and is not present here — so most items have no
//! caller yet outside this file's own tests. Mirrors the same allowance
//! already used by `crate::pty::input` for the same reason.
#![allow(dead_code)]

use crate::pty::input::Modifiers;

/// Which mouse button (or none) an event/report concerns (SC-2, SC-5).
/// Deliberately distinct from `winit::event::MouseButton` and
/// `egui::PointerButton` — L2 must not reference either in a signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MouseButtonId {
    Left,
    Middle,
    Right,
    /// SC-5's "none" identity: under mode 1003 a motion report is always
    /// present, even when no button is held.
    None,
}

/// The kind of pointer/wheel event a report describes (SC-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MouseEventKind {
    Press,
    Release,
    Motion,
    WheelUp,
    WheelDown,
}

/// Which report form to compose bytes for (SC-2, SC-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MouseReportEncoding {
    X10,
    Sgr,
}

const BASE_LEFT: u8 = 0;
const BASE_MIDDLE: u8 = 1;
const BASE_RIGHT: u8 = 2;
const BASE_NONE: u8 = 3;
const BASE_WHEEL_UP: u8 = 64;
const BASE_WHEEL_DOWN: u8 = 65;
const MOTION_BIT: u8 = 32;
const CTRL_BIT: u8 = 16;
const ALT_BIT: u8 = 8;

/// X10's per-axis coordinate ceiling: `column + 32` / `row + 32` must fit in
/// one byte (FR9). Above this, [`encode_report`] returns `None` rather than
/// clamping or emitting a partial sequence.
const X10_MAX_COORD: u32 = 223;

/// SC-2: compose the numeric button code both encodings carry.
///
/// `mods.shift` is intentionally never read: FR7/D4 route Shift through the
/// local override before any event reaches emission, so the conventional
/// shift bit (4) is never contributed by any input combination (AC-3).
pub(super) fn compose_button_code(
    kind: MouseEventKind,
    button: MouseButtonId,
    encoding: MouseReportEncoding,
    mods: Modifiers,
) -> u8 {
    let mut code = match kind {
        MouseEventKind::WheelUp => BASE_WHEEL_UP,
        MouseEventKind::WheelDown => BASE_WHEEL_DOWN,
        // x10 replaces the base with 3 on release; sgr retains the press
        // base, so a plain Release falls through to the button lookup below.
        MouseEventKind::Release if encoding == MouseReportEncoding::X10 => BASE_NONE,
        _ => match button {
            MouseButtonId::Left => BASE_LEFT,
            MouseButtonId::Middle => BASE_MIDDLE,
            MouseButtonId::Right => BASE_RIGHT,
            MouseButtonId::None => BASE_NONE,
        },
    };
    if matches!(kind, MouseEventKind::Motion) {
        code += MOTION_BIT;
    }
    if mods.ctrl {
        code += CTRL_BIT;
    }
    if mods.alt {
        code += ALT_BIT;
    }
    code
}

/// SC-3: turn a button code plus a 1-based cell coordinate into the emitted
/// byte sequence.
///
/// **Pre**: `column` and `row` are 1-based (at least 1).
/// **Post (x10)**: `ESC [ M` then exactly three bytes — the button code, the
/// column and the row, each biased by 32 — or `None` when the column or the
/// row exceeds 223 (FR9): never a clamped value, never a partial sequence.
/// **Post (sgr)**: `ESC [ <` then the unbiased decimal button code, `;`, the
/// decimal column, `;`, the decimal row, then `M` for a press/motion or `m`
/// for a release; no coordinate limit.
pub(super) fn encode_report(
    button_code: u8,
    column: u32,
    row: u32,
    encoding: MouseReportEncoding,
    is_release: bool,
) -> Option<Vec<u8>> {
    match encoding {
        MouseReportEncoding::X10 => {
            if column > X10_MAX_COORD || row > X10_MAX_COORD {
                return None;
            }
            let mut bytes = Vec::with_capacity(6);
            bytes.extend_from_slice(b"\x1b[M");
            bytes.push(button_code + 32);
            bytes.push(column as u8 + 32);
            bytes.push(row as u8 + 32);
            Some(bytes)
        }
        MouseReportEncoding::Sgr => {
            let final_char = if is_release { 'm' } else { 'M' };
            Some(format!("\x1b[<{button_code};{column};{row}{final_char}").into_bytes())
        }
    }
}

/// SC-4: caps motion-report volume at grid resolution by remembering the
/// last reported cell. Owns its cache; deciding *when* to call [`reset`]
/// (D7: no active tracking mode, and active-tab change) is the caller's
/// job, not this filter's.
///
/// [`reset`]: CellChangeFilter::reset
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct CellChangeFilter {
    last: Option<(u32, u32)>,
}

impl CellChangeFilter {
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// `true` (and caches `(column, row)`) when nothing is cached yet or the
    /// cached cell differs; `false` (cache left unchanged) on an exact
    /// repeat of the cached cell.
    pub(super) fn should_report(&mut self, column: u32, row: u32) -> bool {
        if self.last == Some((column, row)) {
            return false;
        }
        self.last = Some((column, row));
        true
    }

    /// Empties the cache so the next [`should_report`] call always reports,
    /// regardless of what cell it names.
    ///
    /// [`should_report`]: CellChangeFilter::should_report
    pub(super) fn reset(&mut self) {
        self.last = None;
    }
}

/// SC-5: decide whether a pointer motion is reportable at all, and with
/// which button, given the three tracking-mode flags and which buttons are
/// currently held.
///
/// **Post**: `None` when no tracking mode is active, or when only 1000 is
/// active; `None` when 1002 is the highest active mode and no button is
/// held; `Some(button)` when 1002 is active and at least one button is
/// held; always `Some(_)` when 1003 is active — the held button, or the
/// "none" identity when none is held. 1003 takes precedence over 1002,
/// which takes precedence over 1000. When several buttons are held, the
/// lowest-numbered one (left, then middle, then right) is reported (D6).
pub(super) fn motion_gate(
    mode_1000: bool,
    mode_1002: bool,
    mode_1003: bool,
    held_left: bool,
    held_middle: bool,
    held_right: bool,
) -> Option<MouseButtonId> {
    let lowest_held = if held_left {
        Some(MouseButtonId::Left)
    } else if held_middle {
        Some(MouseButtonId::Middle)
    } else if held_right {
        Some(MouseButtonId::Right)
    } else {
        None
    };
    if mode_1003 {
        return Some(lowest_held.unwrap_or(MouseButtonId::None));
    }
    if mode_1002 {
        return lowest_held;
    }
    // mode_1000 alone (or no tracking mode active at all) never reports.
    let _ = mode_1000;
    None
}

// ── task0004: SC-8 grid-ownership decision (D11) ──────────────────────

/// SC-8 (D11): the plain-value inputs to the grid-ownership decision — one
/// bool per chrome region the host already hit-tests (the top strip, the
/// bottom strip, the right-edge scrollbar overlay, the mux sidebar in
/// whichever placement is active, the CSD edge-resize hot zone), plus
/// whether the profile selector is visible. Each bool is the SAME hit-test
/// result the existing chrome guards already compute — this decision does
/// not re-derive any geometry itself, it only combines the results into one
/// identity-independent answer. Deliberately carries no button identity, no
/// press/release flag and no event kind: that omission is what makes
/// [`point_belongs_to_grid`] return the same answer for a left press, a
/// right release, a motion and a wheel notch at the same position (AC-1).
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct GridOwnershipInputs {
    pub(super) in_top_strip: bool,
    pub(super) in_bottom_strip: bool,
    pub(super) in_scrollbar_overlay: bool,
    pub(super) in_mux_sidebar: bool,
    pub(super) in_resize_hot_zone: bool,
    pub(super) profile_selector_visible: bool,
}

/// SC-8 (D11): true only when the position lies over the terminal grid —
/// none of the chrome regions in [`GridOwnershipInputs`] claim it, and the
/// profile selector is not visible. **Pre**: the caller evaluates this
/// before any reporting work on every emitting path (before the motion gate
/// and the cell-change filter on the motion path; before the tracking-
/// active read on the wheel path). **Post**: `false` rejects the top strip,
/// the bottom strip, the scrollbar overlay, the mux sidebar and the CSD
/// edge-resize hot zone, and rejects every position while the profile
/// selector is visible; `true` only when none of those apply.
pub(super) fn point_belongs_to_grid(inputs: GridOwnershipInputs) -> bool {
    if inputs.profile_selector_visible {
        return false;
    }
    !(inputs.in_top_strip
        || inputs.in_bottom_strip
        || inputs.in_scrollbar_overlay
        || inputs.in_mux_sidebar
        || inputs.in_resize_hot_zone)
}

// ── task0004: SC-9 gesture-ownership record (D10) ─────────────────────

/// SC-9 (D10): which side owns an in-flight button gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GestureOwner {
    /// The press was reported; the matching release must report too,
    /// whatever the Shift state has become by release time.
    Report,
    /// The press was taken locally (SC-8 rejected the position, no
    /// tracking mode was active, or Shift was held); the matching release
    /// completes the local gesture, whatever the Shift state has become
    /// by release time.
    Local,
}

/// SC-9 (D10): records, per button identity, which side took the button's
/// press, so the matching release routes to the same side regardless of
/// the Shift state at release time. A press [`point_belongs_to_grid`]
/// rejects records no owner at all — its release then finds nothing and
/// routes nowhere new. Three independent slots (left/middle/right): a
/// second button pressed mid-gesture is owned independently of the first.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct GestureOwnership {
    left: Option<GestureOwner>,
    middle: Option<GestureOwner>,
    right: Option<GestureOwner>,
}

impl GestureOwnership {
    pub(super) fn new() -> Self {
        Self::default()
    }

    fn slot(&mut self, button: MouseButtonId) -> Option<&mut Option<GestureOwner>> {
        match button {
            MouseButtonId::Left => Some(&mut self.left),
            MouseButtonId::Middle => Some(&mut self.middle),
            MouseButtonId::Right => Some(&mut self.right),
            // SC-5's "none" identity never owns a button gesture — a
            // gesture is always keyed by a physical left/middle/right
            // press, never the 1003 "no button held" motion identity.
            MouseButtonId::None => None,
        }
    }

    /// Record which side took `button`'s press. Overwrites any stale
    /// owner still recorded for the same identity — a fresh press always
    /// starts a fresh gesture.
    pub(super) fn record_press(&mut self, button: MouseButtonId, owner: GestureOwner) {
        if let Some(slot) = self.slot(button) {
            *slot = Some(owner);
        }
    }

    /// Read-and-clear the owner recorded for `button`'s press, routing its
    /// matching release. `None` when no press for this identity was
    /// recorded (SC-8 rejected the press, or no press preceded this
    /// release at all) — the caller then falls through to its existing,
    /// unowned handling.
    pub(super) fn take(&mut self, button: MouseButtonId) -> Option<GestureOwner> {
        self.slot(button).and_then(|slot| slot.take())
    }

    /// Clears every recorded owner. **Pre**: called on the same two
    /// observations that reset [`CellChangeFilter`] (D7) — the host
    /// observing that no tracking mode is active, and an active-tab change
    /// — so a mode cleared mid-gesture strands no record.
    pub(super) fn clear_all(&mut self) {
        self.left = None;
        self.middle = None;
        self.right = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── AC-1: X10 byte-exact encoding (TS-3) ───────────────────────

    #[test]
    fn x10_left_press_at_1_1_emits_introducer_then_32_33_33() {
        let code = compose_button_code(
            MouseEventKind::Press,
            MouseButtonId::Left,
            MouseReportEncoding::X10,
            Modifiers::NONE,
        );
        let bytes = encode_report(code, 1, 1, MouseReportEncoding::X10, false).unwrap();
        assert_eq!(bytes, vec![0x1b, b'[', b'M', 32, 33, 33]);
    }

    #[test]
    fn x10_left_release_at_1_1_emits_35_33_33_with_base_replaced_by_3() {
        let code = compose_button_code(
            MouseEventKind::Release,
            MouseButtonId::Left,
            MouseReportEncoding::X10,
            Modifiers::NONE,
        );
        assert_eq!(code, 3, "x10 release replaces the base with 3");
        let bytes = encode_report(code, 1, 1, MouseReportEncoding::X10, true).unwrap();
        assert_eq!(bytes, vec![0x1b, b'[', b'M', 35, 33, 33]);
    }

    #[test]
    fn x10_motion_adds_32_to_the_base() {
        let code = compose_button_code(
            MouseEventKind::Motion,
            MouseButtonId::Left,
            MouseReportEncoding::X10,
            Modifiers::NONE,
        );
        assert_eq!(code, 32);
        let bytes = encode_report(code, 1, 1, MouseReportEncoding::X10, false).unwrap();
        assert_eq!(bytes, vec![0x1b, b'[', b'M', 64, 33, 33]);
    }

    #[test]
    fn x10_wheel_up_and_wheel_down_carry_64_and_65() {
        let up = compose_button_code(
            MouseEventKind::WheelUp,
            MouseButtonId::None,
            MouseReportEncoding::X10,
            Modifiers::NONE,
        );
        let down = compose_button_code(
            MouseEventKind::WheelDown,
            MouseButtonId::None,
            MouseReportEncoding::X10,
            Modifiers::NONE,
        );
        assert_eq!(up, 64);
        assert_eq!(down, 65);
        assert_eq!(
            encode_report(up, 1, 1, MouseReportEncoding::X10, false).unwrap(),
            vec![0x1b, b'[', b'M', 96, 33, 33]
        );
        assert_eq!(
            encode_report(down, 1, 1, MouseReportEncoding::X10, false).unwrap(),
            vec![0x1b, b'[', b'M', 97, 33, 33]
        );
    }

    // ── AC-2: SGR byte-exact encoding (TS-4) ───────────────────────

    #[test]
    fn sgr_left_press_at_1_1_is_lt_0_1_1_upper_m() {
        let code = compose_button_code(
            MouseEventKind::Press,
            MouseButtonId::Left,
            MouseReportEncoding::Sgr,
            Modifiers::NONE,
        );
        let bytes = encode_report(code, 1, 1, MouseReportEncoding::Sgr, false).unwrap();
        assert_eq!(bytes, b"\x1b[<0;1;1M".to_vec());
    }

    #[test]
    fn sgr_left_release_retains_press_base_and_ends_in_lowercase_m() {
        let code = compose_button_code(
            MouseEventKind::Release,
            MouseButtonId::Left,
            MouseReportEncoding::Sgr,
            Modifiers::NONE,
        );
        assert_eq!(code, 0, "sgr release keeps the press base, unlike x10");
        let bytes = encode_report(code, 1, 1, MouseReportEncoding::Sgr, true).unwrap();
        assert_eq!(bytes, b"\x1b[<0;1;1m".to_vec());
    }

    #[test]
    fn sgr_motion_and_wheel_directions_are_byte_exact() {
        let motion = compose_button_code(
            MouseEventKind::Motion,
            MouseButtonId::Right,
            MouseReportEncoding::Sgr,
            Modifiers::NONE,
        );
        assert_eq!(
            encode_report(motion, 5, 7, MouseReportEncoding::Sgr, false).unwrap(),
            b"\x1b[<34;5;7M".to_vec(), // right (2) + motion (32)
        );

        let up = compose_button_code(
            MouseEventKind::WheelUp,
            MouseButtonId::None,
            MouseReportEncoding::Sgr,
            Modifiers::NONE,
        );
        assert_eq!(
            encode_report(up, 1, 1, MouseReportEncoding::Sgr, false).unwrap(),
            b"\x1b[<64;1;1M".to_vec()
        );

        let down = compose_button_code(
            MouseEventKind::WheelDown,
            MouseButtonId::None,
            MouseReportEncoding::Sgr,
            Modifiers::NONE,
        );
        assert_eq!(
            encode_report(down, 1, 1, MouseReportEncoding::Sgr, false).unwrap(),
            b"\x1b[<65;1;1M".to_vec()
        );
    }

    // ── AC-3: modifier contribution; shift never reaches the code (TS-5) ──

    #[test]
    fn ctrl_and_alt_contribute_16_and_8_and_24_together() {
        let base = compose_button_code(
            MouseEventKind::Press,
            MouseButtonId::Left,
            MouseReportEncoding::Sgr,
            Modifiers::NONE,
        );
        let ctrl = compose_button_code(
            MouseEventKind::Press,
            MouseButtonId::Left,
            MouseReportEncoding::Sgr,
            Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
        );
        let alt = compose_button_code(
            MouseEventKind::Press,
            MouseButtonId::Left,
            MouseReportEncoding::Sgr,
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
        );
        let both = compose_button_code(
            MouseEventKind::Press,
            MouseButtonId::Left,
            MouseReportEncoding::Sgr,
            Modifiers {
                ctrl: true,
                alt: true,
                ..Modifiers::NONE
            },
        );
        assert_eq!(ctrl - base, 16);
        assert_eq!(alt - base, 8);
        assert_eq!(both - base, 24);
    }

    #[test]
    fn shift_alone_never_changes_the_button_code_even_when_set() {
        let without_shift = compose_button_code(
            MouseEventKind::WheelUp,
            MouseButtonId::None,
            MouseReportEncoding::X10,
            Modifiers::NONE,
        );
        let with_shift = compose_button_code(
            MouseEventKind::WheelUp,
            MouseButtonId::None,
            MouseReportEncoding::X10,
            Modifiers {
                shift: true,
                ..Modifiers::NONE
            },
        );
        assert_eq!(without_shift, with_shift);

        let ctrl_only = compose_button_code(
            MouseEventKind::Press,
            MouseButtonId::Right,
            MouseReportEncoding::Sgr,
            Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
        );
        let ctrl_and_shift = compose_button_code(
            MouseEventKind::Press,
            MouseButtonId::Right,
            MouseReportEncoding::Sgr,
            Modifiers {
                ctrl: true,
                shift: true,
                ..Modifiers::NONE
            },
        );
        assert_eq!(
            ctrl_only, ctrl_and_shift,
            "shift set alongside another modifier must still contribute nothing"
        );
    }

    #[test]
    fn no_input_combination_ever_produces_the_value_4() {
        let kinds = [
            MouseEventKind::Press,
            MouseEventKind::Release,
            MouseEventKind::Motion,
            MouseEventKind::WheelUp,
            MouseEventKind::WheelDown,
        ];
        let buttons = [
            MouseButtonId::Left,
            MouseButtonId::Middle,
            MouseButtonId::Right,
            MouseButtonId::None,
        ];
        let encodings = [MouseReportEncoding::X10, MouseReportEncoding::Sgr];
        let bools = [false, true];
        for &kind in &kinds {
            for &button in &buttons {
                for &encoding in &encodings {
                    for &ctrl in &bools {
                        for &alt in &bools {
                            for &shift in &bools {
                                let code = compose_button_code(
                                    kind,
                                    button,
                                    encoding,
                                    Modifiers { ctrl, shift, alt },
                                );
                                assert_ne!(code, 4);
                            }
                        }
                    }
                }
            }
        }
    }

    // ── AC-4: X10 overflow at 224, independently on each axis (TS-6) ──

    #[test]
    fn x10_column_223_encodes_but_224_yields_nothing() {
        let code = compose_button_code(
            MouseEventKind::Press,
            MouseButtonId::Left,
            MouseReportEncoding::X10,
            Modifiers::NONE,
        );
        assert!(encode_report(code, 223, 1, MouseReportEncoding::X10, false).is_some());
        assert_eq!(
            encode_report(code, 224, 1, MouseReportEncoding::X10, false),
            None
        );
    }

    #[test]
    fn x10_row_223_encodes_but_224_yields_nothing() {
        let code = compose_button_code(
            MouseEventKind::Press,
            MouseButtonId::Left,
            MouseReportEncoding::X10,
            Modifiers::NONE,
        );
        assert!(encode_report(code, 1, 223, MouseReportEncoding::X10, false).is_some());
        assert_eq!(
            encode_report(code, 1, 224, MouseReportEncoding::X10, false),
            None
        );
    }

    #[test]
    fn sgr_carries_the_true_coordinate_at_224_where_x10_would_suppress() {
        let code = compose_button_code(
            MouseEventKind::Press,
            MouseButtonId::Left,
            MouseReportEncoding::Sgr,
            Modifiers::NONE,
        );
        let bytes = encode_report(code, 224, 224, MouseReportEncoding::Sgr, false).unwrap();
        assert_eq!(bytes, b"\x1b[<0;224;224M".to_vec());
    }

    // ── AC-5: cell-change filter (TS-7) ─────────────────────────────

    #[test]
    fn cell_change_filter_reports_first_suppresses_repeat_reports_on_change() {
        let mut filter = CellChangeFilter::new();
        let mut reported = Vec::new();
        for cell in [(1, 1), (1, 1), (2, 1), (2, 1), (3, 3)] {
            if filter.should_report(cell.0, cell.1) {
                reported.push(cell);
            }
        }
        assert_eq!(reported, vec![(1, 1), (2, 1), (3, 3)]);
    }

    #[test]
    fn cell_change_filter_carries_its_cached_cell_across_calls() {
        let mut filter = CellChangeFilter::new();
        assert!(filter.should_report(10, 20));
        assert!(!filter.should_report(10, 20));
        assert!(!filter.should_report(10, 20));
        assert!(filter.should_report(11, 20));
    }

    #[test]
    fn cell_change_filter_reports_unconditionally_after_reset() {
        let mut filter = CellChangeFilter::new();
        assert!(filter.should_report(5, 5));
        assert!(!filter.should_report(5, 5));
        filter.reset();
        assert!(
            filter.should_report(5, 5),
            "reset must clear the cache even for a repeat of the same cell"
        );
    }

    // ── AC-6: motion gate (TS-9) ─────────────────────────────────────

    #[test]
    fn motion_gate_yields_nothing_when_only_1000_is_active() {
        assert_eq!(motion_gate(true, false, false, true, false, false), None);
        assert_eq!(motion_gate(true, false, false, false, false, false), None);
    }

    #[test]
    fn motion_gate_yields_nothing_under_1002_with_no_button_held() {
        assert_eq!(motion_gate(false, true, false, false, false, false), None);
        assert_eq!(motion_gate(true, true, false, false, false, false), None);
    }

    #[test]
    fn motion_gate_yields_held_button_under_1002() {
        assert_eq!(
            motion_gate(false, true, false, false, true, false),
            Some(MouseButtonId::Middle)
        );
    }

    #[test]
    fn motion_gate_always_yields_under_1003() {
        assert_eq!(
            motion_gate(false, false, true, false, false, false),
            Some(MouseButtonId::None)
        );
        assert_eq!(
            motion_gate(false, false, true, false, false, true),
            Some(MouseButtonId::Right)
        );
    }

    #[test]
    fn motion_gate_reports_lowest_numbered_button_when_several_are_held() {
        // D6: left and right both held under 1002 reports left.
        assert_eq!(
            motion_gate(false, true, false, true, false, true),
            Some(MouseButtonId::Left)
        );
        // All three held under 1003 still reports left.
        assert_eq!(
            motion_gate(false, false, true, true, true, true),
            Some(MouseButtonId::Left)
        );
    }

    #[test]
    fn motion_gate_precedence_is_1003_then_1002_then_1000() {
        // 1003 present alongside 1000/1002 still takes the always-present path.
        assert_eq!(
            motion_gate(true, true, true, false, false, false),
            Some(MouseButtonId::None)
        );
    }

    #[test]
    fn motion_gate_yields_nothing_when_no_tracking_mode_is_active() {
        assert_eq!(motion_gate(false, false, false, true, true, true), None);
    }

    // ── task0004 AC-1/AC-2: grid-ownership decision (SC-8, D11), TS-19 ──

    /// Baseline: no chrome region claims the position and the profile
    /// selector is hidden — the position belongs to the grid.
    #[test]
    fn point_belongs_to_grid_true_when_no_guard_is_active() {
        assert!(point_belongs_to_grid(GridOwnershipInputs::default()));
    }

    /// AC-1: each guarded region named in the criterion, tested in
    /// isolation with every other input at its default (`false`), rejects
    /// the position.
    #[test]
    fn point_belongs_to_grid_rejects_each_guarded_region_independently() {
        let cases: [(&str, GridOwnershipInputs); 6] = [
            (
                "top strip",
                GridOwnershipInputs {
                    in_top_strip: true,
                    ..GridOwnershipInputs::default()
                },
            ),
            (
                "bottom strip",
                GridOwnershipInputs {
                    in_bottom_strip: true,
                    ..GridOwnershipInputs::default()
                },
            ),
            (
                "scrollbar overlay",
                GridOwnershipInputs {
                    in_scrollbar_overlay: true,
                    ..GridOwnershipInputs::default()
                },
            ),
            (
                "mux sidebar (persistent or overlay — the caller collapses \
                 both placements into this one bool)",
                GridOwnershipInputs {
                    in_mux_sidebar: true,
                    ..GridOwnershipInputs::default()
                },
            ),
            (
                "CSD edge-resize hot zone",
                GridOwnershipInputs {
                    in_resize_hot_zone: true,
                    ..GridOwnershipInputs::default()
                },
            ),
            (
                "profile selector visible",
                GridOwnershipInputs {
                    profile_selector_visible: true,
                    ..GridOwnershipInputs::default()
                },
            ),
        ];
        for (name, inputs) in cases {
            assert!(
                !point_belongs_to_grid(inputs),
                "{name} must reject the position"
            );
        }
    }

    /// AC-1: several guarded regions active at once still reject — the
    /// decision is an OR over every region, not a priority scheme that
    /// could let one region's `false` mask another's `true`.
    #[test]
    fn point_belongs_to_grid_rejects_when_several_regions_overlap() {
        assert!(!point_belongs_to_grid(GridOwnershipInputs {
            in_bottom_strip: true,
            in_scrollbar_overlay: true,
            ..GridOwnershipInputs::default()
        }));
    }

    /// AC-1: the profile-selector rejection is unconditional — even a
    /// position that no region geometry claims is still rejected while
    /// the selector is visible.
    #[test]
    fn point_belongs_to_grid_rejects_unconditionally_while_profile_selector_visible() {
        assert!(!point_belongs_to_grid(GridOwnershipInputs {
            profile_selector_visible: true,
            ..GridOwnershipInputs::default()
        }));
    }

    // ── task0004 AC-5: gesture-ownership record (SC-9, D10), TS-20 ──────

    /// TS-20 ordering A: Shift held at press records local ownership;
    /// Shift's state at release time is not a parameter `take` can even
    /// consult, so the release still routes to Local regardless of what
    /// Shift does in between.
    #[test]
    fn gesture_ownership_routes_release_to_local_when_press_was_local() {
        let mut owner = GestureOwnership::new();
        owner.record_press(MouseButtonId::Left, GestureOwner::Local);
        assert_eq!(owner.take(MouseButtonId::Left), Some(GestureOwner::Local));
    }

    /// TS-20 ordering B: Shift not held at press records report ownership;
    /// the release still routes to Report regardless of Shift arriving
    /// before the release.
    #[test]
    fn gesture_ownership_routes_release_to_report_when_press_was_reported() {
        let mut owner = GestureOwnership::new();
        owner.record_press(MouseButtonId::Right, GestureOwner::Report);
        assert_eq!(owner.take(MouseButtonId::Right), Some(GestureOwner::Report));
    }

    /// A press SC-8 rejects records no ownership at all — its release
    /// finds nothing and routes nowhere new (the caller falls through to
    /// its existing, unowned handling).
    #[test]
    fn gesture_ownership_take_yields_none_when_no_press_was_recorded() {
        let mut owner = GestureOwnership::new();
        assert_eq!(owner.take(MouseButtonId::Middle), None);
    }

    /// `take` is read-AND-clear: a second release for the same identity
    /// with no intervening press finds nothing.
    #[test]
    fn gesture_ownership_take_is_read_and_clear() {
        let mut owner = GestureOwnership::new();
        owner.record_press(MouseButtonId::Left, GestureOwner::Report);
        assert_eq!(owner.take(MouseButtonId::Left), Some(GestureOwner::Report));
        assert_eq!(owner.take(MouseButtonId::Left), None);
    }

    /// Test Notes edge case: a second button pressed mid-gesture is owned
    /// independently of the first.
    #[test]
    fn gesture_ownership_tracks_two_buttons_independently() {
        let mut owner = GestureOwnership::new();
        owner.record_press(MouseButtonId::Left, GestureOwner::Report);
        owner.record_press(MouseButtonId::Middle, GestureOwner::Local);
        assert_eq!(owner.take(MouseButtonId::Left), Some(GestureOwner::Report));
        assert_eq!(owner.take(MouseButtonId::Middle), Some(GestureOwner::Local));
    }

    /// Test Notes edge case: a press inside the grid whose release
    /// arrives while the pointer sits over a guarded region. `take` has
    /// no position parameter, so ownership alone decides — a guarded
    /// release position cannot change the answer.
    #[test]
    fn gesture_ownership_release_routing_does_not_depend_on_release_position() {
        let mut owner = GestureOwnership::new();
        owner.record_press(MouseButtonId::Left, GestureOwner::Report);
        assert_eq!(owner.take(MouseButtonId::Left), Some(GestureOwner::Report));
    }

    /// `clear_all` wipes every recorded owner — the D7/SC-9 pre for the
    /// operation the host calls on the same two observations that reset
    /// the cell-change filter.
    #[test]
    fn gesture_ownership_clear_all_wipes_every_button() {
        let mut owner = GestureOwnership::new();
        owner.record_press(MouseButtonId::Left, GestureOwner::Report);
        owner.record_press(MouseButtonId::Middle, GestureOwner::Local);
        owner.record_press(MouseButtonId::Right, GestureOwner::Report);
        owner.clear_all();
        assert_eq!(owner.take(MouseButtonId::Left), None);
        assert_eq!(owner.take(MouseButtonId::Middle), None);
        assert_eq!(owner.take(MouseButtonId::Right), None);
    }

    /// A fresh press for an identity overwrites any stale record still
    /// held for it (recording is idempotent per identity per SC-9's
    /// contract: a second press always starts a fresh gesture).
    #[test]
    fn gesture_ownership_record_press_overwrites_a_stale_owner() {
        let mut owner = GestureOwnership::new();
        owner.record_press(MouseButtonId::Left, GestureOwner::Local);
        owner.record_press(MouseButtonId::Left, GestureOwner::Report);
        assert_eq!(owner.take(MouseButtonId::Left), Some(GestureOwner::Report));
    }

    /// `MouseButtonId::None` (SC-5's "no button held" 1003 identity) never
    /// has a slot — recording or taking it is a no-op, not a panic.
    #[test]
    fn gesture_ownership_none_identity_is_a_no_op() {
        let mut owner = GestureOwnership::new();
        owner.record_press(MouseButtonId::None, GestureOwner::Report);
        assert_eq!(owner.take(MouseButtonId::None), None);
    }
}
