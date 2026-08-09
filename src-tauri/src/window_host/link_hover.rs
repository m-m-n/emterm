//! Link hover / click handling: multi-click classification, the hover
//! state cache, OSC 8 + heuristic link detection under the pointer, and
//! opening links in the browser or editor.

use std::process::Stdio;
use std::time::Instant;

use winit::cursor::CursorIcon;

use crate::app::App;
use crate::selection::SelectionMode;

use super::WindowHost;

/// Maximum time between successive clicks that still counts as a "multi-click".
/// Within this window the click counter increments; beyond it the counter
/// resets to 1. 500 ms matches xterm's `multiClickTime` default.
pub(super) const MULTI_CLICK_WINDOW_MS: u128 = 500;

/// Tracks last-click metadata so a double / triple click can be detected by
/// comparing time + position against the next press.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct ClickTracker {
    last_press_at: Option<Instant>,
    /// Last press cell as `(abs_row, col)`. Using the absolute row keeps a
    /// double / triple click from spuriously matching when the viewport
    /// scrolled a different buffer line under the same screen cell between
    /// clicks.
    last_press_pos: Option<(u32, u16)>,
    /// Click counter: 1 → Character, 2 → Word, 3 → Line. After 3, the next
    /// press resets to 1.
    count: u32,
}

/// Output of the click classifier: the click count and the matching
/// selection mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ClickClassification {
    pub(super) count: u32,
    pub(super) mode: SelectionMode,
}

impl ClickTracker {
    /// Classify a new press at absolute `(row, col)` happening at `now`. The
    /// internal state is updated for the next call.
    pub(super) fn classify(&mut self, now: Instant, row: u32, col: u16) -> ClickClassification {
        let mut count = 1u32;
        if let (Some(prev_at), Some(prev_pos)) = (self.last_press_at, self.last_press_pos) {
            let elapsed_ms = now.duration_since(prev_at).as_millis();
            if elapsed_ms <= MULTI_CLICK_WINDOW_MS && prev_pos == (row, col) && self.count < 3 {
                count = self.count + 1;
            }
        }
        self.last_press_at = Some(now);
        self.last_press_pos = Some((row, col));
        self.count = count;
        ClickClassification {
            count,
            mode: match count {
                1 => SelectionMode::Character,
                2 => SelectionMode::Word,
                _ => SelectionMode::Line, // 3 or more
            },
        }
    }
}

/// Cached link-hover state for the active tab's grid. Mirrors the
/// WebView build's `LinkHandler`: a detected link under the pointer gets
/// its physical cells underlined (hover-only, no Ctrl), and the pointer
/// turns into a hand cursor while Ctrl is held over a link (Ctrl is what
/// arms the click-to-open).
#[derive(Default)]
pub(super) struct HoverState {
    /// Grid cell the last detection ran for (`None` = pointer outside the
    /// grid / no detection yet). Used to skip re-running detection on
    /// sub-cell pointer motion.
    cell: Option<(u16, u16)>,
    /// Physical cell spans of the link currently under the pointer
    /// (`(row, col_start, col_end)`), empty when no link is hovered. Read
    /// by the grid pass to underline the matched cells.
    pub(super) link_cells: Vec<(u16, u16, u16)>,
    /// The detected link itself, reused by the Ctrl+click handler so the
    /// click doesn't have to re-run detection for the same cell.
    link: Option<crate::links::DetectedLink>,
    /// Cached logical-line text for the last cell that ran detection.
    /// PTY-output re-detection skips `find_link_at` when this matches the
    /// current line text — meaning the PTY changed but not on the hovered
    /// line — to avoid per-frame regex work under a stationary pointer.
    last_line_text: Option<String>,
    /// FR3 discriminator: whether the active `link` was produced by the
    /// OSC 8 hyperlink path (`true`) or by the regex URL / file-path
    /// detector (`false`). OSC 8 hits survive AltScreen and the PTY-
    /// output invalidation guard; regex hits do not. The two paths
    /// share the rest of the `HoverState` fields (`link_cells` for
    /// the underline, `link.kind` for the click dispatch) so the
    /// renderer / cursor / click branches do not need a parallel API.
    is_osc8: bool,
}

