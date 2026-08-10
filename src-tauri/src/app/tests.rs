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
