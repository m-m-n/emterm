/**
 * Regression tests for the Profiles list drag reorder (settings panel).
 *
 * On Linux (WebKitGTK) a drag only starts when the dragstart handler puts a
 * drag-data entry on the event's DataTransfer. These tests render the
 * Profiles section, dispatch bubbling drag events on the rendered list items
 * and assert on the DataTransfer and on the reorder outcome.
 *
 * Covers:
 * - AC-1: dragstart on the item at index i leaves the DataTransfer returning
 *   String(i) for `application/x-emterm-profile-index`, effectAllowed "move".
 * - AC-2: that entry is the only one on the DataTransfer (no `text/plain`).
 * - AC-3: dragstart on index 0 + drop on index 2 reorders [A, B, C] into
 *   [B, C, A], saves once under "profiles" and re-renders.
 * - AC-4: a drop without a preceding dragstart on the list does not save,
 *   even when the drop's DataTransfer carries the list's custom MIME type.
 * - AC-5: dragstart + drop on the same item does not save.
 */

import { describe, expect, test } from "bun:test";

import { initI18n, t } from "../../i18n/index.ts";
import enLocale from "../../i18n/locales/en.json";
import jaLocale from "../../i18n/locales/ja.json";
import { renderProfilesSection } from "./profiles-section.ts";
import type { SectionContext } from "./types";
import type {
  AppSettings,
  KeybindSettings,
  MuxSettings,
  Profile,
} from "../types";

const PROFILE_INDEX_MIME = "application/x-emterm-profile-index";

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

function makeProfile(name: string, isDefault = false): Profile {
  return {
    name,
    shell_path: "",
    shell_args: [],
    env_vars: "",
    working_directory: "",
    is_default: isDefault,
    ssh_connection_name: "",
    wsl_distro_name: "",
  };
}

function makeSettings(profiles: Profile[]): AppSettings {
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
    scroll_region_scrollback_enabled: true,
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
    profiles,
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
  };
}

interface RenderedProfiles {
  panel: HTMLElement;
  settings: AppSettings;
  saves: Array<[string, unknown]>;
  reRenders: { count: number };
  item: (index: number) => HTMLElement;
}

/**
 * Render a fresh Profiles section (so the section's in-memory drag index
 * never leaks between tests). The stub context attaches listeners to the
 * element it is given and records every saveSetting / reRender call.
 */
function renderProfiles(
  names: string[],
  defaultNames: string[] = [],
): RenderedProfiles {
  const settings = makeSettings(
    names.map((name) => makeProfile(name, defaultNames.includes(name))),
  );
  const saves: Array<[string, unknown]> = [];
  const reRenders = { count: 0 };
  const ctx: SectionContext = {
    currentSettings: settings,
    muxActionDefaults: [],
    addContentListener: (el, ev, handler, capture) =>
      el.addEventListener(ev, handler, capture),
    saveSetting: (key, value) => {
      saves.push([key, value]);
    },
    showFontPicker: () => {},
    keybindCtx: {} as unknown as SectionContext["keybindCtx"],
    reRender: () => {
      reRenders.count += 1;
    },
  };

  const panel = document.createElement("div");
  renderProfilesSection(panel, ctx);

  return {
    panel,
    settings,
    saves,
    reRenders,
    item: (index: number) => {
      const el = panel.querySelector(
        `.profile-list-item[data-index="${index}"]`,
      );
      if (!el) throw new Error(`no profile list item at index ${index}`);
      return el as HTMLElement;
    },
  };
}

/** A real (happy-dom) DataTransfer: records entries in insertion order. */
function makeDataTransfer(): DataTransfer {
  const Ctor = (
    globalThis.window as unknown as { DataTransfer: new () => DataTransfer }
  ).DataTransfer;
  return new Ctor();
}

/**
 * Dispatch a bubbling, cancelable drag event on `target`. happy-dom's
 * DragEvent does not carry the init `dataTransfer`, so it is attached to a
 * plain Event.
 */
function dispatchDrag(
  target: HTMLElement,
  type: "dragstart" | "dragover" | "dragend" | "drop",
  dataTransfer: DataTransfer | null,
): void {
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperty(event, "dataTransfer", { value: dataTransfer });
  target.dispatchEvent(event);
}

function names(profiles: Profile[]): string[] {
  return profiles.map((p) => p.name);
}

describe("Profiles list drag reorder — dragstart drag data", () => {
  test("AC-1: dragstart on item 1 sets its index under the profile MIME type with effectAllowed move", () => {
    const { item } = renderProfiles(["A", "B", "C"]);
    const dt = makeDataTransfer();

    dispatchDrag(item(1), "dragstart", dt);

    expect(dt.getData(PROFILE_INDEX_MIME)).toBe("1");
    expect(dt.effectAllowed).toBe("move");
  });

  test("AC-2: the profile MIME entry is the only one on the DataTransfer (no text/plain)", () => {
    const { item } = renderProfiles(["A", "B", "C"]);
    const dt = makeDataTransfer();

    dispatchDrag(item(1), "dragstart", dt);

    expect([...dt.types]).toEqual([PROFILE_INDEX_MIME]);
    expect(dt.getData("text/plain")).toBe("");
  });
});

