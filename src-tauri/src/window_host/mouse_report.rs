//! task0002 (mouse-reporting): SC-2 through SC-5, IMPLEMENTATION.md's
//! window-free mouse-report decision layer — button-code composition, byte
//! encoding, the motion gate, and the cell-change filter.
//!
//! task0004 adds SC-8 (the grid-ownership decision) and SC-9 (the gesture-
//! ownership record) to this same window-free layer — see D10/D11.
//!
//! task0005 adds SC-10 (the pointer decision sequence, one unit per
//! pointer path: button, motion, wheel) and SC-11 (the outcome
//! application) — see D12. Both consult SC-2 through SC-9 unaltered; this
//! is the seam `task0006` reduces the three real pointer handlers in
//! `pointer_routing.rs` onto (gather / decide / apply / perform-local-arm).
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

use super::input_translate::{WheelConsumer, wheel_consumer};

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

    /// task0005 (SC-10): read-only counterpart of [`should_report`] — same
    /// answer, but never mutates the cache. SC-10's decision sequences must
    /// not mutate a record themselves (the update travels in the returned
    /// outcome instead, for [`commit`] to apply); this is what lets a
    /// sequence unit consult "would this cell report" without side effects.
    ///
    /// [`should_report`]: CellChangeFilter::should_report
    /// [`commit`]: CellChangeFilter::commit
    pub(super) fn would_report(&self, column: u32, row: u32) -> bool {
        self.last != Some((column, row))
    }

    /// task0005 (SC-11): the mutation [`would_report`] deliberately does
    /// not perform — commits `(column, row)` as the new cached cell.
    /// Applied exactly once, by [`apply_outcome`], when a decided outcome's
    /// updates say to.
    ///
    /// [`would_report`]: CellChangeFilter::would_report
    pub(super) fn commit(&mut self, column: u32, row: u32) {
        self.last = Some((column, row));
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

    /// task0005 (SC-10, D10): read-only lookup of the owner recorded for
    /// `button`'s press, WITHOUT clearing it. The motion sequence needs
    /// this on every motion event of an in-progress gesture — ending the
    /// gesture (as [`take`] does) belongs only to its matching release.
    ///
    /// [`take`]: GestureOwnership::take
    pub(super) fn peek(&self, button: MouseButtonId) -> Option<GestureOwner> {
        match button {
            MouseButtonId::Left => self.left,
            MouseButtonId::Middle => self.middle,
            MouseButtonId::Right => self.right,
            MouseButtonId::None => None,
        }
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

// ── task0005: SC-10 pointer decision sequence, SC-11 outcome
// application (D12) ─────────────────────────────────────────────────────
//
// Structure (D12): each pointer path's handler reduces to gather (plain
// values out of the host/app/core — no decision) / decide (SC-10, here) /
// apply (SC-11, here) / perform-local-arm (the handler, for whichever
// `LocalArm` the outcome names). Everything below is window-free: no
// winit type, no `term_core` type, no PTY, in any signature.

/// A plain identifier for a tab — deliberately not `&Tab` or any type that
/// borrows from `App`, which is what keeps every signature below
/// window-free (AC-1). Matches `App::active`'s representation (a tab
/// index).
pub(super) type TabId = usize;

/// task0005 (SC-10/SC-11, D12): the plain state value grouping the two
/// existing mouse-report records — [`CellChangeFilter`] (SC-4) and
/// [`GestureOwnership`] (SC-9) — together with the tab identifier they
/// were last built against, so a decision sequence can read "has the
/// active tab changed since these records were last valid" from the
/// records themselves. A plain value: a test constructs and inspects it
/// directly with no `WindowHost` (AC-2). `task0006` folds
/// `WindowHost`'s existing two fields into this shape when it reduces the
/// pointer handlers onto this seam (see `WindowHost::mouse_report_records`
/// / `set_mouse_report_records` in `mod.rs`).
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct MouseReportRecords {
    pub(super) cell_cache: CellChangeFilter,
    pub(super) gesture_owner: GestureOwnership,
    pub(super) built_for_tab: Option<TabId>,
}

/// task0005 (SC-11, D12): the "held-button record" alongside the
/// gesture-ownership record — which of left/middle/right is currently
/// held. Newly introduced (unlike SC-4/SC-9 this is not an "existing"
/// record) so [`clear_all`] can zero it together with the
/// gesture-ownership record on focus loss, matching the existing pointer
/// button-down counter's already-correct behaviour; `task0006` folds the
/// host's held-button bools into this type when it reduces the pointer
/// handlers onto this seam.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct HeldButtons {
    pub(super) left: bool,
    pub(super) middle: bool,
    pub(super) right: bool,
}

impl HeldButtons {
    /// Releases every held-button bit.
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }
}

/// SC-10 (AC-1): which local arm a "take a local arm" disposition names.
/// Disjoint from `Disposition::Report` and `Disposition::Nothing` — a test
/// can tell "nothing happened" apart from "the local arm ran" without a
/// window. The arm itself is performed by the caller (the handler,
/// `task0006`), never by [`apply_outcome`] — that is what keeps SC-11
/// window/GPU-surface/PTY-free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LocalArm {
    /// A left press (not a Ctrl+link-open) over the grid, either with no
    /// tracking mode active or with Shift held: begin a selection drag.
    BeginSelectionDrag,
    /// The release completing a locally-owned left-button drag: clear the
    /// drag flag, consume the pending anchor, publish the selection to
    /// PRIMARY.
    CompleteSelectionAndPublishToPrimary,
    /// Ctrl+left press over a hovered link: open it.
    OpenHoveredLink,
    /// Middle press with the paste-on-middle-click setting on: paste
    /// PRIMARY.
    PastePrimary,
    /// A wheel notch decided locally with tracking inactive (today's
    /// matrix) or with tracking active and Shift held (D5): scroll
    /// eMterm's own scrollback view.
    ScrollScrollback,
    /// A wheel notch on the alternate screen with the alternate-scroll
    /// mode bit and setting both on, tracking inactive (D5): translate to
    /// arrow-key bytes.
    TranslateToArrowBytes,
}

