//! Key routing ahead of the generic PTY encoder: special chords, the
//! profile-selector / mux-dialog / search-bar key handlers, and the
//! egui -> mux key-input translation used by the mux prefix engine.

use winit::event::KeyEvent;
use winit::keyboard::{Key as WinitKey, NamedKey};

use crate::app::App;
use crate::mux::dialog::{MuxDialogOutcome, MuxDialogState};
use crate::mux::prefix::{KeyInput as MuxKeyInput, KeySym};
use crate::pty::input::Modifiers;
use crate::ui::keybinds::Chord;

use super::WindowHost;
use super::input_translate::{input_mods_to_egui, winit_key_to_egui};

/// Drive one frame of the open mux dialog: render via the UI layer
/// (`ui::mux_dialogs::draw`) and dispatch the resulting outcome into the
/// domain layer (`App::confirm_mux_*`). This is the orchestration glue
/// that previously lived in `ui::mux_dialogs::drive`; moved here so the UI
/// module no longer has to `use crate::app::App` (otherwise the UI layer
/// imports App, and App imports UI types like `TabEvent` — a cycle).
/// `window_host` already owns `App`, so dispatch lives at this boundary.
pub(super) fn drive_mux_dialogs(app: &mut App, ctx: &egui::Context) -> bool {
    if !app.mux_dialog.is_open() {
        return false;
    }
    // Reconcile against any daemon-driven changes that arrived since the
    // dialog opened (PaneCreated / PtyExited / SwitchWindow). If the
    // captured window vanished, refresh_mux_dialog flips the state to
    // Closed; we then early-return without drawing.
    app.refresh_mux_dialog();
    if !app.mux_dialog.is_open() {
        return false;
    }
    let locale = app.locale;
    let outcome = crate::ui::mux_dialogs::draw(&mut app.mux_dialog, ctx, locale);
    match outcome {
        MuxDialogOutcome::Pending => {}
        MuxDialogOutcome::ConfirmRename { window_id, name } => {
            app.mux_dialog = MuxDialogState::Closed;
            app.confirm_mux_rename(window_id, name);
        }
        MuxDialogOutcome::ConfirmMove { window_id, target } => {
            app.mux_dialog = MuxDialogState::Closed;
            app.confirm_mux_move(window_id, target);
        }
        MuxDialogOutcome::Cancelled => {
            app.mux_dialog = MuxDialogState::Closed;
        }
    }
    true
}

/// Convert an (egui::Key, current modifiers) pair from the winit event
/// pipeline into the framework-agnostic [`MuxKeyInput`] the mux prefix
/// latch consumes. Keeps the egui→domain translation pinned to this
/// single boundary site (gpt-architecture #4).
pub(super) fn egui_to_mux_input(mods: Modifiers, key: egui::Key) -> MuxKeyInput {
    let sym = match key {
        egui::Key::A => KeySym::Letter('a'),
        egui::Key::B => KeySym::Letter('b'),
        egui::Key::C => KeySym::Letter('c'),
        egui::Key::D => KeySym::Letter('d'),
        egui::Key::E => KeySym::Letter('e'),
        egui::Key::F => KeySym::Letter('f'),
        egui::Key::G => KeySym::Letter('g'),
        egui::Key::H => KeySym::Letter('h'),
        egui::Key::I => KeySym::Letter('i'),
        egui::Key::J => KeySym::Letter('j'),
        egui::Key::K => KeySym::Letter('k'),
        egui::Key::L => KeySym::Letter('l'),
        egui::Key::M => KeySym::Letter('m'),
        egui::Key::N => KeySym::Letter('n'),
        egui::Key::O => KeySym::Letter('o'),
        egui::Key::P => KeySym::Letter('p'),
        egui::Key::Q => KeySym::Letter('q'),
        egui::Key::R => KeySym::Letter('r'),
        egui::Key::S => KeySym::Letter('s'),
        egui::Key::T => KeySym::Letter('t'),
        egui::Key::U => KeySym::Letter('u'),
        egui::Key::V => KeySym::Letter('v'),
        egui::Key::W => KeySym::Letter('w'),
        egui::Key::X => KeySym::Letter('x'),
        egui::Key::Y => KeySym::Letter('y'),
        egui::Key::Z => KeySym::Letter('z'),
        egui::Key::Num0 => KeySym::Digit(0),
        egui::Key::Num1 => KeySym::Digit(1),
        egui::Key::Num2 => KeySym::Digit(2),
        egui::Key::Num3 => KeySym::Digit(3),
        egui::Key::Num4 => KeySym::Digit(4),
        egui::Key::Num5 => KeySym::Digit(5),
        egui::Key::Num6 => KeySym::Digit(6),
        egui::Key::Num7 => KeySym::Digit(7),
        egui::Key::Num8 => KeySym::Digit(8),
        egui::Key::Num9 => KeySym::Digit(9),
        egui::Key::Comma => KeySym::Comma,
        egui::Key::Period => KeySym::Period,
        egui::Key::Semicolon => KeySym::Semicolon,
        egui::Key::Slash => KeySym::Slash,
        egui::Key::Backslash => KeySym::Backslash,
        egui::Key::Minus => KeySym::Minus,
        _ => KeySym::Other,
    };
    MuxKeyInput {
        ctrl: mods.ctrl,
        shift: mods.shift,
        alt: mods.alt,
        key: sym,
    }
}

