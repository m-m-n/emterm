/**
 * Tests for the SettingsPanel wiring of the new "agent" category
 * (task0002).
 *
 * Covers AC-1: the categories list includes an enabled "agent" entry with
 * a translated (non-key) label, the nav renders a registered (non-empty)
 * icon for it, and selecting it (the content render branch) renders the
 * agent section.
 *
 * SettingsPanel has no prior test coverage in this codebase (init() calls
 * the backend IPC via SettingsService.load(), which these tests avoid).
 * The category list, icon map, and content-render branch are all private
 * to the class, so these tests reach them directly on the instance —
 * TypeScript's `private` is a compile-time-only restriction, and this file
 * is excluded from `bun run typecheck`'s scope like other *.test.ts files.
 * The constructor and the exercised private methods (renderNavigation,
 * renderContent) do not perform any IPC, so no backend mocking is needed.
 */

import { describe, expect, test } from "bun:test";

import { SettingsPanel } from "./settings-panel.ts";
import type { AppSettings, KeybindSettings, MuxSettings } from "./types";

interface SettingsPanelInternals {
  categories: Array<{ id: string; label: string; enabled: boolean }>;
  navElement: HTMLElement | null;
  contentElement: HTMLElement | null;
  currentSettings: AppSettings | null;
  activeCategory: string;
  renderNavigation(): void;
  renderContent(): void;
}

function internals(panel: SettingsPanel): SettingsPanelInternals {
  return panel as unknown as SettingsPanelInternals;
}

function makeKeybinds(): KeybindSettings {
  return {
    copy: "",
    paste: "",
    select_all: "",
    search: "",
    new_tab: "",
    new_tab_global: "",
    close_tab: "",
    next_tab: "",
    prev_tab: "",
    zoom_in: "",
    zoom_out: "",
    zoom_reset: "",
    toggle_fullscreen: "",
    open_settings: "",
    toggle_tab_bar: "",
    jump_to_prev_prompt: "",
    jump_to_next_prompt: "",
    profile_selector: "",
  };
}

function makeMux(): MuxSettings {
  return {
    prefix: "",
    tab_always_expand: false,
    tmux_conf_imported: false,
    window_sidebar_overlay: false,
    keybinds: {},
  };
}

function makeSettings(overrides: Partial<AppSettings> = {}): AppSettings {
  return {
    font_size: 13,
    font_family_primary: "",
    font_family_secondary: "",
    ui_theme: "system",
    ui_theme_preset: "purple",
    terminal_color_scheme: "default",
    padding: 4,
    scrollback_lines: 10000,
    show_scrollbar: "auto",
    show_tab_bar: true,
    shell_path: "",
    shell_args: [],
    cursor_style: "block",
    cursor_blink: true,
    scroll_speed: 3,
    alternate_scroll_enabled: true,
    bell_action: "visual",
    url_detection: true,
    copy_on_select: false,
    fold_enabled: false,
    file_path_detection: true,
    bold_brightens_ansi_colors: false,
    middle_click_paste: true,
    shift_enter_behavior: "alt_enter",
    editor_command: "",
    skk_mode: true,
    notification_enabled: true,
    tab_activity_indicator: true,
    notify_on_process_exit: true,
    notify_on_output: false,
    notify_on_bell: true,
    agent_status_notifications: true,
    agent_notify_on_done: true,
    agent_notify_on_blocked: true,
    agent_notify_visible_pane: true,
    keybinds: makeKeybinds(),
    language: "auto",
    ui_font_family: "",
    custom_color_schemes: [],
    profiles: [],
    markdown_theme_follow_ui: true,
    markdown_theme: "system",
    markdown_theme_preset: "purple",
    markdown_body_font_family: "",
    markdown_code_font_family: "",
    markdown_font_size: 14,
    ssh_command_path: "",
    ssh_connections: [],
    sftp_max_concurrent_uploads: 4,
    clipboard_read_osc52: false,
    clipboard_max_size_osc52: 1024 * 1024,
    log_recording_enabled: false,
    mux: makeMux(),
    statusbar_enabled: true,
    statusbar_app_line1_left: "",
    statusbar_app_line1_right: "",
    statusbar_app_line2_left: "",
    statusbar_app_line2_right: "",
    statusbar_time_format: "",
    statusbar_font_size: null,
    statusbar_custom_commands: {},
    statusbar_refresh_rates: {},
    ...overrides,
  };
}

describe("SettingsPanel — agent category wiring (AC-1)", () => {
  test("categories list includes an enabled 'agent' entry with a translated label", () => {
    const panel = internals(
      new SettingsPanel({ container: document.createElement("div") }),
    );

    const agent = panel.categories.find((c) => c.id === "agent");
    expect(agent).toBeTruthy();
    expect(agent!.enabled).toBe(true);
    expect(agent!.label).not.toBe("settings.categories.agent");
  });

  test("nav renders a registered (non-empty) icon for the 'agent' category", () => {
    const panel = internals(
      new SettingsPanel({ container: document.createElement("div") }),
    );
    panel.navElement = document.createElement("nav");

    panel.renderNavigation();

    const iconSpan = panel.navElement!.querySelector(
      '[data-category-id="agent"] .settings-nav-icon',
    ) as HTMLElement | null;
    expect(iconSpan).toBeTruthy();
    expect(iconSpan!.innerHTML.length).toBeGreaterThan(0);
  });

  test("selecting 'agent' renders the agent section (content render branch)", () => {
    const panel = internals(
      new SettingsPanel({ container: document.createElement("div") }),
    );
    panel.contentElement = document.createElement("main");
    panel.currentSettings = makeSettings();
    panel.activeCategory = "agent";

    panel.renderContent();

    expect(
      panel.contentElement.querySelector(
        "#settings-agent-status-notifications",
      ),
    ).toBeTruthy();
    expect(
      panel.contentElement.querySelector("#settings-agent-notify-on-done"),
    ).toBeTruthy();
    expect(
      panel.contentElement.querySelector("#settings-agent-notify-on-blocked"),
    ).toBeTruthy();
  });
});