/// SC-10 (AC-1): one raw pointer event's disposition — named and disjoint.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Disposition {
    /// Emit a report: the exact bytes to write, and the identifier of the
    /// tab they are destined for (never necessarily whichever tab happens
    /// to be active when [`apply_outcome`] runs — a release carries the
    /// tab its press recorded).
    Report { bytes: Vec<u8>, tab: TabId },
    /// Take the named local arm. Performed by the caller, not here.
    Local(LocalArm),
    /// Nothing happened: no report, no local arm.
    Nothing,
}

/// SC-10: a gesture-ownership change one outcome implies, applied by
/// [`apply_outcome`] against the real owned record — never mutated at
/// decision time (SC-10 property 3: nothing mutates a record during the
/// decision).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GestureUpdate {
    /// Record a fresh owner for `button`'s press.
    Record(MouseButtonId, GestureOwner),
    /// Read-and-clear `button`'s recorded owner (its matching release was
    /// just delivered).
    Clear(MouseButtonId),
}

/// SC-10: the record updates one decided outcome implies. Carried in the
/// outcome rather than applied during decision (SC-10 property 3) so a
/// sequence unit can be a pure function of its plain inputs.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct RecordUpdates {
    /// D7/D12 correction #3: true when either reset observation fired (no
    /// tracking mode active, or the active tab differs from
    /// [`MouseReportRecords::built_for_tab`]) — [`apply_outcome`] applies
    /// this by resetting the cell-change cache and clearing every
    /// gesture-ownership slot, exactly as the two existing reset call
    /// sites already do.
    pub(super) reset: bool,
    /// Set [`MouseReportRecords::built_for_tab`] to this tab — every
    /// sequence that SC-8 accepts re-establishes which tab its records are
    /// now valid against.
    pub(super) built_for_tab: Option<TabId>,
    /// A motion the cell-change filter accepted: the new cell to commit.
    /// `None` when no motion cache update is implied.
    pub(super) cache_cell: Option<(u32, u32)>,
    /// A gesture-ownership change. `None` when this outcome implies no
    /// change to [`MouseReportRecords::gesture_owner`].
    pub(super) gesture: Option<GestureUpdate>,
}

/// SC-10: one raw pointer event's full decision — the disposition plus
/// the record updates it implies.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SequenceOutcome {
    pub(super) disposition: Disposition,
    pub(super) updates: RecordUpdates,
}

impl SequenceOutcome {
    fn nothing() -> Self {
        SequenceOutcome {
            disposition: Disposition::Nothing,
            updates: RecordUpdates::default(),
        }
    }
}

/// SC-10 input bundle for the button pointer path (press AND release —
/// one sequence unit covers both halves of a gesture).
#[derive(Debug, Clone, Copy)]
pub(super) struct ButtonEventInputs {
    pub(super) kind: MouseEventKind, // Press | Release
    pub(super) button: MouseButtonId,
    pub(super) grid: GridOwnershipInputs,
    pub(super) mods: Modifiers,
    pub(super) mode_1000: bool,
    pub(super) mode_1002: bool,
    pub(super) mode_1003: bool,
    pub(super) encoding: MouseReportEncoding,
    pub(super) active_tab: TabId,
    /// 1-based column/row of the event's cell.
    pub(super) column: u32,
    pub(super) row: u32,
    /// Press-only: is a link hovered at this position (for Ctrl+left).
    pub(super) hovered_link: bool,
    /// Press-only: the middle-click-paste setting.
    pub(super) middle_click_paste_enabled: bool,
    pub(super) records: MouseReportRecords,
}

/// SC-10 input bundle for the motion pointer path.
#[derive(Debug, Clone, Copy)]
pub(super) struct MotionEventInputs {
    pub(super) grid: GridOwnershipInputs,
    pub(super) mods: Modifiers,
    pub(super) mode_1000: bool,
    pub(super) mode_1002: bool,
    pub(super) mode_1003: bool,
    pub(super) encoding: MouseReportEncoding,
    pub(super) active_tab: TabId,
    pub(super) held_left: bool,
    pub(super) held_middle: bool,
    pub(super) held_right: bool,
    /// 1-based column/row of the event's cell.
    pub(super) column: u32,
    pub(super) row: u32,
    pub(super) records: MouseReportRecords,
}

/// SC-10 input bundle for the wheel pointer path.
#[derive(Debug, Clone, Copy)]
pub(super) struct WheelEventInputs {
    pub(super) kind: MouseEventKind, // WheelUp | WheelDown
    pub(super) grid: GridOwnershipInputs,
    pub(super) mods: Modifiers,
    pub(super) mode_1000: bool,
    pub(super) mode_1002: bool,
    pub(super) mode_1003: bool,
    pub(super) encoding: MouseReportEncoding,
    pub(super) active_tab: TabId,
    /// 1-based column/row of the event's cell.
    pub(super) column: u32,
    pub(super) row: u32,
    pub(super) on_alt_screen: bool,
    pub(super) alt_scroll_mode_bit: bool,
    pub(super) alt_scroll_setting: bool,
    pub(super) records: MouseReportRecords,
}

