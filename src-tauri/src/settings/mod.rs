//! Runtime settings for the GUI build.
//!
//! `Language` and `settings_path` are re-exported from `crate::settings_core`
//! so the CLI-only build can read them without compiling the rest of this
//! module.

pub use crate::settings_core::{Language, settings_path};

mod types;
pub use types::*;

mod raw;
use raw::*;

#[derive(Debug, Clone)]
pub struct Settings {
    pub ambiguous_width_mode: AmbiguousWidthMode,
    /// Font rasterizer engine. See [`FontEngine`]. Defaults to `Swash`
    /// per FR6.
    pub font_engine: FontEngine,
    /// Ordered list of additional font families (after the base font and
    /// before the bundled CJK / emoji fonts) to consult during fallback
    /// resolution. Each entry is a fontdb family name.
    #[allow(dead_code)] // Phase 7: settings.json loader will populate this.
    pub font_family_fallback: Vec<String>,
    /// Variable-font axis settings (e.g. `("wght", 700.0)`). Applied by
    /// the swash adapter to shaping, rasterization, and metrics of every
    /// registered font; ignored (with a WARN) under `font_engine =
    /// ab_glyph`.
    pub variable_font_axes: std::collections::HashMap<String, f32>,
    /// Maximum number of rows preserved above the viewport before old lines
    /// are dropped. Plumbed to `TerminalCore::new` at tab spawn time.
    pub scrollback_lines: u32,
    /// Cap on decoded-image memory in megabytes, held by the image-viewer
    /// router while a `Place` is awaited. Eviction is least-recently-used
    /// (see `crate::viewer::image::ImageViewerRouter`).
    pub image_memory_quota_mb: u32,
    /// Whether OSC 52 *read* (clipboard query `? ` form) is permitted.
    /// Writes are always allowed if size ≤ `clipboard_max_size_osc52`.
    /// Default `true` matches the legacy WebView build, where the same
    /// `settings.json` field controls behavior.
    pub clipboard_read_osc52: bool,
    /// Maximum payload size accepted by OSC 52 in bytes. Payloads above
    /// this cap are dropped and `LOG_OSC52_DENIED` is emitted.
    pub clipboard_max_size_osc52: u32,
    /// Prefix-key chord for mux mode. Stored as a textual spec (e.g.
    /// `"Ctrl+Z"`) and parsed at startup by
    /// `crate::mux::prefix::parse_prefix_key`. Phase 7 will surface this in
    /// `settings.json`; today the field is populated from
    /// [`DEFAULT_MUX_PREFIX_KEY`] and never mutated.
    #[allow(dead_code)] // Phase 4-D status bar / settings UI will consume this.
    pub mux_prefix_key: String,
    /// Resolved mux UI settings (`mux.tab_always_expand` / `keybinds`
    /// / `statusbar.*`). Consumed by the mux tab group, the
    /// prefix latch, and the mux status row.
    pub mux: MuxSettings,
    /// Status-bar widget configuration. See [`StatusBarSettings`]. Phase
    /// 4-D introduces this; today the value is always the default until
    /// Phase 7 wires `settings.json` loading.
    pub statusbar: StatusBarSettings,
    /// Keyboard-shortcut chord specs. See [`KeybindSettings`]. Parsed
    /// into a [`crate::ui::keybinds::KeybindTable`] at startup so the
    /// dispatcher can match against user-configured chords.
    pub keybinds: KeybindSettings,
    /// IME backend configuration. See [`ImeSettings`]. Phase 4-G
    /// introduces `native_integration` (default `true`). Phase 7 wires
    /// JSON loading; until then `Settings::default()` exercises the
    /// default shape only.
    #[allow(dead_code)] // Phase 7: settings.json loader will populate this.
    pub ime: ImeSettings,
    /// Terminal cell font size in logical points. Mirrors the legacy
    /// WebView build's `font_size`. Plumbed into
    /// `crate::render::theme::Theme::font_size_pt` at tab spawn time
    /// so `TerminalGridPass::CellMetrics::font_size_px` resolves to
    /// this value × the host's `pixels_per_point`.
    pub font_size: f32,
    /// Inner padding (logical pixels) between the window edge and the
    /// terminal cell grid. Plumbed into the renderer's `TOP_PAD` /
    /// `LEFT_PAD` so the user-configured strip surrounds the grid.
    pub padding: u32,
    /// Cursor visual style. See [`CursorStyle`]. Plumbed into the
    /// per-tab `Theme::cursor_style` at spawn time; OSC 22 may still
    /// mutate it at runtime.
    pub cursor_style: CursorStyle,
    /// Whether the terminal cursor blinks. Plumbed into
    /// `TerminalCore::set_cursor_blink` at tab spawn time; DECTCEM /
    /// app-driven mode changes may still override at runtime.
    pub cursor_blink: bool,
    /// UI brightness mode. `Light`/`Dark`/`System`. Picks the light or
    /// dark variant of the `ui::md3` preset palette at startup. `System`
    /// resolves to dark (no desktop-portal brightness lookup yet).
    pub ui_theme: UiTheme,
    /// MD3 accent color preset for the UI chrome. Swaps `md3::PRIMARY`
    /// at runtime so tab indicator / focused borders pick up the user's
    /// hue choice.
    pub ui_theme_preset: UiThemePreset,
    /// UI font family override for the egui-rendered chrome. Prepended
    /// to the `Proportional` chain (tab bar / title bar) by
    /// `window_host::configure_egui_fonts`; the status bar mirrors the
    /// WebView's terminal-font styling and stays on `Monospace`. Empty
    /// string keeps egui's bundled default.
    pub ui_font_family: String,
    /// Terminal color-scheme preset name. When non-empty and matched
    /// either by [`COLOR_SCHEME_PRESETS`] or by [`Settings::custom_color_schemes`],
    /// drives `Theme::{fg, bg, cursor_fg, palette16}` at spawn time.
    pub terminal_color_scheme: String,
    /// User-defined terminal color schemes. Looked up by name from
    /// `terminal_color_scheme`. User schemes override preset names of
    /// the same value.
    pub custom_color_schemes: Vec<UserColorScheme>,
    /// Scrollbar visibility policy for the terminal viewport
    /// (`ui::scrollbar`). `Auto` shows the bar only when scrollback
    /// content exists, mirroring the WebView's `overflow-y: auto`.
    pub show_scrollbar: ScrollbarMode,
    /// Whether the tab bar is drawn. `false` hides it entirely; the
    /// central terminal panel takes the freed vertical space.
    pub show_tab_bar: bool,
    /// When `true`, SGR `bold` promotes indexed ANSI colors 0-7 to their
    /// bright variants (8-15) on the foreground. Matches xterm's
    /// historical behavior and the WebView build's default. Plumbed into
    /// `Theme::bold_brightens_ansi_colors` at spawn time.
    pub bold_brightens_ansi_colors: bool,
    /// Default shell executable for newly spawned tabs. Empty string
    /// keeps the historical `$SHELL` / `/bin/sh` fallback in
    /// `PtySession::spawn`.
    pub shell_path: String,
    /// Argv tail passed to the spawned shell after the executable. Each
    /// entry becomes a separate argv slot.
    pub shell_args: Vec<String>,
    /// Number of grid rows scrolled per wheel notch when the mouse
    /// wheel delivers one line. Clamped at the call site to keep the
    /// behavior identical to the WebView build.
    pub scroll_speed: u32,
    /// DECSET 1007 user opt-out for AltScreen wheel→arrow translation.
    /// When `true` (default), a wheel event in alternate screen with the
    /// terminal-side `MODE_ALTERNATE_SCROLL` bit on emits arrow-key
    /// bytes to the active PTY so AltScreen apps (Claude Code, vim,
    /// less) scroll their own log; when `false`, the wheel falls
    /// through to the eMterm scrollback view as before.
    pub alternate_scroll_enabled: bool,
    /// When `true`, releasing a left-click selection also copies the
    /// resolved text to the system CLIPBOARD selection (the PRIMARY
    /// selection is always updated regardless).
    pub copy_on_select: bool,
    /// When `true`, a middle-click pastes the current PRIMARY selection
    /// into the terminal. When `false`, middle-click is a no-op.
    pub middle_click_paste: bool,
    /// `Shift+Enter` key-rewrite behavior. See [`ShiftEnterBehavior`].
    /// Default `AltEnter` mirrors the legacy `true` behavior (helps
    /// integrate with editors / shells that bind a multi-line
    /// continuation on `M-RET`).
    pub shift_enter_behavior: ShiftEnterBehavior,
    /// When `true` (default), a bare `Ctrl+J` press is withheld from
    /// the PTY. Emacs-style IMEs (SKK) use `Ctrl+J` for mode switching;
    /// without the skip the chord encodes to LF (`0x0A`) and inserts
    /// unwanted newlines. Mirrors the WebView build's keyboard-handler
    /// skip (`src/terminal-app/handlers/keyboard.ts`).
    pub skk_mode: bool,
    /// Bell action when the terminal receives `BEL` (`0x07`). One of
    /// `Sound`, `Visual`, `None`. The native-poc renderer does not yet
    /// implement either side, so this field is captured for forward
    /// compatibility only (see `Settings::default`).
    #[allow(dead_code)]
    pub bell_action: BellAction,
    /// Whether to auto-detect URLs in the terminal grid and underline
    /// them on hover. Consumed by `crate::links::find_link_at` (hover
    /// underline) and the Ctrl+click open path in `window_host`.
    pub url_detection: bool,
    /// Whether to auto-detect file paths in the terminal grid and
    /// underline them on hover. Consumed alongside `url_detection`.
    pub file_path_detection: bool,
    /// Whether the prompt-folding affordance is enabled. Seeds each tab's
    /// `FoldManager` at construction (see `Tab::spawn_shell`); when `false`,
    /// fold clicks are no-ops and no region is collapsed, but C→D / custom
    /// region registration still runs.
    pub fold_enabled: bool,
    /// Editor command template (e.g. `code --goto {file}:{line}:{col}`).
    /// Consumed by the Ctrl+click file-path open path in `window_host`
    /// via `crate::links::build_editor_command`.
    pub editor_command: String,
    /// Master switch for desktop notifications. When `false`, no OS
    /// notification is dispatched for any activity type (the tab
    /// activity dot is governed separately by `tab_activity_indicator`).
    pub notification_enabled: bool,
    /// Whether inactive tabs show a dot indicator when they accumulate
    /// unread activity (output / bell / process exit).
    pub tab_activity_indicator: bool,
    /// Notify when the shell process in an inactive tab exits.
    pub notify_on_process_exit: bool,
    /// Notify when an inactive tab produces new output.
    pub notify_on_output: bool,
    /// Notify when an inactive tab receives BEL (`0x07`).
    pub notify_on_bell: bool,
    /// task0007: desktop notification for a blocked/done agent-status
    /// transition on a pane the user is not looking at (FR9). Read at
    /// notification-fire time (no restart needed on change) alongside the
    /// master [`Settings::notification_enabled`] switch.
    pub agent_status_notifications: bool,
    /// task0001 (agent-desktop-notification): per-event-type toggle for
    /// the "turn ended" (done) agent-status notification. Read alongside
    /// [`Settings::agent_status_notifications`] and
    /// [`Settings::notification_enabled`] by the notification gate
    /// (`crate::notifications::should_fire_agent_notification`).
    pub agent_notify_on_done: bool,
    /// task0001 (agent-desktop-notification): per-event-type toggle for
    /// the "waiting for input" (blocked) agent-status notification. Same
    /// gating role as [`Settings::agent_notify_on_done`].
    pub agent_notify_on_blocked: bool,
    /// task0001 (active-window-agent-notification): whether a
    /// blocked/done agent-status transition on a *visible* pane may fire
    /// a desktop notification. Default `true`. Read alongside the other
    /// agent-status gates by
    /// [`crate::notifications::should_fire_agent_notification`], which
    /// treats the pane-visibility conjunct as "not visible OR this
    /// setting is on" — every other gate still applies.
    pub agent_notify_visible_pane: bool,
    /// UI language: `Auto` (OS locale), `En`, or `Ja`. Resolved to a
    /// concrete [`crate::i18n::Locale`] once at startup
    /// (`App::with_settings`); `Auto` consults the system locale and
    /// falls back to English for unsupported languages.
    pub language: Language,
    /// Whether WARN/ERROR log lines are appended to `emterm.log`
    /// (release builds only). The file path matches the legacy Tauri
    /// build's `app_log_dir()` so both binaries share a single log
    /// file during the native-poc transition.
    pub log_recording_enabled: bool,