impl WindowHost {
    /// Recompute the link-hover state for the current pointer position.
    /// Runs the detection regex only when the pointer crosses into a new
    /// grid cell (or leaves the grid); requests a redraw when the
    /// underlined span changes so the renderer repaints. Also refreshes
    /// the pointer icon (hand while Ctrl is held over a link).
    ///
    /// No-op for detection when both `url_detection` and
    /// `file_path_detection` are off, but the cursor icon is still
    /// reset so a stale hand cursor doesn't linger.
    pub(super) fn refresh_link_hover(&mut self, app: &App) {
        let detect_urls = app.settings.url_detection;
        let detect_paths = app.settings.file_path_detection;

        let new_cell = self.pixel_to_grid_cell(self.cursor_pos, app);

        // Re-run detection only when the target cell changed. The cursor
        // icon (Ctrl-dependent) is refreshed every call so toggling Ctrl
        // over a held position flips the hand on/off without motion.
        if new_cell != self.hover.cell {
            self.hover.cell = new_cell;
            let prev_cells = std::mem::take(&mut self.hover.link_cells);
            self.hover.link = None;
            self.hover.last_line_text = None;
            self.hover.is_osc8 = false;

            // FR3: OSC 8 hyperlink path runs FIRST, before the regex
            // gate and the AltScreen suppression. OSC 8 hits are a
            // first-class signal from the application (the program
            // explicitly marked the cell with `ESC]8;;<uri>ESC\`) so
            // they survive AltScreen where the regex auto-detector
            // does not. When the helper returns `Some(link)`, populate
            // the same hover fields the regex path uses and skip the
            // regex; the renderer underline + Ctrl-hand cursor + click
            // dispatch all reuse the existing infrastructure.
            if let (Some((row, col)), Some(tab)) = (new_cell, app.active_tab()) {
                let core = tab.core.lock();
                if let Some(link) = detect_osc8_link_at(&core, row, col) {
                    self.hover.link_cells = link.cells.clone();
                    self.hover.link = Some(link);
                    self.hover.is_osc8 = true;
                }
            }

            if !self.hover.is_osc8 && (detect_urls || detect_paths) && !app.alt_screen {
                if let (Some((row, col)), Some(tab)) = (new_cell, app.active_tab()) {
                    let core = tab.core.lock();
                    // Cache the logical-line text so PTY-output re-detection
                    // can skip `find_link_at` when the hovered line is
                    // unchanged.
                    self.hover.last_line_text = Some(crate::links::logical_line_text(&core, row));
                    if let Some(link) =
                        crate::links::find_link_at(&core, row, col, detect_urls, detect_paths)
                    {
                        self.hover.link_cells = link.cells.clone();
                        self.hover.link = Some(link);
                    }
                }
            }

            if hover_link_cells_changed(&prev_cells, &self.hover.link_cells) {
                // task0005 AC-1: latch so `render` forces a full redraw
                // and the affected rows actually rebuild in the row
                // cache (the request_redraw below only wakes the loop —
                // it does not by itself make `dirty_rows_this_frame`
                // non-empty).
                self.hover_span_changed = true;
                self.window().request_redraw();
            }
        }

        self.update_link_cursor();
    }