/// SC-10: press-time choice among the local arms a press (not routed to
/// report) can take. `grid` has already been checked by the caller.
fn press_local_disposition(
    button: MouseButtonId,
    mods: Modifiers,
    hovered_link: bool,
    middle_click_paste_enabled: bool,
) -> Disposition {
    match button {
        MouseButtonId::Left if mods.ctrl && hovered_link => {
            Disposition::Local(LocalArm::OpenHoveredLink)
        }
        MouseButtonId::Left => Disposition::Local(LocalArm::BeginSelectionDrag),
        MouseButtonId::Middle if middle_click_paste_enabled => {
            Disposition::Local(LocalArm::PastePrimary)
        }
        // Right press, or middle press with the setting off: SC-9
        // ownership is still recorded as Local by the caller (so the
        // matching release routes here, not into a stray report path),
        // but there is no local arm to name.
        MouseButtonId::Middle | MouseButtonId::Right | MouseButtonId::None => Disposition::Nothing,
    }
}

/// SC-10: the button pointer path — one sequence unit covering both a
/// press and its matching release (AC-1).
pub(super) fn decide_button_event(inputs: ButtonEventInputs) -> SequenceOutcome {
    match inputs.kind {
        MouseEventKind::Press => decide_press(&inputs),
        MouseEventKind::Release => decide_release(&inputs),
        MouseEventKind::Motion | MouseEventKind::WheelUp | MouseEventKind::WheelDown => {
            debug_assert!(
                false,
                "decide_button_event only handles Press/Release, got {:?}",
                inputs.kind
            );
            SequenceOutcome::nothing()
        }
    }
}

/// SC-10 (AC-1, AC-4): the press half. SC-8's answer is consulted first
/// (D11) — a rejected position records no owner (SC-9) and implies no
/// other update. An accepted position always carries the two reset
/// observations (D12 correction #3) alongside whichever disposition it
/// decides.
fn decide_press(inputs: &ButtonEventInputs) -> SequenceOutcome {
    if !point_belongs_to_grid(inputs.grid) {
        return SequenceOutcome::nothing();
    }
    let tracking_active = inputs.mode_1000 || inputs.mode_1002 || inputs.mode_1003;
    let tab_changed = inputs.records.built_for_tab != Some(inputs.active_tab);
    let reset = !tracking_active || tab_changed;

    if tracking_active && !inputs.mods.shift {
        let code = compose_button_code(
            MouseEventKind::Press,
            inputs.button,
            inputs.encoding,
            inputs.mods,
        );
        let disposition =
            match encode_report(code, inputs.column, inputs.row, inputs.encoding, false) {
                Some(bytes) => Disposition::Report {
                    bytes,
                    tab: inputs.active_tab,
                },
                None => Disposition::Nothing,
            };
        return SequenceOutcome {
            disposition,
            updates: RecordUpdates {
                reset,
                built_for_tab: Some(inputs.active_tab),
                gesture: Some(GestureUpdate::Record(inputs.button, GestureOwner::Report)),
                ..Default::default()
            },
        };
    }

    let disposition = press_local_disposition(
        inputs.button,
        inputs.mods,
        inputs.hovered_link,
        inputs.middle_click_paste_enabled,
    );
    SequenceOutcome {
        disposition,
        updates: RecordUpdates {
            reset,
            built_for_tab: Some(inputs.active_tab),
            gesture: Some(GestureUpdate::Record(inputs.button, GestureOwner::Local)),
            ..Default::default()
        },
    }
}

/// SC-10 (AC-1, AC-4, AC-6): the release half. Gesture ownership (SC-9)
/// decides here, NOT the release position (Test Notes: a press inside the
/// grid whose release arrives over a guarded region still gets its
/// release) — SC-8 is deliberately not consulted at all for a release.
/// Correction #1: a Report-owned release checks "at least one tracking
/// mode active", not merely which encoding is selected. Correction #2: a
/// reported release targets [`MouseReportRecords::built_for_tab`] (the tab
/// its press recorded), never `inputs.active_tab`.
fn decide_release(inputs: &ButtonEventInputs) -> SequenceOutcome {
    if let Some(owner) = inputs.records.gesture_owner.peek(inputs.button) {
        let disposition = match owner {
            GestureOwner::Report => {
                let tracking_active = inputs.mode_1000 || inputs.mode_1002 || inputs.mode_1003;
                if tracking_active {
                    let target_tab = inputs.records.built_for_tab.unwrap_or(inputs.active_tab);
                    let code = compose_button_code(
                        MouseEventKind::Release,
                        inputs.button,
                        inputs.encoding,
                        inputs.mods,
                    );
                    match encode_report(code, inputs.column, inputs.row, inputs.encoding, true) {
                        Some(bytes) => Disposition::Report {
                            bytes,
                            tab: target_tab,
                        },
                        None => Disposition::Nothing,
                    }
                } else {
                    Disposition::Nothing
                }
            }
            GestureOwner::Local => {
                if inputs.button == MouseButtonId::Left {
                    Disposition::Local(LocalArm::CompleteSelectionAndPublishToPrimary)
                } else {
                    // Middle/right local presses (paste, or no-op) are
                    // one-shot at press time; their release has no arm.
                    Disposition::Nothing
                }
            }
        };
        return SequenceOutcome {
            disposition,
            updates: RecordUpdates {
                gesture: Some(GestureUpdate::Clear(inputs.button)),
                ..Default::default()
            },
        };
    }
    // No owner recorded (SC-8 rejected the press, or no press preceded
    // this release at all): still carries the two reset observations
    // (D12 correction #3), so a click with no intervening motion is not
    // decided from stale records.
    let tracking_active = inputs.mode_1000 || inputs.mode_1002 || inputs.mode_1003;
    let tab_changed = inputs.records.built_for_tab != Some(inputs.active_tab);
    let reset = !tracking_active || tab_changed;
    SequenceOutcome {
        disposition: Disposition::Nothing,
        updates: RecordUpdates {
            reset,
            ..Default::default()
        },
    }
}