    // ── Markdown viewer (Phase 1) ──
    /// When `true`, the Markdown viewer follows the UI chrome theme
    /// (`ui_theme` / `ui_theme_preset`). When `false`, it uses the
    /// dedicated `markdown_theme` / `markdown_theme_preset`. Mirrors the
    /// WebView build's `markdown_theme_follow_ui`.
    pub markdown_theme_follow_ui: bool,
    /// Brightness mode for the Markdown viewer when `follow_ui = false`.
    pub markdown_theme: UiTheme,
    /// Accent preset for the Markdown viewer when `follow_ui = false`.
    pub markdown_theme_preset: UiThemePreset,
    /// Body font family for the Markdown viewer. Empty → CSS fallback chain.
    pub markdown_body_font_family: String,
    /// Code font family for the Markdown viewer. Empty → CSS fallback chain.
    pub markdown_code_font_family: String,
    /// Base font size (pt) for the Markdown viewer.
    pub markdown_font_size: u32,

    // ── Profiles / SSH / SFTP ──
    /// Terminal profiles (per-tab shell / SSH / WSL spawn presets).
    /// Resolved into spawn overrides by [`crate::profiles::resolve_spawn`];
    /// the `is_default` profile is applied by the `new_tab` keybind
    /// (`new_tab_global` always uses the global settings).
    pub profiles: Vec<app_settings::Profile>,
    /// Path to the ssh(1) binary used to launch SSH profiles. Empty means
    /// "not configured" — SSH profiles fail with a logged error, matching
    /// the WebView's alert.
    pub ssh_command_path: String,
    /// Saved SSH connections, referenced by `Profile::ssh_connection_name`.
    pub ssh_connections: Vec<app_settings::SshConnection>,
    /// Maximum concurrent SFTP uploads. Applied to the in-process upload
    /// pool by `App::with_settings` at startup and `App::apply_settings` on
    /// reload (see `crate::sftp::service::SftpService`).
    pub sftp_max_concurrent_uploads: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ambiguous_width_mode: AmbiguousWidthMode::default(),
            font_engine: FontEngine::default(),
            font_family_fallback: Vec::new(),
            variable_font_axes: std::collections::HashMap::new(),
            scrollback_lines: DEFAULT_SCROLLBACK_LINES,
            image_memory_quota_mb: DEFAULT_IMAGE_MEMORY_QUOTA_MB,
            clipboard_read_osc52: true,
            clipboard_max_size_osc52: DEFAULT_CLIPBOARD_MAX_SIZE_OSC52,
            mux_prefix_key: DEFAULT_MUX_PREFIX_KEY.to_string(),
            mux: MuxSettings::default(),
            statusbar: StatusBarSettings::default(),
            keybinds: KeybindSettings::default(),
            ime: ImeSettings::default(),
            font_size: DEFAULT_FONT_SIZE_PT,
            padding: DEFAULT_PADDING_PX,
            cursor_style: CursorStyle::default(),
            cursor_blink: true,
            ui_theme: UiTheme::default(),
            ui_theme_preset: UiThemePreset::default(),
            ui_font_family: String::new(),
            terminal_color_scheme: String::new(),
            custom_color_schemes: Vec::new(),
            show_scrollbar: ScrollbarMode::default(),
            show_tab_bar: true,
            bold_brightens_ansi_colors: true,
            shell_path: String::new(),
            shell_args: Vec::new(),
            scroll_speed: 3,
            alternate_scroll_enabled: true,
            copy_on_select: false,
            middle_click_paste: true,
            shift_enter_behavior: ShiftEnterBehavior::AltEnter,
            skk_mode: true,
            bell_action: BellAction::default(),
            url_detection: true,
            file_path_detection: true,
            fold_enabled: true,
            editor_command: "code --goto {file}:{line}:{col}".to_string(),
            notification_enabled: true,
            tab_activity_indicator: true,
            notify_on_process_exit: true,
            notify_on_output: false,
            notify_on_bell: true,
            agent_status_notifications: true,
            agent_notify_on_done: true,
            agent_notify_on_blocked: true,
            agent_notify_visible_pane: true,
            language: Language::default(),
            log_recording_enabled: false,
            markdown_theme_follow_ui: true,
            markdown_theme: UiTheme::default(),
            markdown_theme_preset: UiThemePreset::default(),
            markdown_body_font_family: String::new(),
            markdown_code_font_family: String::new(),
            markdown_font_size: DEFAULT_MARKDOWN_FONT_SIZE,
            profiles: Vec::new(),
            ssh_command_path: String::new(),
            ssh_connections: Vec::new(),
            sftp_max_concurrent_uploads: DEFAULT_SFTP_MAX_CONCURRENT_UPLOADS,
        }
    }
}