/// Intercept Phase 4 chords. Returns `true` when the event was consumed
/// (the generic encoder should not run).
///
/// `egui_key` is the pre-computed result of `winit_key_to_egui` for this
/// event, passed in so the caller can reuse the same value for the
/// keybinds dispatch path without a second translation.
pub(super) fn handle_special_chord(
    event: &KeyEvent,
    mods: Modifiers,
    egui_key: Option<egui::Key>,
    host: &mut WindowHost,
    app: &mut App,
) -> bool {
    // Clipboard chords are settings-driven (`keybinds.copy` /
    // `keybinds.paste`). Build the incoming chord from the winit event +
    // modifiers and compare against the resolved table.
    if let Some(key) = egui_key {
        let chord = Chord {
            ctrl: mods.ctrl,
            shift: mods.shift,
            alt: mods.alt,
            key,
        };
        if chord == app.keybinds.copy {
            // Copy current selection to CLIPBOARD. We consume the chord
            // even when there is no active selection so the configured
            // copy key never leaks through to the PTY.
            if let Some(sel) = app.selection {
                if let Some(tab) = app.tabs.get(app.active) {
                    let core = tab.core.lock();
                    let text = sel.resolve(&core, app.fold_layout());
                    drop(core);
                    host.set_clipboard(&text);
                }
            }
            return true;
        }
        if chord == app.keybinds.paste {
            if let Some(text) = host.get_clipboard() {
                host.deliver_paste(app, &text);
            }
            return true;
        }
    }

    // Scrollback chords use Shift + nav keys.
    if mods.shift && !mods.ctrl && !mods.alt {
        match &event.logical_key {
            WinitKey::Named(NamedKey::PageUp) => {
                let rows = app.cell_size.rows.max(1) as u32;
                app.scroll_up_by(rows);
                // Viewport shifted under the pointer; cached hover is stale.
                host.invalidate_link_hover();
                return true;
            }
            WinitKey::Named(NamedKey::PageDown) => {
                let rows = app.cell_size.rows.max(1) as u32;
                app.scroll_down_by(rows);
                host.invalidate_link_hover();
                return true;
            }
            WinitKey::Named(NamedKey::Home) => {
                app.scroll_to_top();
                host.invalidate_link_hover();
                return true;
            }
            WinitKey::Named(NamedKey::End) => {
                app.scroll_to_live();
                host.invalidate_link_hover();
                return true;
            }
            _ => {}
        }
    }
    false
}

/// Route a key press into the search overlay while it owns the keyboard.
///
/// Precedence (mirrors `SearchBar.keydown` + the WebView nav bindings):
///   1. `Esc` → close the overlay + clear state.
///   2. `Enter` / `Shift+Enter` → next / previous match.
///   3. `keybinds.copy` / `keybinds.paste` chords → inject an
///      `egui::Event::Copy` / `egui::Event::Paste(text)` so the field's
///      own clipboard handling fires (copy of the field selection, paste
///      of the OS clipboard into the field).
///   4. Everything else → forward to egui as an `Event::Key` plus, when
///      the key produced committed text and no Ctrl/Alt is held, an
///      `Event::Text` so the TextEdit inserts the character.
///
/// The terminal IME dispatch + PTY encoder are intentionally bypassed:
/// while searching, keystrokes belong to the search field, not the shell.
/// Keyboard handling while the profile-selector modal is visible. The
/// modal owns the keyboard completely: navigation / confirm / cancel act
/// on the selector state, every other key is swallowed (never encoded to
/// the PTY). Port of `profile-selector.ts::handleKeydown` (ArrowUp /
/// ArrowDown wrap, Home / End, Enter / Space confirm, Escape cancel).
pub(super) fn handle_profile_selector_key(event: &KeyEvent, app: &mut App) {
    use winit::keyboard::NamedKey;

    // Row count includes the synthetic "Global Settings" row in new-tab
    // chooser mode.
    let len = app.profile_selector_row_count();
    match &event.logical_key {
        WinitKey::Named(NamedKey::Escape) => app.profile_selector.close(),
        WinitKey::Named(NamedKey::ArrowDown) => app.profile_selector.move_selection(1, len),
        WinitKey::Named(NamedKey::ArrowUp) => app.profile_selector.move_selection(-1, len),
        WinitKey::Named(NamedKey::Home) => app.profile_selector.select_edge(false, len),
        WinitKey::Named(NamedKey::End) => app.profile_selector.select_edge(true, len),
        // winit 0.31 removed `NamedKey::Space`; the space bar now arrives
        // as `Character(" ")`, already covered by the arm below.
        WinitKey::Named(NamedKey::Enter) => {
            let idx = app.profile_selector.selected;
            app.confirm_profile_selection(idx);
        }
        WinitKey::Character(c) if c == " " => {
            let idx = app.profile_selector.selected;
            app.confirm_profile_selection(idx);
        }
        _ => {}
    }
}