    /// Re-run link detection after a PTY update *only* when the hovered
    /// logical line actually changed under a stationary pointer.
    ///
    /// Called from the event loop's `about_to_wait` when `pty_changed &&
    /// pointer_in_window && !dragging`. The staleness check (compare the
    /// current logical-line text against the cached `last_line_text`) is
    /// owned here, alongside [`refresh_link_hover`], so the
    /// `HoverState` fields (`cell` / `last_line_text`) are never touched
    /// from the event loop body — that keeps the cache-invalidation
    /// policy in one place instead of duplicated across both sites.
    ///
    /// Applies the same detection gate as [`refresh_link_hover`]
    /// (`url_detection || file_path_detection`, and not on the alt
    /// screen) before doing any per-frame work, so high-throughput PTY
    /// output under a stationary pointer skips the `logical_line_text`
    /// allocation entirely when detection is disabled.
    pub(super) fn refresh_link_hover_on_pty_change(&mut self, app: &App) {
        // Alt-screen: clear any underline cache carried over from the normal
        // screen to prevent hover-underline bleed onto alt-screen content.
        // invalidate_link_hover is a no-op when link_cells is already empty,
        // so this is cheap on frames where no link was hovered.
        //
        // FR3: OSC 8 hover state survives AltScreen — the regex
        // detector is the only consumer with the AltScreen suppression
        // policy, so the guard only fires when the active hover came
        // from the regex path (`is_osc8 == false`). An OSC 8 hit
        // re-runs `refresh_link_hover` below so the cell range stays
        // accurate when scrollback shifts under the pointer.
        if app.alt_screen && !self.hover.is_osc8 {
            self.invalidate_link_hover();
            return;
        }

        let detect_urls = app.settings.url_detection;
        let detect_paths = app.settings.file_path_detection;
        // FR3: when the cached hover IS an OSC 8 hit, keep re-running
        // refresh_link_hover even if regex detection is off — OSC 8
        // doesn't go through the regex gate.
        if !self.hover.is_osc8 && !detect_urls && !detect_paths {
            return;
        }

        // Content-change guard: fetch the current logical-line text first.
        // If it matches the cache, the hovered line is unchanged and
        // `find_link_at` can be skipped entirely (avoiding the per-frame
        // alloc + regex during high-throughput output like `tail -f` or a
        // build log). Only when the text actually changed do we clear
        // `hover.cell` and let `refresh_link_hover` re-run detection.
        //
        // For OSC 8 hits the cached `last_line_text` is `None` (the
        // OSC 8 path doesn't populate it), so the comparison below
        // always falls through to the re-detection branch — that is
        // intentional: cheap to redo the cell read; correct when the
        // OSC 8 run shifts because content scrolled.
        if let Some((row, _col)) = self.hover.cell {
            let current_text = match app.active_tab() {
                Some(tab) => {
                    let core = tab.core.lock();
                    crate::links::logical_line_text(&core, row)
                }
                None => return,
            };
            let cached = self.hover.last_line_text.as_deref().unwrap_or("");
            if !self.hover.is_osc8 && current_text == cached {
                // Hovered line text is unchanged; existing hover state
                // (underline + link) is still valid.
                return;
            }
            // Line changed (or OSC 8 hover that always re-detects):
            // drop the cached cell so refresh re-detects.
            self.hover.cell = None;
            self.refresh_link_hover(app);
        } else {
            // No previously-hovered cell: let refresh_link_hover resolve
            // whether the pointer is now over a detectable cell.
            self.hover.cell = None;
            self.refresh_link_hover(app);
        }
    }

    /// Set the pointer icon by precedence: an active CSD resize zone wins,
    /// then a hand cursor when Ctrl is held over a detected link, else the
    /// default arrow. Skips the `set_cursor` IPC when the icon is
    /// unchanged. The hand is gated on Ctrl to match the WebView build,
    /// where Ctrl/Meta arms the click-to-open.
    pub(super) fn update_link_cursor(&mut self) {
        let icon = if self.current_resize_dir.is_some() {
            self.current_cursor // leave resize-hint icon untouched
        } else if self.current_mods.ctrl && self.hover.link.is_some() {
            CursorIcon::Pointer
        } else {
            CursorIcon::Default
        };
        if icon != self.current_cursor {
            self.current_cursor = icon;
            self.window.set_cursor(icon.into());
        }
    }

    /// Drop any cached link-hover state and clear a hand cursor. Called
    /// when the grid content shifts under the pointer (scroll) or the
    /// pointer leaves the window, so a stale underline / cursor doesn't
    /// survive. Requests a redraw when an underline was showing.
    pub(super) fn invalidate_link_hover(&mut self) {
        self.hover.cell = None;
        self.hover.link = None;
        self.hover.last_line_text = None;
        self.hover.is_osc8 = false;
        let had = !self.hover.link_cells.is_empty();
        self.hover.link_cells.clear();
        if had {
            // task0005 AC-1: same latch as `refresh_link_hover` above —
            // a span disappearing needs its rows rebuilt just as much as
            // one appearing or moving.
            self.hover_span_changed = true;
            self.window().request_redraw();
        }
        self.update_link_cursor();
    }

