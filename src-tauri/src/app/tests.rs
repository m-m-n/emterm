use super::*;
use crate::prompts::{PromptMarkKind, ResolvedPromptMark};
use crate::selection::{Pos, SelectionMode};
use std::time::Duration;
use term_core::terminal_core::TerminalCore;

mod agent_status;
mod chooser;
mod font_settings;
mod ime;
mod misc;
mod mux_ui;
mod scroll_search_fold;
mod sftp;
mod tab_lifecycle;
mod timing;

fn fresh_core(cols: u16, rows: u16) -> TerminalCore {
    TerminalCore::new(cols, rows, 100)
}

fn app_with_cleared_state(core: &mut TerminalCore) -> App {
    let mut app = App::new();
    // Initial frame uses a full redraw; clear it so subsequent calls
    // exercise the union logic rather than the bypass.
    app.record_render_state(core);
    app
}

/// Build an `App` whose single tab spawned NO real shell process
/// ([`crate::tabs::Tab::test_shell_less`]): its PTY is absent and its
/// event channel starts disconnected, so `pump_all` sees only the
/// state the test itself injects. The `pump_all_*` eviction/anchor
/// tests need this determinism — with a real shell, startup output
/// draining mid-`pump_all` under host load perturbed the eviction
/// counters they assert on (their historical parallel-suite flakiness).
fn app_with_shell_less_tab() -> App {
    let mut app = App::new();
    let dims = app.cell_size;
    let tab = crate::tabs::Tab::test_shell_less(
        "shell",
        dims.cols,
        dims.rows,
        app.settings.scrollback_lines,
        app.settings.clone(),
        app.notification_sink.clone(),
    );
    app.tabs.push(tab);
    app.active = 0;
    app
}

/// Build an `App` with one initial tab whose core has `scrollback`
/// rows pushed into scrollback (so absolute rows 0..scrollback are
/// scrollback and scrollback.. is viewport), and the given prompt-start
/// marks installed. The grid is tiny (4 rows) so a handful of `\r\n`
/// lines spill into scrollback quickly.
fn app_with_prompts(scrollback: u32, prompt_rows: &[u32]) -> App {
    let mut app = App::new();
    app.spawn_initial_tab();
    {
        let tab = &mut app.tabs[0];
        // Push `scrollback + rows` newlines so `scrollback` rows land in
        // scrollback. Tab core is 80x24-ish by default; feeding plenty
        // of newlines guarantees the requested scrollback depth.
        let mut bytes = Vec::new();
        let total = scrollback + 64; // overshoot to fill the viewport too
        for _ in 0..total {
            bytes.extend_from_slice(b"\r\n");
        }
        tab.core.lock().process_pty_data(&bytes);
        for &row in prompt_rows {
            tab.prompts.push(ResolvedPromptMark {
                kind: PromptMarkKind::PromptStart,
                row,
                exit_code: None,
            });
        }
    }
    app
}

/// Seed one tab with a selection, a pending anchor, an OSC 133 prompt
/// mark, and a fold region, after normalizing the grid to a known width.
fn app_with_seeded_trackers() -> App {
    let mut app = App::new();
    app.spawn_initial_tab();
    // Normalize to a known width first; the very first set_grid_size may
    // itself be a width change from the default, which would clear the
    // (still empty) trackers — harmless, but we seed afterward.
    app.set_grid_size(80, 24);
    app.selection = Some(Selection {
        anchor: Pos { row: 1, col: 0 },
        extent: Pos { row: 3, col: 4 },
        mode: SelectionMode::Character,
        origin: Pos { row: 1, col: 0 },
    });
    app.pending_selection_anchor = Some(Pos { row: 2, col: 1 });
    app.tabs[0]
        .prompts
        .push(crate::prompts::ResolvedPromptMark {
            kind: crate::prompts::PromptMarkKind::PromptStart,
            row: 5,
            exit_code: None,
        });
    app.tabs[0]
        .folds
        .register_osc133_region(5, 8, "cmd".to_string(), None);
    app
}

// ── selection-clear-on-enter-copy task0001: clear_selection helper ────

#[test]
fn clear_selection_unsets_both_when_both_are_set() {
    // AC-3: called with both fields set, `clear_selection` leaves both
    // unset.
    let mut app = app_with_seeded_trackers();
    assert!(app.selection.is_some(), "test setup: selection must start set");
    assert!(
        app.pending_selection_anchor.is_some(),
        "test setup: pending anchor must start set"
    );
    app.clear_selection();
    assert!(
        app.selection.is_none(),
        "clear_selection must unset selection (AC-3)"
    );
    assert!(
        app.pending_selection_anchor.is_none(),
        "clear_selection must unset pending_selection_anchor (AC-3)"
    );
}

#[test]
fn clear_selection_is_a_noop_when_nothing_is_selected() {
    // AC-3: called with no selection present, state is left unchanged —
    // and since `clear_selection` takes no host/clipboard handle, it
    // structurally cannot write to the clipboard or PRIMARY.
    let mut app = App::new();
    assert!(app.selection.is_none(), "test setup: no selection");
    assert!(
        app.pending_selection_anchor.is_none(),
        "test setup: no pending anchor"
    );
    let needs_full_redraw_before = app.needs_full_redraw;
    app.clear_selection();
    assert!(app.selection.is_none(), "still unset (AC-3)");
    assert!(
        app.pending_selection_anchor.is_none(),
        "still unset (AC-3)"
    );
    assert_eq!(
        app.needs_full_redraw, needs_full_redraw_before,
        "clear_selection must not request a redraw of its own (NFR5): the \
         existing dirty-row union already repaints any affected rows"
    );
}

#[test]
fn clear_selection_dirties_the_rows_the_old_highlight_occupied() {
    // AC-6: the frame after clear_selection flips the selection from set
    // to unset must report exactly the screen rows the old highlight
    // occupied as dirty (dirty_rows_this_frame's previous ∪ current
    // selection union), with no full-redraw flag introduced by this
    // change (FR6, NFR5).
    let mut app = app_with_prompts(50, &[]);
    let core_arc = app.tabs[0].core.clone();
    // Clear the initial full-redraw latch before seeding the selection,
    // so the union path runs rather than the 0..rows bypass.
    {
        let mut core = core_arc.lock();
        app.record_render_state(&mut core);
    }
    let visible_start = core_arc.lock().get_scrollback_length();
    app.selection = Some(Selection {
        anchor: Pos {
            row: visible_start + 3,
            col: 0,
        },
        extent: Pos {
            row: visible_start + 4,
            col: 5,
        },
        mode: SelectionMode::Character,
        origin: Pos {
            row: visible_start + 3,
            col: 0,
        },
    });
    // Render this frame so the highlight becomes `previous_selection`.
    {
        let mut core = core_arc.lock();
        app.record_render_state(&mut core);
    }
    app.clear_selection();
    let set = {
        let core = core_arc.lock();
        app.dirty_rows_this_frame(&core)
    };
    assert!(
        set.contains(&3) && set.contains(&4),
        "the old selection's screen rows (3, 4) must be dirty the frame \
         after clear_selection (AC-6): got {set:?}"
    );
    assert!(
        !set.contains(&0),
        "an unrelated screen row must stay clean — a dirty set covering row \
         0 too would indicate a full-redraw fallback rather than the \
         targeted selection-row union (AC-6, NFR5): got {set:?}"
    );
    assert!(
        !app.needs_full_redraw,
        "clear_selection must not set needs_full_redraw itself (NFR5)"
    );
}