impl Settings {
    pub fn new() -> Self {
        Self::default()
    }

    /// Terminal cell font size translated from logical points
    /// (`self.font_size`) into the CSS-compatible pixel size the
    /// rasterizer + cell metrics consume. See [`PT_TO_PX`] for the
    /// conversion rationale.
    pub fn font_size_px(&self) -> f32 {
        self.font_size * PT_TO_PX
    }

    /// Resolve the effective Markdown viewer appearance.
    ///
    /// When `markdown_theme_follow_ui` is `true`, the theme/preset come
    /// from the UI chrome (`ui_theme` / `ui_theme_preset`); otherwise from
    /// the dedicated `markdown_theme` / `markdown_theme_preset`. Fonts and
    /// size always come from the `markdown_*` keys (the WebView build has
    /// no UI-chrome equivalents to inherit).
    pub fn markdown_appearance(&self) -> MarkdownAppearance {
        let (theme, preset) = if self.markdown_theme_follow_ui {
            (self.ui_theme, self.ui_theme_preset)
        } else {
            (self.markdown_theme, self.markdown_theme_preset)
        };
        MarkdownAppearance {
            theme,
            preset,
            body_font_family: self.markdown_body_font_family.clone(),
            code_font_family: self.markdown_code_font_family.clone(),
            font_size: self.markdown_font_size,
        }
    }