    /// Open the link under the pointer (URL via the OS opener, file path
    /// via the configured editor). Returns `true` when a link was found
    /// at the click cell (so the caller skips starting a selection),
    /// regardless of whether the spawn ultimately succeeded — a blocked
    /// scheme or missing file is still "the user clicked a link".
    ///
    /// Always re-runs detection against the live grid at click time so
    /// the result reflects the current terminal content regardless of
    /// hover-cache staleness (terminal output, tab switches, alt-screen
    /// transitions, or settings changes since the last pointer-move can
    /// all mutate the visible content under a stationary pointer).
    pub(super) fn try_open_link_at_pointer(&mut self, app: &App) -> bool {
        let detect_urls = app.settings.url_detection;
        let detect_paths = app.settings.file_path_detection;
        let click_cell = self.pixel_to_grid_cell(self.cursor_pos, app);
        let Some((row, col)) = click_cell else {
            return false;
        };

        // FR3: OSC 8 lookup runs FIRST and is NOT gated on
        // `url_detection` / `file_path_detection` (those settings
        // control the regex auto-detector only — OSC 8 is an explicit
        // application-supplied signal). It also bypasses the AltScreen
        // short-circuit so PR-ID links inside Claude Code's AltScreen
        // are clickable. Dispatch via the same `LinkKind::Url ->
        // open_url` arm the regex hit uses.
        if let Some(tab) = app.active_tab() {
            let core = tab.core.lock();
            if let Some(link) = detect_osc8_link_at(&core, row, col) {
                drop(core);
                if let crate::links::LinkKind::Url(url) = link.kind {
                    if crate::links::is_safe_uri(&url) {
                        open_url(&url);
                    } else {
                        log::warn!("native-poc: refusing to open unsafe OSC 8 URI scheme: {url}");
                    }
                }
                return true;
            }
        }

        if !detect_urls && !detect_paths {
            return false;
        }
        // Guard against alt-screen: mirrors the same condition applied in
        // `refresh_link_hover` so hover and click use identical detection
        // rules. (OSC 8 already returned above for AltScreen cases.)
        if app.alt_screen {
            return false;
        }

        // Always re-detect from the live grid; clicks are infrequent so
        // the regex cost is negligible, and this prevents acting on a
        // stale DetectedLink from the hover cache.
        let link = if let Some(tab) = app.active_tab() {
            let core = tab.core.lock();
            crate::links::find_link_at(&core, row, col, detect_urls, detect_paths)
        } else {
            None
        };

        let Some(link) = link else {
            return false;
        };

        match link.kind {
            crate::links::LinkKind::Url(url) => {
                if crate::links::is_safe_uri(&url) {
                    open_url(&url);
                } else {
                    log::warn!("native-poc: refusing to open unsafe URI scheme: {url}");
                }
            }
            crate::links::LinkKind::FilePath { path, line, col } => {
                self.open_file_in_editor(app, &path, line, col);
            }
        }
        true
    }

    /// Resolve `file_path` against the active tab's OSC 7 CWD, verify the
    /// file exists, then spawn `settings.editor_command` with `{file}` /
    /// `{line}` / `{col}` expanded. Mirrors `openFileInEditor` in the
    /// WebView build's `link.ts`: existence is checked only at click time
    /// (not on hover), a relative path with no CWD passes through as-is
    /// (SPEC FR6), and a blank editor command / absent file is a logged
    /// no-op.
    fn open_file_in_editor(&self, app: &App, file_path: &str, line: u32, col: u32) {
        let editor = app.settings.editor_command.trim();
        if editor.is_empty() {
            log::warn!("native-poc: editor_command is blank; not opening {file_path}");
            return;
        }
        let cwd = app.active_tab().and_then(|t| t.cb_state.lock().cwd.clone());
        // SPEC FR6: with no CWD a relative path passes through as-is; the
        // is_file() check below then decides whether anything opens.
        let resolved = crate::links::resolve_path(file_path, cwd.as_deref());
        if !std::path::Path::new(&resolved).is_file() {
            log::warn!("native-poc: file not found, not opening: {resolved}");
            app.notify("ファイルが見つかりません", &resolved);
            return;
        }
        // Canonicalize to an absolute path so a leading-dash path (e.g.
        // `-S/tmp/x.vim:1`) cannot be passed as an option to the editor.
        let canonical = match std::fs::canonicalize(&resolved) {
            Ok(p) => p,
            Err(e) => {
                log::warn!("native-poc: canonicalize failed for {resolved}: {e}");
                app.notify("ファイルが見つかりません", &resolved);
                return;
            }
        };
        let canonical_str = canonical.to_string_lossy();
        let Some((program, args)) = crate::links::build_editor_command(
            &app.settings.editor_command,
            &canonical_str,
            line,
            col,
        ) else {
            log::warn!("native-poc: editor_command produced no program; not opening");
            return;
        };
        match std::process::Command::new(&program)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(mut child) => {
                log::info!("native-poc: opened {canonical_str} in editor ({program})");
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(e) => {
                log::warn!("native-poc: failed to spawn editor {program}: {e}");
                app.notify("エディタの起動に失敗しました", &e.to_string());
            }
        }
    }
}