describe("Profiles list drag reorder — dragstart then drop", () => {
  test("AC-3: dragstart on index 0 + drop on index 2 reorders [A, B, C] into [B, C, A], saves once and re-renders", () => {
    const { settings, saves, reRenders, item } = renderProfiles([
      "A",
      "B",
      "C",
    ]);

    dispatchDrag(item(0), "dragstart", makeDataTransfer());
    dispatchDrag(item(2), "drop", makeDataTransfer());

    expect(names(settings.profiles)).toEqual(["B", "C", "A"]);
    expect(saves.length).toBe(1);
    const [key, value] = saves[0]!;
    expect(key).toBe("profiles");
    expect(names(value as Profile[])).toEqual(["B", "C", "A"]);
    expect(reRenders.count).toBeGreaterThanOrEqual(1);
  });

  test("AC-4: a drop with no preceding dragstart does not save and leaves profiles unchanged", () => {
    const { settings, saves, item } = renderProfiles(["A", "B", "C"]);

    dispatchDrag(item(2), "drop", makeDataTransfer());

    expect(saves.length).toBe(0);
    expect(names(settings.profiles)).toEqual(["A", "B", "C"]);
  });

  test("AC-4: a drop whose DataTransfer carries the profile MIME type with value 0 but has no preceding dragstart does not save", () => {
    const { settings, saves, item } = renderProfiles(["A", "B", "C"]);
    const dt = makeDataTransfer();
    dt.setData(PROFILE_INDEX_MIME, "0");

    dispatchDrag(item(2), "drop", dt);

    expect(saves.length).toBe(0);
    expect(names(settings.profiles)).toEqual(["A", "B", "C"]);
  });

  test("AC-5: dragstart on index 1 + drop on index 1 does not save and leaves profiles unchanged", () => {
    const { settings, saves, item } = renderProfiles(["A", "B", "C"]);

    dispatchDrag(item(1), "dragstart", makeDataTransfer());
    dispatchDrag(item(1), "drop", makeDataTransfer());

    expect(saves.length).toBe(0);
    expect(names(settings.profiles)).toEqual(["A", "B", "C"]);
  });
});

/** The action buttons of one rendered profile item, in DOM order. */
function actionButtons(item: HTMLElement): HTMLButtonElement[] {
  return Array.from(
    item.querySelectorAll<HTMLButtonElement>(".profile-item-actions button"),
  );
}

function buttonTexts(item: HTMLElement): string[] {
  return actionButtons(item).map((btn) => btn.textContent ?? "");
}

describe("Profiles list Launch removal — action buttons", () => {
  test("AC-1: each profile item holds exactly the default toggle, Edit, Duplicate and Delete buttons in that order", () => {
    const { panel } = renderProfiles(["A", "B"], ["A"]);
    const items = Array.from(
      panel.querySelectorAll<HTMLElement>(".profile-list-item"),
    );
    expect(items.length).toBe(2);

    const [itemA, itemB] = items as [HTMLElement, HTMLElement];
    expect(buttonTexts(itemA)).toEqual([
      t("settings.profiles.unsetDefault"),
      t("settings.profiles.edit"),
      t("settings.profiles.duplicate"),
      t("settings.profiles.delete"),
    ]);
    expect(buttonTexts(itemB)).toEqual([
      t("settings.profiles.setDefault"),
      t("settings.profiles.edit"),
      t("settings.profiles.duplicate"),
      t("settings.profiles.delete"),
    ]);
  });

  test("AC-1: no action button reads Launch or 起動 in either UI language", () => {
    try {
      for (const locale of ["en", "ja"]) {
        initI18n(locale);
        const { panel } = renderProfiles(["A", "B"], ["A"]);
        const texts = Array.from(
          panel.querySelectorAll<HTMLButtonElement>(
            ".profile-item-actions button",
          ),
        ).map((btn) => btn.textContent);

        expect(texts).not.toContain("Launch");
        expect(texts).not.toContain("起動");
        // Guard: the buttons were found (4 per profile, 2 profiles).
        expect(texts.length).toBe(8);
      }
    } finally {
      initI18n("en");
    }
  });

  test("AC-2: clicking every action button of a profile never dispatches profile:launch on the document", () => {
    const { item } = renderProfiles(["A"]);
    const buttons = actionButtons(item(0));
    expect(buttons.length).toBeGreaterThan(0);

    let launchEvents = 0;
    const onLaunch = () => {
      launchEvents += 1;
    };
    document.addEventListener("profile:launch", onLaunch);
    // Bun's native CustomEvent is rejected by happy-dom's
    // document.dispatchEvent, so a CustomEvent dispatch would throw before
    // reaching any listener and this test could not detect it. Use the
    // window's CustomEvent (what a browser provides) for the duration of
    // the test.
    const nativeCustomEvent = globalThis.CustomEvent;
    globalThis.CustomEvent = (
      globalThis.window as unknown as { CustomEvent: typeof CustomEvent }
    ).CustomEvent;
    // Clicking Edit opens the profile editor overlay on document.body; keep
    // track of what was already there so it can be removed afterwards.
    const bodyChildrenBefore = new Set(Array.from(document.body.children));
    try {
      for (const btn of buttons) btn.click();
    } finally {
      globalThis.CustomEvent = nativeCustomEvent;
      document.removeEventListener("profile:launch", onLaunch);
      for (const child of Array.from(document.body.children)) {
        if (!bodyChildrenBefore.has(child)) child.remove();
      }
    }

    expect(launchEvents).toBe(0);
  });
});

describe("Profiles list Launch removal — locale files", () => {
  test.each([
    ["en", enLocale],
    ["ja", jaLocale],
  ] as const)("AC-3: %s.json settings.profiles has no launch property", (_name, locale) => {
    const profiles = (locale as { settings: Record<string, unknown> }).settings
      .profiles as Record<string, unknown>;

    // Guard: the object itself must still exist with its other entries.
    expect(typeof profiles).toBe("object");
    expect(profiles).toHaveProperty("edit");
    expect(Object.hasOwn(profiles, "launch")).toBe(false);
  });
});