/// SC-10: lowest-numbered currently-held button (D6's tie-break, reused
/// here to pick which button's gesture ownership governs a motion event).
fn lowest_held_button(left: bool, middle: bool, right: bool) -> Option<MouseButtonId> {
    if left {
        Some(MouseButtonId::Left)
    } else if middle {
        Some(MouseButtonId::Middle)
    } else if right {
        Some(MouseButtonId::Right)
    } else {
        None
    }
}

/// SC-10: SC-5's gate, then SC-4's filter (non-mutating — [`apply_outcome`]
/// commits the cache), then SC-2/SC-3's encoding. Shared by the
/// owner-established (D10) and no-owner motion branches.
fn report_motion(inputs: &MotionEventInputs, cache: CellChangeFilter) -> SequenceOutcome {
    let Some(identity) = motion_gate(
        inputs.mode_1000,
        inputs.mode_1002,
        inputs.mode_1003,
        inputs.held_left,
        inputs.held_middle,
        inputs.held_right,
    ) else {
        return SequenceOutcome::nothing();
    };
    if !cache.would_report(inputs.column, inputs.row) {
        return SequenceOutcome::nothing();
    }
    let code = compose_button_code(
        MouseEventKind::Motion,
        identity,
        inputs.encoding,
        inputs.mods,
    );
    let disposition = match encode_report(code, inputs.column, inputs.row, inputs.encoding, false)
    {
        Some(bytes) => Disposition::Report {
            bytes,
            tab: inputs.active_tab,
        },
        None => Disposition::Nothing,
    };
    SequenceOutcome {
        disposition,
        updates: RecordUpdates {
            cache_cell: Some((inputs.column, inputs.row)),
            ..Default::default()
        },
    }
}

/// SC-10 (AC-1, AC-4, AC-5): the motion pointer path. SC-8's answer is
/// consulted first (D11), ahead of SC-5's gate and SC-4's filter — a
/// rejected position can neither emit nor advance the cached cell. When
/// the currently-held button (D6's lowest-numbered tie-break) already owns
/// a gesture, that recorded owner decides motion outright (D10) — Shift's
/// instantaneous state is not consulted at all, and neither is a reset,
/// so the drag's records survive intact through the gesture. With no
/// owning gesture, today's rules apply in full: Shift is a local override
/// (FR7) and the two reset observations (D12 correction #3) are carried.
pub(super) fn decide_motion_event(inputs: MotionEventInputs) -> SequenceOutcome {
    if !point_belongs_to_grid(inputs.grid) {
        return SequenceOutcome::nothing();
    }

    let held_button = lowest_held_button(inputs.held_left, inputs.held_middle, inputs.held_right);
    let owner = held_button.and_then(|b| inputs.records.gesture_owner.peek(b));

    match owner {
        Some(GestureOwner::Local) => SequenceOutcome::nothing(),
        Some(GestureOwner::Report) => report_motion(&inputs, inputs.records.cell_cache),
        None => {
            let tracking_active = inputs.mode_1000 || inputs.mode_1002 || inputs.mode_1003;
            let tab_changed = inputs.records.built_for_tab != Some(inputs.active_tab);
            let reset = !tracking_active || tab_changed;
            if !tracking_active || inputs.mods.shift {
                return SequenceOutcome {
                    disposition: Disposition::Nothing,
                    updates: RecordUpdates {
                        reset,
                        built_for_tab: Some(inputs.active_tab),
                        ..Default::default()
                    },
                };
            }
            let effective_cache = if reset {
                CellChangeFilter::default()
            } else {
                inputs.records.cell_cache
            };
            let mut outcome = report_motion(&inputs, effective_cache);
            outcome.updates.reset = reset;
            outcome.updates.built_for_tab = Some(inputs.active_tab);
            outcome
        }
    }
}

/// SC-10 (AC-1, AC-3, AC-6): the wheel pointer path. SC-8's answer is
/// consulted first (D11), ahead of the tracking-active read — a rejected
/// position emits nothing and updates no record. An accepted notch always
/// carries the two reset observations (D12 correction #3) and chooses
/// exactly one wheel consumer via SC-6 (`wheel_consumer`), unchanged.
pub(super) fn decide_wheel_event(inputs: &WheelEventInputs) -> SequenceOutcome {
    if !point_belongs_to_grid(inputs.grid) {
        return SequenceOutcome::nothing();
    }
    let tracking_active = inputs.mode_1000 || inputs.mode_1002 || inputs.mode_1003;
    let tab_changed = inputs.records.built_for_tab != Some(inputs.active_tab);
    let reset = !tracking_active || tab_changed;

    let consumer = wheel_consumer(
        tracking_active,
        inputs.mods.shift,
        inputs.on_alt_screen,
        inputs.alt_scroll_mode_bit,
        inputs.alt_scroll_setting,
    );
    let disposition = match consumer {
        WheelConsumer::ReportToApplication => {
            let code =
                compose_button_code(inputs.kind, MouseButtonId::None, inputs.encoding, inputs.mods);
            match encode_report(code, inputs.column, inputs.row, inputs.encoding, false) {
                Some(bytes) => Disposition::Report {
                    bytes,
                    tab: inputs.active_tab,
                },
                None => Disposition::Nothing,
            }
        }
        WheelConsumer::TranslateToArrows => Disposition::Local(LocalArm::TranslateToArrowBytes),
        WheelConsumer::ScrollScrollback => Disposition::Local(LocalArm::ScrollScrollback),
    };
    SequenceOutcome {
        disposition,
        updates: RecordUpdates {
            reset,
            built_for_tab: Some(inputs.active_tab),
            ..Default::default()
        },
    }
}

