/**
 * Regression tests for the SSH connection list drag reorder.
 *
 * Covers:
 * - AC-1: dragstart on a connection item puts the item's index on the
 *   DataTransfer under application/x-emterm-ssh-index, effectAllowed "move".
 * - AC-2: that entry is the only one; no text/plain entry is added.
 * - AC-3: dragstart on item 0 then drop on item 2 reorders to [B, C, A],
 *   saves once and re-renders.
 * - AC-4: a drop with no preceding dragstart (even when the DataTransfer
 *   carries the custom MIME type) saves nothing.
 * - AC-5: dragstart then drop on the same item saves nothing.
 */

import { describe, expect, test } from "bun:test";

import { renderSshSection } from "./ssh-section.ts";
import type { SectionContext } from "./types";
import type {
  AppSettings,
  KeybindSettings,
  MuxSettings,
  SshConnection,
} from "../types";

const SSH_INDEX_MIME = "application/x-emterm-ssh-index";

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

function makeConnection(name: string): SshConnection {
  return {
    name,
    hostname: `${name.toLowerCase()}.example.com`,
    port: 22,
    username: "user",
    identity_file: "",
    ssh_options: [],
  };
}

function makeSettings(connections: SshConnection[]): AppSettings {
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
    profiles: [],
    markdown_theme_follow_ui: true,
    markdown_theme: "system",
    markdown_theme_preset: "purple",
    markdown_body_font_family: "",
    markdown_code_font_family: "",
    markdown_font_size: 14,
    // Empty so the section does not request the .ssh/config host list
    // over Tauri IPC.
    ssh_command_path: "",
    ssh_connections: connections,
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

interface Harness {
  settings: AppSettings;
  panel: HTMLElement;
  saved: Array<[string, unknown]>;
  reRenderCalls: { count: number };
  items: HTMLElement[];
}

/** Renders a fresh SSH section so the in-memory drag index never leaks. */
function renderSection(connections: SshConnection[]): Harness {
  const settings = makeSettings(connections);
  const saved: Array<[string, unknown]> = [];
  const reRenderCalls = { count: 0 };
  const ctx: SectionContext = {
    currentSettings: settings,
    muxActionDefaults: [],
    addContentListener: (el, ev, handler, capture) =>
      el.addEventListener(ev, handler, capture),
    saveSetting: (key, value) => {
      saved.push([key, value]);
    },
    showFontPicker: () => {},
    keybindCtx: {} as unknown as SectionContext["keybindCtx"],
    reRender: () => {
      reRenderCalls.count++;
    },
  };

  const panel = document.createElement("div");
  renderSshSection(panel, ctx);

  const items = Array.from(
    panel.querySelectorAll<HTMLElement>(".profile-list-item[data-index]"),
  );
  return { settings, panel, saved, reRenderCalls, items };
}

function createDataTransfer(): DataTransfer {
  const Ctor = (
    globalThis.window as unknown as { DataTransfer: new () => DataTransfer }
  ).DataTransfer;
  return new Ctor();
}

/**
 * Dispatches a bubbling drag event on `target` that carries `dataTransfer`.
 * happy-dom's DragEvent ignores `dataTransfer` in its init dictionary, so the
 * property is defined on the event instance.
 */
function dispatchDrag(
  target: Element,
  type: "dragstart" | "dragover" | "dragend" | "drop",
  dataTransfer: DataTransfer = createDataTransfer(),
): DataTransfer {
  const Ctor = (
    globalThis.window as unknown as {
      DragEvent: new (type: string, init: EventInit) => Event;
    }
  ).DragEvent;
  const event = new Ctor(type, { bubbles: true, cancelable: true });
  Object.defineProperty(event, "dataTransfer", { value: dataTransfer });
  target.dispatchEvent(event);
  return dataTransfer;
}

const A = makeConnection("A");
const B = makeConnection("B");
const C = makeConnection("C");

describe("renderSshSection() - connection list drag reorder", () => {
  test("AC-1: dragstart on item 1 sets the item index under the SSH index MIME type with effectAllowed move", () => {
    const { items } = renderSection([A, B, C]);
    expect(items.length).toBe(3);

    const dt = dispatchDrag(items[1]!, "dragstart");

    expect(dt.getData(SSH_INDEX_MIME)).toBe("1");
    expect(dt.effectAllowed).toBe("move");
  });

  test("AC-2: dragstart leaves exactly one DataTransfer entry and no text/plain entry", () => {
    const { items } = renderSection([A, B, C]);

    const dt = dispatchDrag(items[1]!, "dragstart");

    expect(Array.from(dt.types)).toEqual([SSH_INDEX_MIME]);
    expect(Array.from(dt.types)).not.toContain("text/plain");
    expect(dt.getData("text/plain")).toBe("");
  });

  test("AC-3: dragstart on item 0 then drop on item 2 reorders to [B, C, A], saves once and re-renders", () => {
    const { settings, items, saved, reRenderCalls } = renderSection([A, B, C]);

    dispatchDrag(items[0]!, "dragstart");
    dispatchDrag(items[2]!, "drop");

    expect(settings.ssh_connections.map((c) => c.name)).toEqual([
      "B",
      "C",
      "A",
    ]);
    expect(settings.ssh_connections).toEqual([B, C, A]);
    expect(saved.length).toBe(1);
    const [key, value] = saved[0]!;
    expect(key).toBe("ssh_connections");
    expect(value).toEqual([B, C, A]);
    expect(reRenderCalls.count).toBe(1);
  });

  test("AC-4: a drop with no preceding dragstart saves nothing and leaves the connections unchanged", () => {
    const { settings, items, saved, reRenderCalls } = renderSection([A, B, C]);

    dispatchDrag(items[2]!, "drop");

    expect(saved.length).toBe(0);
    expect(reRenderCalls.count).toBe(0);
    expect(settings.ssh_connections).toEqual([A, B, C]);
  });

  test("AC-4: a drop carrying the SSH index MIME type but no preceding dragstart saves nothing", () => {
    const { settings, items, saved, reRenderCalls } = renderSection([A, B, C]);
    const foreignDt = createDataTransfer();
    foreignDt.setData(SSH_INDEX_MIME, "0");

    dispatchDrag(items[2]!, "drop", foreignDt);

    expect(saved.length).toBe(0);
    expect(reRenderCalls.count).toBe(0);
    expect(settings.ssh_connections).toEqual([A, B, C]);
  });

  test("AC-5: dragstart on item 1 then drop on the same item saves nothing and leaves the connections unchanged", () => {
    const { settings, items, saved, reRenderCalls } = renderSection([A, B, C]);

    dispatchDrag(items[1]!, "dragstart");
    dispatchDrag(items[1]!, "drop");

    expect(saved.length).toBe(0);
    expect(reRenderCalls.count).toBe(0);
    expect(settings.ssh_connections).toEqual([A, B, C]);
  });
});