/// Forward a key press into egui while a mux rename / move dialog is open.
/// Mirrors the search-bar capture: editing keys (Backspace / arrows /
/// Enter / Escape …) go through as egui `Key` events and printable text as
/// `Text` events, so the dialog's `TextEdit` / `DragValue` and its
/// Enter-confirm / Escape-cancel handling work. The terminal IME backend,
/// the keybind dispatcher, and the PTY encoder never see the key — without
/// this gate, typing in the dialog would leak into the running shell.
pub(super) fn handle_mux_dialog_key(event: &KeyEvent, mods: Modifiers, host: &mut WindowHost) {
    if let Some(key) = winit_key_to_egui(&event.logical_key) {
        host.pending_egui_events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: event.repeat,
            modifiers: input_mods_to_egui(mods),
        });
    }
    // Printable characters insert into the focused field. Suppressed while
    // Ctrl/Alt is held so control chords do not also emit a literal glyph.
    if !mods.ctrl && !mods.alt {
        if let Some(text) = &event.text {
            let printable: String = text.chars().filter(|c| !c.is_control()).collect();
            if !printable.is_empty() {
                host.pending_egui_events.push(egui::Event::Text(printable));
            }
        }
    }
}

pub(super) fn handle_search_key(
    event: &KeyEvent,
    mods: Modifiers,
    host: &mut WindowHost,
    app: &mut App,
) {
    use winit::keyboard::NamedKey;

    // 1. Esc closes the overlay.
    if matches!(event.logical_key, WinitKey::Named(NamedKey::Escape)) {
        app.close_search();
        return;
    }

    // 2. Enter / Shift+Enter navigate. Handled before egui so the field's
    //    default Enter (which does nothing useful for a single-line edit)
    //    never swallows them.
    if matches!(event.logical_key, WinitKey::Named(NamedKey::Enter)) {
        if mods.shift {
            app.search_prev();
        } else {
            app.search_next();
        }
        // Same contract as the search-bar button path in `render()`:
        // match navigation can scroll the viewport (and expand folds),
        // so the cached hover spans index the pre-jump viewport.
        host.invalidate_link_hover();
        return;
    }

    let egui_key = winit_key_to_egui(&event.logical_key);

    // 3. Copy / paste chords → egui clipboard events targeting the field.
    if let Some(key) = egui_key {
        let chord = Chord {
            ctrl: mods.ctrl,
            shift: mods.shift,
            alt: mods.alt,
            key,
        };
        // Re-pressing the search chord while the overlay is open re-focuses
        // the field + reselects the query (rather than inserting an 'f').
        if chord == app.keybinds.search {
            app.open_search();
            return;
        }
        if chord == app.keybinds.copy {
            host.pending_egui_events.push(egui::Event::Copy);
            return;
        }
        if chord == app.keybinds.paste {
            if let Some(text) = host.get_clipboard() {
                host.pending_egui_events.push(egui::Event::Paste(text));
            }
            return;
        }
    }

    // 4. Forward as an egui key event so the TextEdit can act on editing
    //    keys (Backspace / Delete / arrows / Home / End / Ctrl+A …).
    if let Some(key) = egui_key {
        host.pending_egui_events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: event.repeat,
            modifiers: input_mods_to_egui(mods),
        });
    }

    // …and forward the committed text for character insertion. Suppressed
    // when Ctrl/Alt is held so control chords (e.g. Ctrl+A select-all) do
    // not also insert a literal character into the field.
    if !mods.ctrl && !mods.alt {
        if let Some(text) = &event.text {
            // Drop control characters (e.g. the Enter/Tab text payloads
            // winit attaches) — printable text only reaches the field.
            let printable: String = text.chars().filter(|c| !c.is_control()).collect();
            if !printable.is_empty() {
                host.pending_egui_events.push(egui::Event::Text(printable));
            }
        }
    }
}