/// SC-11 (AC-2): applies an outcome to the record pair as a plain value —
/// appends report bytes to `dest` paired with their target tab, applies
/// the outcome's record updates exactly once, appends nothing for a
/// do-nothing or local disposition, and never performs the named local
/// arm itself (that stays the caller's job — see [`LocalArm`]).
pub(super) fn apply_outcome(
    outcome: SequenceOutcome,
    records: &mut MouseReportRecords,
    dest: &mut Vec<(TabId, Vec<u8>)>,
) {
    if outcome.updates.reset {
        records.cell_cache.reset();
        records.gesture_owner.clear_all();
    }
    if let Some(tab) = outcome.updates.built_for_tab {
        records.built_for_tab = Some(tab);
    }
    if let Some((column, row)) = outcome.updates.cache_cell {
        records.cell_cache.commit(column, row);
    }
    match outcome.updates.gesture {
        Some(GestureUpdate::Record(button, owner)) => {
            records.gesture_owner.record_press(button, owner);
        }
        Some(GestureUpdate::Clear(button)) => {
            records.gesture_owner.take(button);
        }
        None => {}
    }
    if let Disposition::Report { bytes, tab } = outcome.disposition {
        dest.push((tab, bytes));
    }
}

/// SC-11 (AC-2, D12): empties the gesture-ownership record and the
/// held-button record together, for the host to invoke on focus loss —
/// matching the already-correct reset of the existing pointer button-down
/// counter. Deliberately does not touch the cell-change cache, which has
/// its own reset points (D7) unrelated to focus.
pub(super) fn clear_all(gesture_owner: &mut GestureOwnership, held: &mut HeldButtons) {
    gesture_owner.clear_all();
    held.clear();
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

    // ── task0005: SC-10/SC-11 test helpers ──────────────────────────────
    //
    // Every sequence unit and the applier is constructed and called here
    // with no winit window, no GPU surface and no live PTY (AC-1, AC-2) —
    // these helpers just cut down on field repetition across cases.

    /// A button-path input with tracking (1002/sgr) active, over the open
    /// grid, no modifiers — the neutral case each test customizes from.
    fn base_button_inputs(kind: MouseEventKind, button: MouseButtonId) -> ButtonEventInputs {
        ButtonEventInputs {
            kind,
            button,
            grid: GridOwnershipInputs::default(),
            mods: Modifiers::NONE,
            mode_1000: false,
            mode_1002: true,
            mode_1003: false,
            encoding: MouseReportEncoding::Sgr,
            active_tab: 0,
            column: 5,
            row: 5,
            hovered_link: false,
            middle_click_paste_enabled: true,
            records: MouseReportRecords::default(),
        }
    }

    /// A motion-path input with tracking (1002/sgr) active, the left
    /// button held, over the open grid, no modifiers.
    fn base_motion_inputs() -> MotionEventInputs {
        MotionEventInputs {
            grid: GridOwnershipInputs::default(),
            mods: Modifiers::NONE,
            mode_1000: false,
            mode_1002: true,
            mode_1003: false,
            encoding: MouseReportEncoding::Sgr,
            active_tab: 0,
            held_left: true,
            held_middle: false,
            held_right: false,
            column: 5,
            row: 5,
            records: MouseReportRecords::default(),
        }
    }

    /// A wheel-path input with tracking (1002/sgr) active, over the open
    /// grid, no modifiers.
    fn base_wheel_inputs(kind: MouseEventKind) -> WheelEventInputs {
        WheelEventInputs {
            kind,
            grid: GridOwnershipInputs::default(),
            mods: Modifiers::NONE,
            mode_1000: false,
            mode_1002: true,
            mode_1003: false,
            encoding: MouseReportEncoding::Sgr,
            active_tab: 0,
            column: 5,
            row: 5,
            on_alt_screen: false,
            alt_scroll_mode_bit: false,
            alt_scroll_setting: false,
            records: MouseReportRecords::default(),
        }
    }

    // ── AC-1: named, disjoint dispositions ──────────────────────────────

    #[test]
    fn ac1_report_disposition_carries_bytes_and_target_tab() {
        let mut inputs = base_button_inputs(MouseEventKind::Press, MouseButtonId::Left);
        inputs.active_tab = 42;
        let outcome = decide_button_event(inputs);
        match outcome.disposition {
            Disposition::Report { bytes, tab } => {
                assert_eq!(tab, 42);
                assert!(!bytes.is_empty());
            }
            other => panic!("expected Report, got {other:?}"),
        }
    }

    #[test]
    fn ac1_local_and_nothing_dispositions_are_distinct_named_values() {
        // No tracking mode active: a left press over the grid is a named
        // local arm, not bare "nothing".
        let mut inputs = base_button_inputs(MouseEventKind::Press, MouseButtonId::Left);
        inputs.mode_1002 = false;
        let outcome = decide_button_event(inputs);
        assert_eq!(
            outcome.disposition,
            Disposition::Local(LocalArm::BeginSelectionDrag)
        );
        assert_ne!(outcome.disposition, Disposition::Nothing);

        // A rejected position is bare "nothing" — no local arm.
        let mut inputs = base_button_inputs(MouseEventKind::Press, MouseButtonId::Left);
        inputs.grid = GridOwnershipInputs {
            in_top_strip: true,
            ..GridOwnershipInputs::default()
        };
        let outcome = decide_button_event(inputs);
        assert_eq!(outcome.disposition, Disposition::Nothing);
    }

    #[test]
    fn ac1_units_mutate_nothing_themselves_updates_travel_in_the_outcome() {
        // Calling decide_* on a plain records value never changes it —
        // only `apply_outcome` (SC-11) does.
        let records = MouseReportRecords::default();
        let inputs = ButtonEventInputs {
            records,
            ..base_button_inputs(MouseEventKind::Press, MouseButtonId::Left)
        };
        let _ = decide_button_event(inputs);
        assert_eq!(records.built_for_tab, None);
        assert_eq!(records.gesture_owner.peek(MouseButtonId::Left), None);
    }

    // ── AC-2: the applier ────────────────────────────────────────────────

    #[test]
    fn ac2_applier_appends_report_bytes_with_tab_and_applies_updates_once() {
        let mut records = MouseReportRecords::default();
        let outcome = SequenceOutcome {
            disposition: Disposition::Report {
                bytes: vec![1, 2, 3],
                tab: 7,
            },
            updates: RecordUpdates {
                reset: false,
                built_for_tab: Some(7),
                cache_cell: Some((4, 5)),
                gesture: Some(GestureUpdate::Record(
                    MouseButtonId::Left,
                    GestureOwner::Report,
                )),
            },
        };
        let mut dest = Vec::new();
        apply_outcome(outcome, &mut records, &mut dest);
        assert_eq!(dest, vec![(7, vec![1, 2, 3])]);
        assert!(!records.cell_cache.would_report(4, 5));
        assert_eq!(
            records.gesture_owner.peek(MouseButtonId::Left),
            Some(GestureOwner::Report)
        );
        assert_eq!(records.built_for_tab, Some(7));
    }

    #[test]
    fn ac2_applier_appends_nothing_for_nothing_or_local_dispositions() {
        let mut records = MouseReportRecords::default();
        let mut dest = Vec::new();
        apply_outcome(SequenceOutcome::nothing(), &mut records, &mut dest);
        apply_outcome(
            SequenceOutcome {
                disposition: Disposition::Local(LocalArm::ScrollScrollback),
                updates: RecordUpdates::default(),
            },
            &mut records,
            &mut dest,
        );
        assert!(dest.is_empty());
    }

    #[test]
    fn ac2_clear_all_empties_gesture_and_held_button_records() {
        let mut gesture = GestureOwnership::new();
        gesture.record_press(MouseButtonId::Left, GestureOwner::Report);
        gesture.record_press(MouseButtonId::Right, GestureOwner::Local);
        let mut held = HeldButtons {
            left: true,
            middle: true,
            right: false,
        };
        clear_all(&mut gesture, &mut held);
        assert_eq!(gesture.peek(MouseButtonId::Left), None);
        assert_eq!(gesture.peek(MouseButtonId::Right), None);
        assert_eq!(held, HeldButtons::default());
    }

    // ── AC-3 (TS-19): chrome-guarded regions × event kind × button ──────

    #[test]
    fn ac3_ts19_guarded_regions_reject_every_event_kind_and_button() {
        let regions: [(&str, GridOwnershipInputs); 6] = [
            (
                "top strip",
                GridOwnershipInputs {
                    in_top_strip: true,
                    ..GridOwnershipInputs::default()
                },
            ),
            (
                "status-bar bottom strip",
                GridOwnershipInputs {
                    in_bottom_strip: true,
                    ..GridOwnershipInputs::default()
                },
            ),
            (
                "right-edge scrollbar overlay",
                GridOwnershipInputs {
                    in_scrollbar_overlay: true,
                    ..GridOwnershipInputs::default()
                },
            ),
            (
                "mux sidebar (persistent or overlay collapse to one bool)",
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

        for (name, grid) in regions {
            for button in [
                MouseButtonId::Left,
                MouseButtonId::Middle,
                MouseButtonId::Right,
            ] {
                for kind in [MouseEventKind::Press, MouseEventKind::Release] {
                    let mut records = MouseReportRecords::default();
                    let inputs = ButtonEventInputs {
                        grid,
                        ..base_button_inputs(kind, button)
                    };
                    let outcome = decide_button_event(inputs);
                    let mut dest = Vec::new();
                    apply_outcome(outcome, &mut records, &mut dest);
                    assert!(
                        dest.is_empty(),
                        "{name}: {kind:?} {button:?} must not report"
                    );
                }
            }

            let mut records = MouseReportRecords::default();
            let motion_inputs = MotionEventInputs {
                grid,
                ..base_motion_inputs()
            };
            let outcome = decide_motion_event(motion_inputs);
            let mut dest = Vec::new();
            apply_outcome(outcome, &mut records, &mut dest);
            assert!(dest.is_empty(), "{name}: motion must not report");

            for kind in [MouseEventKind::WheelUp, MouseEventKind::WheelDown] {
                let mut records = MouseReportRecords::default();
                let wheel_inputs = WheelEventInputs {
                    grid,
                    ..base_wheel_inputs(kind)
                };
                let outcome = decide_wheel_event(&wheel_inputs);
                let mut dest = Vec::new();
                apply_outcome(outcome, &mut records, &mut dest);
                assert!(dest.is_empty(), "{name}: {kind:?} must not report");
            }
        }
    }

    #[test]
    fn ac3_ts19_a_position_over_the_grid_with_no_region_claiming_it_produces_bytes() {
        // Press.
        let mut records = MouseReportRecords::default();
        let outcome = decide_button_event(base_button_inputs(
            MouseEventKind::Press,
            MouseButtonId::Left,
        ));
        let mut dest = Vec::new();
        apply_outcome(outcome, &mut records, &mut dest);
        assert!(!dest.is_empty(), "press over the open grid must report");

        // Its matching release (gesture-owned, so position is irrelevant).
        dest.clear();
        let outcome = decide_button_event(ButtonEventInputs {
            records,
            ..base_button_inputs(MouseEventKind::Release, MouseButtonId::Left)
        });
        apply_outcome(outcome, &mut records, &mut dest);
        assert!(!dest.is_empty(), "matching release must report");

        // Motion (different cell so the cache does not suppress it).
        dest.clear();
        let mut records = MouseReportRecords::default();
        let outcome = decide_motion_event(MotionEventInputs {
            column: 9,
            row: 9,
            ..base_motion_inputs()
        });
        apply_outcome(outcome, &mut records, &mut dest);
        assert!(!dest.is_empty(), "motion over the open grid must report");

        // Wheel.
        dest.clear();
        let mut records = MouseReportRecords::default();
        let outcome = decide_wheel_event(&base_wheel_inputs(MouseEventKind::WheelUp));
        apply_outcome(outcome, &mut records, &mut dest);
        assert!(!dest.is_empty(), "wheel notch over the open grid must report");
    }

    // ── AC-4 (TS-20, TS-24): sequence-driven, one persistent record pair ─

    #[test]
    fn ac4_ts20_shift_press_then_lift_then_release_completes_selection_locally() {
        let mut records = MouseReportRecords::default();
        let mut dest = Vec::new();

        let press = ButtonEventInputs {
            mods: Modifiers {
                shift: true,
                ..Modifiers::NONE
            },
            records,
            ..base_button_inputs(MouseEventKind::Press, MouseButtonId::Left)
        };
        let outcome = decide_button_event(press);
        assert_eq!(
            outcome.disposition,
            Disposition::Local(LocalArm::BeginSelectionDrag)
        );
        apply_outcome(outcome, &mut records, &mut dest);
        assert!(dest.is_empty());

        // Shift lifted mid-drag: motion must stay local regardless.
        let motion = MotionEventInputs {
            mods: Modifiers::NONE,
            column: 6,
            row: 6,
            records,
            ..base_motion_inputs()
        };
        let outcome = decide_motion_event(motion);
        assert_eq!(outcome.disposition, Disposition::Nothing);
        apply_outcome(outcome, &mut records, &mut dest);
        assert!(dest.is_empty());

        // Release with Shift now lifted: still routed by the recorded
        // (Local) owner, not the instantaneous Shift flag.
        let release = ButtonEventInputs {
            mods: Modifiers::NONE,
            records,
            ..base_button_inputs(MouseEventKind::Release, MouseButtonId::Left)
        };
        let outcome = decide_button_event(release);
        assert_eq!(
            outcome.disposition,
            Disposition::Local(LocalArm::CompleteSelectionAndPublishToPrimary)
        );
        apply_outcome(outcome, &mut records, &mut dest);
        assert!(dest.is_empty(), "a completed local selection emits no bytes");
        assert_eq!(
            records.gesture_owner.peek(MouseButtonId::Left),
            None,
            "a delivered release clears its record"
        );
    }

    #[test]
    fn ac4_ts20_no_shift_press_then_shift_arrives_release_still_reports_without_shift_bit() {
        let mut records = MouseReportRecords::default();
        let mut dest = Vec::new();

        let press = ButtonEventInputs {
            mods: Modifiers::NONE,
            records,
            ..base_button_inputs(MouseEventKind::Press, MouseButtonId::Left)
        };
        let outcome = decide_button_event(press);
        assert!(matches!(outcome.disposition, Disposition::Report { .. }));
        apply_outcome(outcome, &mut records, &mut dest);
        assert_eq!(dest.len(), 1);

        // Motion with Shift now held: the recorded (Report) owner keeps
        // reporting motion regardless of the instantaneous Shift flag.
        dest.clear();
        let motion = MotionEventInputs {
            mods: Modifiers {
                shift: true,
                ..Modifiers::NONE
            },
            column: 6,
            row: 6,
            records,
            ..base_motion_inputs()
        };
        let outcome = decide_motion_event(motion);
        assert!(
            matches!(outcome.disposition, Disposition::Report { .. }),
            "an owned Report gesture keeps reporting motion despite Shift"
        );
        apply_outcome(outcome, &mut records, &mut dest);

        // Release, Shift held: still reports, and the button code never
        // carries the shift bit (AC-10 / D4).
        dest.clear();
        let release = ButtonEventInputs {
            mods: Modifiers {
                shift: true,
                ..Modifiers::NONE
            },
            records,
            ..base_button_inputs(MouseEventKind::Release, MouseButtonId::Left)
        };
        let outcome = decide_button_event(release);
        match outcome.disposition {
            Disposition::Report { bytes, tab } => {
                assert_eq!(tab, 0);
                assert_eq!(bytes, b"\x1b[<0;5;5m".to_vec());
            }
            other => panic!("expected a release report, got {other:?}"),
        }
    }

    #[test]
    fn ac4_press_rejected_by_grid_ownership_records_no_owner() {
        let mut records = MouseReportRecords::default();
        let press = ButtonEventInputs {
            grid: GridOwnershipInputs {
                in_top_strip: true,
                ..GridOwnershipInputs::default()
            },
            ..base_button_inputs(MouseEventKind::Press, MouseButtonId::Left)
        };
        let outcome = decide_button_event(press);
        let mut dest = Vec::new();
        apply_outcome(outcome, &mut records, &mut dest);
        assert_eq!(records.gesture_owner.peek(MouseButtonId::Left), None);
    }

    #[test]
    fn ac4_two_buttons_pressed_mid_gesture_are_owned_independently() {
        let mut records = MouseReportRecords::default();
        let mut dest = Vec::new();

        let left_press = ButtonEventInputs {
            records,
            ..base_button_inputs(MouseEventKind::Press, MouseButtonId::Left)
        };
        apply_outcome(decide_button_event(left_press), &mut records, &mut dest);

        let middle_press = ButtonEventInputs {
            mods: Modifiers {
                shift: true,
                ..Modifiers::NONE
            },
            records,
            ..base_button_inputs(MouseEventKind::Press, MouseButtonId::Middle)
        };
        apply_outcome(decide_button_event(middle_press), &mut records, &mut dest);

        assert_eq!(
            records.gesture_owner.peek(MouseButtonId::Left),
            Some(GestureOwner::Report)
        );
        assert_eq!(
            records.gesture_owner.peek(MouseButtonId::Middle),
            Some(GestureOwner::Local)
        );
    }

    // ── AC-5 (TS-21): rejected motion leaves the cached cell untouched ──

    #[test]
    fn ac5_ts21_rejected_motion_leaves_cached_cell_unchanged_then_next_motion_still_reports() {
        let mut records = MouseReportRecords::default();
        records.cell_cache.commit(1, 1);
        let mut dest = Vec::new();

        let rejected = MotionEventInputs {
            grid: GridOwnershipInputs {
                in_bottom_strip: true,
                ..GridOwnershipInputs::default()
            },
            column: 9,
            row: 9,
            records,
            ..base_motion_inputs()
        };
        let outcome = decide_motion_event(rejected);
        assert_eq!(outcome.disposition, Disposition::Nothing);
        apply_outcome(outcome, &mut records, &mut dest);
        assert!(dest.is_empty());
        assert!(
            !records.cell_cache.would_report(1, 1),
            "the cached cell must be exactly what it was before the rejected motion"
        );

        let accepted = MotionEventInputs {
            column: 2,
            row: 2,
            records,
            ..base_motion_inputs()
        };
        let outcome = decide_motion_event(accepted);
        apply_outcome(outcome, &mut records, &mut dest);
        assert!(
            !dest.is_empty(),
            "a following motion over the grid at a different cell must still report"
        );
    }

    // ── AC-6 (TS-22, TS-23): release tracking/tab-identity corrections ──

    #[test]
    fn ac6_ts22_release_with_tracking_cleared_mid_gesture_produces_no_bytes() {
        let mut records = MouseReportRecords::default();
        let mut dest = Vec::new();
        apply_outcome(
            decide_button_event(ButtonEventInputs {
                records,
                ..base_button_inputs(MouseEventKind::Press, MouseButtonId::Left)
            }),
            &mut records,
            &mut dest,
        );
        assert!(!dest.is_empty(), "sanity: the press reported");
        dest.clear();

        let release = ButtonEventInputs {
            mode_1000: false,
            mode_1002: false,
            mode_1003: false,
            records,
            ..base_button_inputs(MouseEventKind::Release, MouseButtonId::Left)
        };
        let outcome = decide_button_event(release);
        assert_eq!(outcome.disposition, Disposition::Nothing);
        apply_outcome(outcome, &mut records, &mut dest);
        assert!(dest.is_empty());
    }

    #[test]
    fn ac6_ts23_release_after_active_tab_change_targets_the_tab_the_press_recorded() {
        let mut records = MouseReportRecords::default();
        let mut dest = Vec::new();
        apply_outcome(
            decide_button_event(ButtonEventInputs {
                active_tab: 1,
                records,
                ..base_button_inputs(MouseEventKind::Press, MouseButtonId::Right)
            }),
            &mut records,
            &mut dest,
        );
        dest.clear();

        let release = ButtonEventInputs {
            active_tab: 2,
            records,
            ..base_button_inputs(MouseEventKind::Release, MouseButtonId::Right)
        };
        let outcome = decide_button_event(release);
        match &outcome.disposition {
            Disposition::Report { tab, .. } => assert_eq!(*tab, 1),
            other => panic!("expected a release report targeting tab 1, got {other:?}"),
        }
        apply_outcome(outcome, &mut records, &mut dest);
        assert_eq!(dest, vec![(1, dest[0].1.clone())]);
    }

    #[test]
    fn ac6_release_with_no_recorded_press_produces_no_bytes() {
        let mut records = MouseReportRecords::default();
        let outcome = decide_button_event(base_button_inputs(
            MouseEventKind::Release,
            MouseButtonId::Left,
        ));
        assert_eq!(outcome.disposition, Disposition::Nothing);
        let mut dest = Vec::new();
        apply_outcome(outcome, &mut records, &mut dest);
        assert!(dest.is_empty());
    }

    #[test]
    fn ac6_button_and_wheel_paths_both_carry_the_two_reset_observations() {
        // Button path: no tracking mode active -> reset, even for a click
        // with no intervening motion.
        let mut records = MouseReportRecords::default();
        records.cell_cache.commit(9, 9);
        records
            .gesture_owner
            .record_press(MouseButtonId::Left, GestureOwner::Report);
        records.built_for_tab = Some(5);
        let mut dest = Vec::new();
        let press = ButtonEventInputs {
            mode_1000: false,
            mode_1002: false,
            mode_1003: false,
            active_tab: 5,
            records,
            ..base_button_inputs(MouseEventKind::Press, MouseButtonId::Right)
        };
        apply_outcome(decide_button_event(press), &mut records, &mut dest);
        assert!(
            records.cell_cache.would_report(9, 9),
            "button path resets the stale cache when no tracking mode is active"
        );
        assert_eq!(
            records.gesture_owner.peek(MouseButtonId::Left),
            None,
            "button path clears stale ownership when no tracking mode is active"
        );

        // Wheel path: active tab differs from the one the records were
        // built against -> reset, even with no intervening motion.
        let mut records = MouseReportRecords::default();
        records.cell_cache.commit(3, 3);
        records
            .gesture_owner
            .record_press(MouseButtonId::Middle, GestureOwner::Local);
        records.built_for_tab = Some(1);
        let wheel = WheelEventInputs {
            active_tab: 2,
            records,
            ..base_wheel_inputs(MouseEventKind::WheelUp)
        };
        apply_outcome(decide_wheel_event(&wheel), &mut records, &mut dest);
        assert!(
            records.cell_cache.would_report(3, 3),
            "wheel path resets the stale cache on an active-tab change"
        );
        assert_eq!(
            records.gesture_owner.peek(MouseButtonId::Middle),
            None,
            "wheel path clears stale ownership on an active-tab change"
        );
    }
}