    /// Load `settings.json` from the platform config dir; fall back to
    /// [`Settings::default`] on any read / parse failure (logged at warn).
    ///
    /// The path is intentionally identical to the legacy Tauri WebView
    /// build's `AppHandle::path().app_config_dir()` so a single
    /// `settings.json` is shared by both binaries during the native-poc
    /// transition:
    /// - Linux:   `$XDG_CONFIG_HOME/net.laser5.app.emterm/settings.json`
    ///            (default: `$HOME/.config/...`)
    /// - Windows: `%APPDATA%\net.laser5.app.emterm\settings.json`
    pub fn load_or_default() -> Self {
        match settings_path() {
            Some(p) => Self::load_from(&p),
            None => {
                log::warn!("settings: unable to resolve config dir; using defaults");
                Self::default()
            }
        }
    }

    /// Read and merge a specific `settings.json` path into a fresh
    /// [`Settings::default`]. Missing file → defaults (logged at info).
    /// Unreadable / unparseable file → defaults (logged at warn).
    pub fn load_from(path: &std::path::Path) -> Self {
        let mut base = Self::default();
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                log::info!("settings: {} not found; using defaults", path.display());
                return base;
            }
            Err(e) => {
                log::warn!(
                    "settings: failed to read {}: {}; using defaults",
                    path.display(),
                    e
                );
                return base;
            }
        };
        match serde_json::from_slice::<RawSettings>(&bytes) {
            Ok(raw) => {
                raw.merge_into(&mut base);
                log::info!("settings: loaded {}", path.display());
            }
            Err(e) => {
                log::warn!(
                    "settings: failed to parse {}: {}; using defaults",
                    path.display(),
                    e
                );
            }
        }
        base
    }
}

// `settings_path` is defined in `crate::settings_core` and re-exported
// at the top of this module.

#[cfg(test)]
mod tests;
