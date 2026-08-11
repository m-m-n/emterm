/**
 * Tests for the Mux section's window sidebar overlay toggle and the
 * toggle-window-sidebar keybind action label.
 *
 * Covers:
 * - AC-1: renders a toggle for mux.window_sidebar_overlay reflecting the
 *   current settings value (off by default).
 * - AC-2: flipping the toggle saves the mux settings object with the field
 *   set, preserving sibling mux fields (spread-save).
 * - AC-3: ja/en locale files contain the toggle label/description keys and
 *   the toggle-window-sidebar keybind action label key; the keybind grid
 *   renders the translated action name once the backend exposes it.
 */

import { afterEach, describe, expect, test } from "bun:test";

import { renderMuxSection } from "./mux-section.ts";
import type { SectionContext } from "./types";
import { setLocale, t } from "../../i18n/index.ts";
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

function makeMux(overrides: Partial<MuxSettings> = {}): MuxSettings {
  return {
    prefix: "",
    tab_always_expand: false,
    tmux_conf_imported: false,
    window_sidebar_overlay: false,
    keybinds: {},
    ...overrides,
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

function makeCtx(
  settings: AppSettings,
  saveSetting: SectionContext["saveSetting"],
  muxActionDefaults: SectionContext["muxActionDefaults"] = [],
): SectionContext {
  return {
    currentSettings: settings,
    muxActionDefaults,
    addContentListener: (el, ev, handler, capture) =>
      el.addEventListener(ev, handler, capture),
    saveSetting,
    showFontPicker: () => {},
    keybindCtx: {} as unknown as SectionContext["keybindCtx"],
    reRender: () => {},
  };
}

afterEach(() => {
  setLocale("en");
});

describe("renderMuxSection() — window sidebar overlay toggle", () => {
  test("AC-1: renders the toggle off by default, reflecting settings", () => {
    const panel = document.createElement("div");
    const settings = makeSettings();
    const ctx = makeCtx(settings, () => {});

    renderMuxSection(panel, ctx);

    const toggle = panel.querySelector(
      "#settings-mux-window-sidebar-overlay",
    ) as HTMLButtonElement | null;
    expect(toggle).toBeTruthy();
    expect(toggle!.getAttribute("role")).toBe("switch");
    expect(toggle!.getAttribute("aria-checked")).toBe("false");
  });

  test("AC-1: reflects an on value from settings", () => {
    const panel = document.createElement("div");
    const settings = makeSettings({
      mux: makeMux({ window_sidebar_overlay: true }),
    });
    const ctx = makeCtx(settings, () => {});

    renderMuxSection(panel, ctx);

    const toggle = panel.querySelector(
      "#settings-mux-window-sidebar-overlay",
    ) as HTMLButtonElement | null;
    expect(toggle!.getAttribute("aria-checked")).toBe("true");
  });

  test("AC-2: flipping the toggle saves mux with the field set, preserving sibling mux fields", () => {
    const panel = document.createElement("div");
    const mux = makeMux({
      prefix: "Ctrl+B",
      tmux_conf_imported: true,
      keybinds: { detach: "d" },
    });
    const settings = makeSettings({ mux });
    const saved: Array<[string, unknown]> = [];
    const ctx = makeCtx(settings, (key, value) => {
      saved.push([key, value]);
    });

    renderMuxSection(panel, ctx);

    const toggle = panel.querySelector(
      "#settings-mux-window-sidebar-overlay",
    ) as HTMLButtonElement;
    toggle.click();

    expect(saved.length).toBe(1);
    const [key, value] = saved[0]!;
    expect(key).toBe("mux");
    expect(value).toEqual({
      ...mux,
      window_sidebar_overlay: true,
    });
  });
});

describe("renderMuxSection() — i18n keys", () => {
  test("AC-3: ja and en both resolve the toggle label/description keys", () => {
    for (const locale of ["en", "ja"] as const) {
      setLocale(locale);
      expect(t("settings.mux.windowSidebarOverlay")).not.toBe(
        "settings.mux.windowSidebarOverlay",
      );
      expect(t("settings.mux.windowSidebarOverlayDesc")).not.toBe(
        "settings.mux.windowSidebarOverlayDesc",
      );
    }
  });

  test("AC-3: ja and en both resolve the toggle-window-sidebar keybind action label key", () => {
    for (const locale of ["en", "ja"] as const) {
      setLocale(locale);
      expect(t("settings.mux.keybind.toggleWindowSidebar")).not.toBe(
        "settings.mux.keybind.toggleWindowSidebar",
      );
    }
  });

  test("AC-3: the keybind grid renders the translated toggle-window-sidebar label once the backend exposes the action", () => {
    setLocale("en");
    const panel = document.createElement("div");
    const settings = makeSettings();
    const ctx = makeCtx(settings, () => {}, [
      { action: "toggle-window-sidebar", key: "Ctrl+W" },
    ]);

    renderMuxSection(panel, ctx);

    const labels = Array.from(
      panel.querySelectorAll(".settings-row-keybind .settings-label"),
    ).map((el) => el.textContent);
    expect(labels).toContain(t("settings.mux.keybind.toggleWindowSidebar"));
  });
});

describe("renderMuxSection() — next-agent-window keybind row", () => {
  test("AC-1: ja and en both resolve the next-agent-window keybind label key", () => {
    for (const locale of ["en", "ja"] as const) {
      setLocale(locale);
      expect(t("settings.mux.keybind.nextAgentWindow")).not.toBe(
        "settings.mux.keybind.nextAgentWindow",
      );
    }
  });

  test("AC-1: the keybind grid renders a labeled row for next-agent-window once the backend exposes the action", () => {
    setLocale("en");
    const panel = document.createElement("div");
    const settings = makeSettings();
    const ctx = makeCtx(settings, () => {}, [
      { action: "next-agent-window", key: "Ctrl+A" },
    ]);

    renderMuxSection(panel, ctx);

    const labels = Array.from(
      panel.querySelectorAll(".settings-row-keybind .settings-label"),
    ).map((el) => el.textContent);
    expect(labels).toContain(t("settings.mux.keybind.nextAgentWindow"));
  });

  test("AC-2: the row reads settings.mux.keybinds['next-agent-window'] over the backend default", () => {
    setLocale("en");
    const panel = document.createElement("div");
    const settings = makeSettings({
      mux: makeMux({ keybinds: { "next-agent-window": "Ctrl+X" } }),
    });
    const ctx = makeCtx(settings, () => {}, [
      { action: "next-agent-window", key: "Ctrl+A" },
    ]);

    renderMuxSection(panel, ctx);

    const button = Array.from(
      panel.querySelectorAll<HTMLButtonElement>(
        ".settings-row-keybind .settings-keybind-input",
      ),
    ).find((el) => el.textContent === "Ctrl+X");
    expect(button).toBeTruthy();
  });

  test("AC-2: capturing a key writes settings.mux.keybinds['next-agent-window'] like existing mux action rows", () => {
    setLocale("en");
    const panel = document.createElement("div");
    const settings = makeSettings({
      mux: makeMux({ keybinds: { "next-agent-window": "Ctrl+A" } }),
    });
    const saved: Array<[string, unknown]> = [];
    const ctx = makeCtx(
      settings,
      (key, value) => {
        saved.push([key, value]);
      },
      [{ action: "next-agent-window", key: "Ctrl+A" }],
    );

    renderMuxSection(panel, ctx);

    const button = Array.from(
      panel.querySelectorAll<HTMLButtonElement>(
        ".settings-row-keybind .settings-keybind-input",
      ),
    ).find((el) => el.textContent === "Ctrl+A");
    expect(button).toBeTruthy();

    button!.click();
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "b", ctrlKey: true, bubbles: true }),
    );

    expect(saved.length).toBe(1);
    const [key, value] = saved[0]!;
    expect(key).toBe("mux");
    expect((value as MuxSettings).keybinds).toEqual({
      "next-agent-window": "Ctrl+B",
    });
  });
});
