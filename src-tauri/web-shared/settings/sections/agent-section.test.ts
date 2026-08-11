/**
 * Tests for the Agent settings section: the agent notification master
 * toggle (moved here from Notifications) and the three per-event-type
 * toggles (turn complete / waiting for input / visible pane).
 *
 * Covers:
 * - AC-2: master -> turn complete -> waiting for input -> visible pane
 *   toggles render in that order, each reflecting the corresponding
 *   currentSettings value.
 * - AC-3: flipping each toggle saves the corresponding setting key.
 * - AC-4: the notification section no longer renders the agent master
 *   toggle (moved, not duplicated).
 * - AC-6: en/ja both resolve the category label and every label/description
 *   key used by this section to a translated (non-key) string, the rendered
 *   labels are not raw i18n keys, and the old
 *   settings.notification.agentStatusNotifications* keys resolve to
 *   nothing (moved, not duplicated).
 *
 * task0002 (active-window-agent-notification) additions:
 * - AC-2/AC-3/AC-4 above extended to the fourth "visible pane" toggle
 *   (`agent_notify_visible_pane`).
 */

import { afterEach, describe, expect, test } from "bun:test";

import { renderAgentSection } from "./agent-section.ts";
import { renderNotificationSection } from "./notification-section.ts";
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

function makeCtx(
  settings: AppSettings,
  saveSetting: SectionContext["saveSetting"],
): SectionContext {
  return {
    currentSettings: settings,
    muxActionDefaults: [],
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

describe("renderAgentSection() — toggle order and values (AC-2)", () => {
  test("renders master, turn-complete, waiting-for-input, visible-pane toggles in that order, reflecting settings", () => {
    const panel = document.createElement("div");
    const settings = makeSettings({
      agent_status_notifications: true,
      agent_notify_on_done: false,
      agent_notify_on_blocked: true,
      agent_notify_visible_pane: false,
    });
    const ctx = makeCtx(settings, () => {});

    renderAgentSection(panel, ctx);

    const toggles = Array.from(
      panel.querySelectorAll('[role="switch"]'),
    ) as HTMLButtonElement[];
    expect(toggles.map((el) => el.id)).toEqual([
      "settings-agent-status-notifications",
      "settings-agent-notify-on-done",
      "settings-agent-notify-on-blocked",
      "settings-agent-notify-visible-pane",
    ]);
    expect(toggles[0]!.getAttribute("aria-checked")).toBe("true");
    expect(toggles[1]!.getAttribute("aria-checked")).toBe("false");
    expect(toggles[2]!.getAttribute("aria-checked")).toBe("true");
    expect(toggles[3]!.getAttribute("aria-checked")).toBe("false");
  });

  test("reflects visible-pane toggle on when settings carry the default true", () => {
    const panel = document.createElement("div");
    const settings = makeSettings({ agent_notify_visible_pane: true });
    const ctx = makeCtx(settings, () => {});

    renderAgentSection(panel, ctx);

    const toggle = panel.querySelector(
      "#settings-agent-notify-visible-pane",
    ) as HTMLButtonElement;
    expect(toggle.getAttribute("aria-checked")).toBe("true");
  });

  test("reflects all-off settings", () => {
    const panel = document.createElement("div");
    const settings = makeSettings({
      agent_status_notifications: false,
      agent_notify_on_done: false,
      agent_notify_on_blocked: false,
      agent_notify_visible_pane: false,
    });
    const ctx = makeCtx(settings, () => {});

    renderAgentSection(panel, ctx);

    const toggles = Array.from(
      panel.querySelectorAll('[role="switch"]'),
    ) as HTMLButtonElement[];
    expect(
      toggles.every((el) => el.getAttribute("aria-checked") === "false"),
    ).toBe(true);
  });
});

describe("renderAgentSection() — save on toggle (AC-3)", () => {
  test("flipping the master toggle saves agent_status_notifications", () => {
    const panel = document.createElement("div");
    const settings = makeSettings({ agent_status_notifications: false });
    const saved: Array<[string, unknown]> = [];
    const ctx = makeCtx(settings, (key, value) => {
      saved.push([key, value]);
    });

    renderAgentSection(panel, ctx);
    (
      panel.querySelector(
        "#settings-agent-status-notifications",
      ) as HTMLButtonElement
    ).click();

    expect(saved).toEqual([["agent_status_notifications", true]]);
  });

  test("flipping the turn-complete toggle saves agent_notify_on_done", () => {
    const panel = document.createElement("div");
    const settings = makeSettings({ agent_notify_on_done: true });
    const saved: Array<[string, unknown]> = [];
    const ctx = makeCtx(settings, (key, value) => {
      saved.push([key, value]);
    });

    renderAgentSection(panel, ctx);
    (
      panel.querySelector("#settings-agent-notify-on-done") as HTMLButtonElement
    ).click();

    expect(saved).toEqual([["agent_notify_on_done", false]]);
  });

  test("flipping the waiting-for-input toggle saves agent_notify_on_blocked", () => {
    const panel = document.createElement("div");
    const settings = makeSettings({ agent_notify_on_blocked: false });
    const saved: Array<[string, unknown]> = [];
    const ctx = makeCtx(settings, (key, value) => {
      saved.push([key, value]);
    });

    renderAgentSection(panel, ctx);
    (
      panel.querySelector(
        "#settings-agent-notify-on-blocked",
      ) as HTMLButtonElement
    ).click();

    expect(saved).toEqual([["agent_notify_on_blocked", true]]);
  });

  test("flipping the visible-pane toggle saves agent_notify_visible_pane", () => {
    const panel = document.createElement("div");
    const settings = makeSettings({ agent_notify_visible_pane: true });
    const saved: Array<[string, unknown]> = [];
    const ctx = makeCtx(settings, (key, value) => {
      saved.push([key, value]);
    });

    renderAgentSection(panel, ctx);
    (
      panel.querySelector(
        "#settings-agent-notify-visible-pane",
      ) as HTMLButtonElement
    ).click();

    expect(saved).toEqual([["agent_notify_visible_pane", false]]);
  });
});

describe("renderNotificationSection() — agent toggle moved out (AC-4)", () => {
  test("does not render the agent status notifications toggle", () => {
    const panel = document.createElement("div");
    const settings = makeSettings();
    const ctx = makeCtx(settings, () => {});

    renderNotificationSection(panel, ctx);

    expect(
      panel.querySelector("#settings-agent-status-notifications"),
    ).toBeNull();
  });
});

describe("renderAgentSection() — i18n (AC-6)", () => {
  test("ja and en both resolve the category label", () => {
    for (const locale of ["en", "ja"] as const) {
      setLocale(locale);
      expect(t("settings.categories.agent")).not.toBe(
        "settings.categories.agent",
      );
    }
  });

  test("ja and en both resolve every label/description key used by the section", () => {
    const keys = [
      "settings.agent.title",
      "settings.agent.master",
      "settings.agent.masterDesc",
      "settings.agent.notifyOnDone",
      "settings.agent.notifyOnDoneDesc",
      "settings.agent.notifyOnBlocked",
      "settings.agent.notifyOnBlockedDesc",
      "settings.agent.notifyVisiblePane",
      "settings.agent.notifyVisiblePaneDesc",
    ];
    for (const locale of ["en", "ja"] as const) {
      setLocale(locale);
      for (const key of keys) {
        expect(t(key)).not.toBe(key);
      }
    }
  });

  test("the rendered labels are translated text, not the raw i18n key (en)", () => {
    setLocale("en");
    const panel = document.createElement("div");
    const settings = makeSettings();
    const ctx = makeCtx(settings, () => {});

    renderAgentSection(panel, ctx);

    const labels = Array.from(panel.querySelectorAll(".settings-label")).map(
      (el) => el.textContent,
    );
    expect(labels).toEqual([
      t("settings.agent.master"),
      t("settings.agent.notifyOnDone"),
      t("settings.agent.notifyOnBlocked"),
      t("settings.agent.notifyVisiblePane"),
    ]);
    expect(labels).not.toContain("settings.agent.master");
  });

  test("the old settings.notification.agentStatusNotifications* keys resolve to nothing in en/ja", () => {
    for (const locale of ["en", "ja"] as const) {
      setLocale(locale);
      expect(t("settings.notification.agentStatusNotifications")).toBe(
        "settings.notification.agentStatusNotifications",
      );
      expect(t("settings.notification.agentStatusNotificationsDesc")).toBe(
        "settings.notification.agentStatusNotificationsDesc",
      );
    }
  });
});
