import {
  applyCursorStyle,
  applyCursorBlink,
  applyFoldEnabled,
} from "../settings-applier";
import type { CursorStyle, BellAction } from "../types";
import { MIN_SCROLL_SPEED, MAX_SCROLL_SPEED } from "../types";
import { t } from "../../i18n/index.ts";
import { isLinux } from "../../platform";
import {
  renderSubsectionHeader,
  renderTextInput,
  renderSelect,
  renderToggle,
  renderSlider,
} from "../settings-components";
import type { SectionContext } from "./types";

export function renderTerminalBehaviorSection(
  panel: HTMLElement,
  ctx: SectionContext,
): void {
  const settings = ctx.currentSettings;

  const header = document.createElement("h2");
  header.className = "settings-section-header";
  header.textContent = t("settings.terminalBehavior.title");
  panel.appendChild(header);

  // -- Cursor subsection --
  renderSubsectionHeader(panel, t("settings.terminal.cursor"));

  // Cursor Style (select)
  renderSelect(
    panel,
    {
      key: "cursor-style",
      label: t("settings.terminal.cursorStyle"),
      value: settings.cursor_style,
      options: [
        { value: "block", label: t("settings.terminal.cursorBlock") },
        { value: "underline", label: t("settings.terminal.cursorUnderline") },
        { value: "bar", label: t("settings.terminal.cursorBar") },
      ],
      description: t("settings.terminal.cursorStyleDesc"),
      onSave: (v) => {
        applyCursorStyle(v as CursorStyle);
        ctx.saveSetting("cursor_style", v as CursorStyle);
      },
    },
    ctx.addContentListener,
  );

  // Cursor Blink (toggle)
  renderToggle(
    panel,
    {
      key: "cursor-blink",
      label: t("settings.terminal.cursorBlink"),
      value: settings.cursor_blink,
      description: t("settings.terminal.cursorBlinkDesc"),
      onSave: (v) => {
        applyCursorBlink(v);
        ctx.saveSetting("cursor_blink", v);
      },
    },
    ctx.addContentListener,
  );

  // -- Shell subsection --
  renderSubsectionHeader(panel, t("settings.terminal.shell"));

  // Shell Path (text input)
  renderTextInput(
    panel,
    {
      key: "shell-path",
      label: t("settings.terminal.shellPath"),
      value: settings.shell_path,
      placeholder: t("settings.terminal.shellPathPlaceholder"),
      hint: t("settings.terminal.shellPathHint"),
      description: t("settings.terminal.shellPathDesc"),
      onSave: (v) => ctx.saveSetting("shell_path", v),
    },
    ctx.addContentListener,
  );

  // Shell Arguments (text input, comma-separated)
  renderTextInput(
    panel,
    {
      key: "shell-args",
      label: t("settings.terminal.shellArgs"),
      value: settings.shell_args.join(", "),
      placeholder: t("settings.terminal.shellArgsPlaceholder"),
      hint: t("settings.terminal.shellArgsHint"),
      description: t("settings.terminal.shellArgsDesc"),
      onSave: (v) => {
        const args = v
          ? v
              .split(",")
              .map((s) => s.trim())
              .filter(Boolean)
          : [];
        ctx.saveSetting("shell_args", args);
      },
    },
    ctx.addContentListener,
  );

  // -- Behavior subsection --
  renderSubsectionHeader(panel, t("settings.terminal.behavior"));

  // Scroll Speed (slider)
  renderSlider(
    panel,
    {
      key: "scroll-speed",
      label: t("settings.terminal.scrollSpeed"),
      value: settings.scroll_speed,
      min: MIN_SCROLL_SPEED,
      max: MAX_SCROLL_SPEED,
      step: 1,
      hint: t("settings.terminal.scrollSpeedHint", {
        min: MIN_SCROLL_SPEED,
        max: MAX_SCROLL_SPEED,
      }),
      description: t("settings.terminal.scrollSpeedDesc"),
      onInput: () => {},
      onSave: (v) => ctx.saveSetting("scroll_speed", v),
    },
    ctx.addContentListener,
  );

  // Alternate Scroll (DECSET 1007) toggle
  renderToggle(
    panel,
    {
      key: "alternate-scroll-enabled",
      label: t("settings.terminal.alternateScrollEnabled"),
      value: settings.alternate_scroll_enabled,
      description: t("settings.terminal.alternateScrollEnabledDesc"),
      onSave: (v) => ctx.saveSetting("alternate_scroll_enabled", v),
    },
    ctx.addContentListener,
  );

  // Bell Action (select)
  renderSelect(
    panel,
    {
      key: "bell-action",
      label: t("settings.terminal.bellAction"),
      value: settings.bell_action,
      options: [
        { value: "visual", label: t("settings.terminal.bellVisual") },
        { value: "sound", label: t("settings.terminal.bellSound") },
        { value: "none", label: t("settings.terminal.bellNone") },
      ],
      description: t("settings.terminal.bellActionDesc"),
      onSave: (v) => ctx.saveSetting("bell_action", v as BellAction),
    },
    ctx.addContentListener,
  );

  // URL Detection (toggle)
  renderToggle(
    panel,
    {
      key: "url-detection",
      label: t("settings.terminal.urlDetection"),
      value: settings.url_detection,
      description: t("settings.terminal.urlDetectionDesc"),
      onSave: (v) => ctx.saveSetting("url_detection", v),
    },
    ctx.addContentListener,
  );

  // File Path Detection (toggle)
  renderToggle(
    panel,
    {
      key: "file-path-detection",
      label: t("settings.terminal.filePathDetection"),
      value: settings.file_path_detection,
      description: t("settings.terminal.filePathDetectionDesc"),
      onSave: (v) => ctx.saveSetting("file_path_detection", v),
    },
    ctx.addContentListener,
  );

  // Editor Command (text input)
  renderTextInput(
    panel,
    {
      key: "editor-command",
      label: t("settings.terminal.editorCommand"),
      value: settings.editor_command,
      placeholder: t("settings.terminal.editorCommandPlaceholder"),
      hint: t("settings.terminal.editorCommandHint"),
      description: t("settings.terminal.editorCommandDesc"),
      onSave: (v) => ctx.saveSetting("editor_command", v),
    },
    ctx.addContentListener,
  );

  // Copy on Select / Middle Click Paste toggles.
  //
  // On Linux, these two settings are force-overridden to the native
  // Linux defaults (copy_on_select = false, middle_click_paste = true)
  // because the PRIMARY selection is used for select-to-copy and
  // middle-click-paste independently of the Ctrl+C/Ctrl+V CLIPBOARD.
  // See src/settings/effective-settings.ts. To avoid offering toggles
  // that cannot change the actual behavior, both rows are omitted from
  // the settings UI on Linux.
  if (!isLinux()) {
    // Copy on Select (toggle)
    renderToggle(
      panel,
      {
        key: "copy-on-select",
        label: t("settings.terminal.copyOnSelect"),
        value: settings.copy_on_select,
        description: t("settings.terminal.copyOnSelectDesc"),
        onSave: (v) => ctx.saveSetting("copy_on_select", v),
      },
      ctx.addContentListener,
    );

    // Middle Click Paste (toggle)
    renderToggle(
      panel,
      {
        key: "middle-click-paste",
        label: t("settings.terminal.middleClickPaste"),
        value: settings.middle_click_paste,
        description: t("settings.terminal.middleClickPasteDesc"),
        onSave: (v) => ctx.saveSetting("middle_click_paste", v),
      },
      ctx.addContentListener,
    );
  }

  // Shift+Enter as Alt+Enter (toggle)
  renderToggle(
    panel,
    {
      key: "shift-enter-as-alt-enter",
      label: t("settings.terminal.shiftEnterAsAltEnter"),
      value: settings.shift_enter_as_alt_enter,
      description: t("settings.terminal.shiftEnterAsAltEnterDesc"),
      onSave: (v) => ctx.saveSetting("shift_enter_as_alt_enter", v),
    },
    ctx.addContentListener,
  );

  // SKK Mode (toggle)
  renderToggle(
    panel,
    {
      key: "skk-mode",
      label: t("settings.terminal.skkMode"),
      value: settings.skk_mode,
      description: t("settings.terminal.skkModeDesc"),
      onSave: (v) => ctx.saveSetting("skk_mode", v),
    },
    ctx.addContentListener,
  );

  // Fold Enabled (toggle)
  renderToggle(
    panel,
    {
      key: "fold-enabled",
      label: t("settings.terminal.foldEnabled"),
      value: settings.fold_enabled,
      description: t("settings.terminal.foldEnabledDesc"),
      onSave: (v) => {
        applyFoldEnabled(v);
        ctx.saveSetting("fold_enabled", v);
      },
    },
    ctx.addContentListener,
  );

  // -- Clipboard subsection --
  renderSubsectionHeader(panel, t("settings.terminal.clipboard"));

  // OSC 52 Clipboard Read (toggle)
  renderToggle(
    panel,
    {
      key: "clipboard-read-osc52",
      label: t("settings.terminal.clipboardReadOsc52"),
      value: settings.clipboard_read_osc52,
      description: t("settings.terminal.clipboardReadOsc52Desc"),
      onSave: (v) => ctx.saveSetting("clipboard_read_osc52", v),
    },
    ctx.addContentListener,
  );

  // OSC 52 Clipboard Max Size (slider)
  renderSlider(
    panel,
    {
      key: "clipboard-max-size-osc52",
      label: t("settings.terminal.clipboardMaxSizeOsc52"),
      value: settings.clipboard_max_size_osc52 / (1024 * 1024),
      min: 1,
      max: 50,
      step: 1,
      hint: t("settings.terminal.clipboardMaxSizeOsc52Hint"),
      description: t("settings.terminal.clipboardMaxSizeOsc52Desc"),
      onInput: () => {},
      onSave: (v) =>
        ctx.saveSetting("clipboard_max_size_osc52", v * 1024 * 1024),
    },
    ctx.addContentListener,
  );
}
