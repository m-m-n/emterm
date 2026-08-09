//! Profile selector / new-tab chooser flow for [`App`].

use super::App;

impl App {
    /// `profile_selector` keybind: open the modal selector. No-op when no
    /// profiles are configured (WebView parity:
    /// `keyboard-handler.ts::handleProfileSelector`).
    pub fn open_profile_selector(&mut self) {
        if self.settings.profiles.is_empty() {
            return;
        }
        self.profile_selector.open();
        self.needs_full_redraw = true;
    }

    /// Tab-bar `+` button (`TabEvent::New`): spawn directly with the
    /// global settings when no profiles exist and no tmux rows were
    /// found, otherwise open the new-tab chooser (a "Global Settings"
    /// row + each profile + each discovered tmux session / fallback
    /// socket row, with the default profile preselected). Port of the
    /// WebView's `tab-bar-ui.ts::handleNewTabClick` dialog, extended per
    /// task0001 AC-6: the fast path now requires BOTH profiles and tmux
    /// entries to be empty (previously profiles alone).
    pub fn open_new_tab_chooser(&mut self) {
        let entries = discover_tmux_entries();
        self.open_new_tab_chooser_with_entries(entries);
    }

    /// Core decision logic behind [`Self::open_new_tab_chooser`], taking
    /// the discovered tmux entries as a parameter. Split out so AC-6
    /// (profiles-empty × entries-empty/non-empty) is unit-testable
    /// without touching the real socket directory (`tdd-testing`: "test
    /// the pure decision core").
    pub(super) fn open_new_tab_chooser_with_entries(
        &mut self,
        entries: Vec<crate::ui::profile_selector::TmuxRow>,
    ) {
        if self.settings.profiles.is_empty() && entries.is_empty() {
            self.spawn_new_tab();
            return;
        }
        // Preselect the default profile's row (chooser mode prepends a
        // "Global Settings" row, so the row<->profile offset lives in
        // `ProfileSelectorState::profile_row`). No default → row 0
        // (Global Settings). Tmux entries never carry a "default", so
        // they never move the initial selection.
        self.profile_selector.open_with_global(0);
        self.profile_selector.tmux_entries = entries;
        if let Some(i) = self.settings.profiles.iter().position(|p| p.is_default) {
            self.profile_selector.selected = self.profile_selector.profile_row(i);
        }
        self.needs_full_redraw = true;
    }

    /// Number of rows the open selector shows (profiles + discovered tmux
    /// entries, plus the leading "Global Settings" row in new-tab chooser
    /// mode). Drives the keyboard wrap-around in `window_host`.
    pub fn profile_selector_row_count(&self) -> usize {
        self.settings.profiles.len()
            + self.profile_selector.tmux_entries.len()
            + usize::from(self.profile_selector.include_global)
    }

    /// Selector confirmed: resolve the chosen row and spawn a tab. The
    /// row→choice decode (including the chooser-mode "Global Settings" /
    /// tmux-entry offsets) lives in `ProfileSelectorState::row_to_choice`,
    /// the single authority shared with the renderer. Resolution failures
    /// log an error and spawn nothing (WebView parity with
    /// `launchSshProfile`'s alert path).
    pub fn confirm_profile_selection(&mut self, index: usize) {
        let choice = self
            .profile_selector
            .row_to_choice(index, self.settings.profiles.len());
        self.profile_selector.close();
        let profile_index = match choice {
            crate::ui::profile_selector::Choice::Global => {
                self.spawn_new_tab();
                return;
            }
            crate::ui::profile_selector::Choice::Tmux(i) => {
                // AC-5: attach is a plain PTY spawn (`tmux -S <socket>
                // attach[-session]`), not a mux-subsystem integration
                // (IMPLEMENTATION.md). `argv` was built by the shared
                // attach-argument rule (`tmux_sockets::attach_args`) when
                // the entry was discovered.
                let Some(entry) = self.profile_selector.tmux_entries.get(i) else {
                    return;
                };
                let overrides = crate::profiles::SpawnOverrides {
                    shell_path: Some("tmux".to_string()),
                    shell_args: Some(entry.argv.clone()),
                    ..Default::default()
                };
                self.spawn_new_tab_with_overrides(Some(overrides));
                return;
            }
            crate::ui::profile_selector::Choice::Profile(i) => i,
        };
        let Some(profile) = self.settings.profiles.get(profile_index) else {
            return;
        };
        match crate::profiles::resolve_spawn(profile, &self.settings) {
            Ok(overrides) => self.spawn_new_tab_with_overrides(Some(overrides)),
            Err(e) => log::error!("profile {:?}: {e}", profile.name),
        }
    }
}

/// Discover every tmux row for the new-tab chooser (task0001, SPEC A5):
/// one per live session, one fallback per un-enumerable socket. Always
/// empty on Windows (`crate::tmux_sockets` is Unix-only — task0001 Out
/// of Scope), so the chooser's fast-path / row-count logic in `App`
/// needs no platform branching beyond this one function. Labels and
/// spawn argv are precomputed here via the shared label / attach-
/// argument rules (`tmux_sockets::label` / `tmux_sockets::attach_args`)
/// so `ui::profile_selector::TmuxRow` stays a plain, cross-platform type
/// that never needs to name the Unix-only `tmux_sockets` module.
#[cfg(unix)]
fn discover_tmux_entries() -> Vec<crate::ui::profile_selector::TmuxRow> {
    crate::tmux_sockets::enumerate()
        .iter()
        .map(|entry| crate::ui::profile_selector::TmuxRow {
            label: crate::tmux_sockets::label(entry),
            argv: crate::tmux_sockets::attach_args(entry),
        })
        .collect()
}

#[cfg(not(unix))]
fn discover_tmux_entries() -> Vec<crate::ui::profile_selector::TmuxRow> {
    Vec::new()
}
