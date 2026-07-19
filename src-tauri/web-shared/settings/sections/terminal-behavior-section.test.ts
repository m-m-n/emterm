/**
 * Tests for the Terminal Behavior section's Shift+Enter behavior select.
 *
 * Covers:
 * - AC-2: with current value != kitty_csi_u, renders exactly the three
 *   D2-ordered options (alt_enter, none, lf) with the current value
 *   selected.
 * - AC-3: with current value == kitty_csi_u, renders four options
 *   including kitty_csi_u, grandfathered in and selected.
 * - AC-4: selecting the LF option saves `shift_enter_behavior` as "lf".
 */

import { describe, expect, test } from "bun:test";

import { renderTerminalBehaviorSection } from "./terminal-behavior-section.ts";
import type { SectionContext } from "./types";
import type { AppSettings, KeybindSettings, MuxSettings } from "../types";

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
    keybinds: {},
    statusbar: {
      enabled: true,
      left: "",
      right: "",
      commands: {},
    },
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

function makeCtx(
  settings: AppSettings,
  saveSetting: SectionContext["saveSetting"],
): SectionContext {
  return {
    currentSettings: settings,
    muxActionDefaults: [],
    addContentListener: () => {},
    saveSetting,
    showFontPicker: () => {},
    keybindCtx: {} as unknown as SectionContext["keybindCtx"],
    reRender: () => {},
  };
}

/** Opens the MD3 select dropdown identified by `id` inside `panel`. */
function openSelect(panel: HTMLElement, id: string): HTMLElement {
  const root = panel.querySelector(`#${id}`) as HTMLElement;
  expect(root).toBeTruthy();
  const trigger = root.querySelector(
    ".md3-select-trigger",
  ) as HTMLButtonElement;
  expect(trigger).toBeTruthy();
  trigger.click();
  return root;
}

describe("renderTerminalBehaviorSection() — Shift+Enter behavior select", () => {
  test("AC-2: with current value != kitty_csi_u, renders exactly the three D2-ordered options with the current value selected", () => {
    const panel = document.createElement("div");
    const settings = makeSettings({ shift_enter_behavior: "none" });
    const ctx = makeCtx(settings, () => {});

    renderTerminalBehaviorSection(panel, ctx);

    const root = openSelect(panel, "settings-shift-enter-behavior");
    const items = Array.from(
      root.querySelectorAll(".md3-select-item"),
    ) as HTMLElement[];

    expect(items.map((item) => item.dataset.value)).toEqual([
      "alt_enter",
      "none",
      "lf",
    ]);

    const selected = items.filter(
      (item) => item.getAttribute("aria-selected") === "true",
    );
    expect(selected.length).toBe(1);
    expect(selected[0]!.dataset.value).toBe("none");
  });

  test("AC-3: with current value == kitty_csi_u, renders four options including kitty_csi_u selected", () => {
    const panel = document.createElement("div");
    const settings = makeSettings({ shift_enter_behavior: "kitty_csi_u" });
    const ctx = makeCtx(settings, () => {});

    renderTerminalBehaviorSection(panel, ctx);

    const root = openSelect(panel, "settings-shift-enter-behavior");
    const items = Array.from(
      root.querySelectorAll(".md3-select-item"),
    ) as HTMLElement[];

    expect(items.map((item) => item.dataset.value)).toEqual([
      "alt_enter",
      "none",
      "lf",
      "kitty_csi_u",
    ]);

    const selected = items.filter(
      (item) => item.getAttribute("aria-selected") === "true",
    );
    expect(selected.length).toBe(1);
    expect(selected[0]!.dataset.value).toBe("kitty_csi_u");
  });

  test("AC-4: selecting the LF option saves shift_enter_behavior as lf", () => {
    const panel = document.createElement("div");
    const settings = makeSettings({ shift_enter_behavior: "alt_enter" });
    const saved: Array<[string, unknown]> = [];
    const ctx = makeCtx(settings, (key, value) => {
      saved.push([key, value]);
    });

    renderTerminalBehaviorSection(panel, ctx);

    const root = openSelect(panel, "settings-shift-enter-behavior");
    const lfItem = root.querySelector(
      '.md3-select-item[data-value="lf"]',
    ) as HTMLElement;
    expect(lfItem).toBeTruthy();
    lfItem.click();

    expect(saved).toEqual([["shift_enter_behavior", "lf"]]);
  });

  test("choosing kitty_csi_u still saves shift_enter_behavior with the wire value", () => {
    const panel = document.createElement("div");
    const settings = makeSettings({ shift_enter_behavior: "kitty_csi_u" });
    const saved: Array<[string, unknown]> = [];
    const ctx = makeCtx(settings, (key, value) => {
      saved.push([key, value]);
    });

    renderTerminalBehaviorSection(panel, ctx);

    const root = openSelect(panel, "settings-shift-enter-behavior");
    const noneItem = root.querySelector(
      '.md3-select-item[data-value="none"]',
    ) as HTMLElement;
    expect(noneItem).toBeTruthy();
    noneItem.click();

    expect(saved).toEqual([["shift_enter_behavior", "none"]]);
  });

  test("does not render the legacy Shift+Enter as Alt+Enter toggle", () => {
    const panel = document.createElement("div");
    const settings = makeSettings();
    const ctx = makeCtx(settings, () => {});

    renderTerminalBehaviorSection(panel, ctx);

    expect(
      panel.querySelector("#settings-shift-enter-as-alt-enter"),
    ).toBeNull();
  });
});