/// task0005 AC-1: whether the hovered link's cell-span changed (appear,
/// move, or disappear). Extracted as a pure equality check so
/// `refresh_link_hover` / `invalidate_link_hover`'s latch-setting logic is
/// directly unit-testable without a window (mirrors `should_skip_frame`
/// below).
pub(super) fn hover_link_cells_changed(
    prev_cells: &[(u16, u16, u16)],
    new_cells: &[(u16, u16, u16)],
) -> bool {
    prev_cells != new_cells
}

/// FR3: synthesize a `DetectedLink` for an OSC 8 hyperlinked cell at
/// `(row, col)`. Returns `None` for cells with `hyperlink_id == 0`
/// (no link), for ids whose URI is missing from `hyperlink_table`
/// (e.g. evicted from scrollback), for empty URIs, and for URIs whose
/// scheme fails [`crate::links::is_safe_uri`] (unsafe schemes such as
/// `javascript:` / `data:` are rejected with a `warn` log so the
/// click-time branch can dispatch unconditionally on `Some`).
///
/// On a hit the cell range is expanded leftward and rightward across
/// contiguous cells on the same row carrying the same `hyperlink_id`,
/// so the renderer underlines the whole OSC 8 run instead of a single
/// cell. The bound is the row width, which is small (≤ a few hundred).
pub(super) fn detect_osc8_link_at(
    core: &term_core::terminal_core::TerminalCore,
    row: u16,
    col: u16,
) -> Option<crate::links::DetectedLink> {
    let cols = core.cols();
    if cols == 0 || row >= core.rows() || col >= cols {
        return None;
    }
    let id = core.get_cell_hyperlink_id(col, row);
    if id == 0 {
        return None;
    }
    let uri = core.get_hyperlink_uri(id);
    if uri.is_empty() {
        // Missing id in `hyperlink_table` (id evicted with the cell row
        // it was last seen on, or never registered) or an OSC 8 with
        // an empty URI. Either way there is nothing to open.
        return None;
    }
    if !crate::links::is_safe_uri(&uri) {
        log::warn!("native-poc: refusing OSC 8 URI with unsafe scheme: {uri}");
        return None;
    }
    // Expand the run leftward.
    let mut col_start = col;
    while col_start > 0 && core.get_cell_hyperlink_id(col_start - 1, row) == id {
        col_start -= 1;
    }
    // Expand the run rightward (exclusive end column).
    let mut col_end = col + 1;
    while col_end < cols && core.get_cell_hyperlink_id(col_end, row) == id {
        col_end += 1;
    }
    Some(crate::links::DetectedLink {
        kind: crate::links::LinkKind::Url(uri),
        cells: vec![(row, col_start, col_end)],
    })
}

/// Hand a (safe-scheme-checked) URL to the OS opener. Linux uses
/// `xdg-open`; Windows uses ShellExecuteW via the `opener` crate. A
/// spawn failure is logged at `warn` and otherwise ignored — the
/// WebView build similarly swallows opener errors. The caller is
/// responsible for the `is_safe_uri` gate.
fn open_url(url: &str) {
    #[cfg(target_os = "linux")]
    {
        match std::process::Command::new("xdg-open")
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(mut child) => {
                log::info!("native-poc: opened URL via xdg-open: {url}");
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(e) => log::warn!("native-poc: xdg-open failed for {url}: {e}"),
        }
    }
    #[cfg(target_os = "windows")]
    {
        // ShellExecuteW receives the URL as a bare parameter — unlike
        // `cmd /c start`, no cmd.exe parsing happens, so cmd
        // metacharacters (`&`, `|`, `^`) in a PTY-supplied URL cannot
        // inject commands.
        match opener::open(url) {
            Ok(()) => log::info!("native-poc: opened URL via ShellExecuteW: {url}"),
            Err(e) => log::warn!("native-poc: ShellExecuteW failed for {url}: {e}"),
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        log::warn!("native-poc: no URL opener on this platform for {url}");
    }
}
