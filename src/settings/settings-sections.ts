/**
 * Settings Sections
 *
 * Section renderers for the 5 settings categories:
 * - UI Settings: language, theme, UI font
 * - Keybinds: keyboard shortcuts
 * - Terminal Appearance: fonts, colors, layout
 * - Terminal Behavior: cursor, shell, scrolling
 * - Markdown Viewer: fonts for Markdown content
 */

import { invoke } from "@tauri-apps/api/core";
import {
  applyFontSize,
  applyFontFamily,
  applyUiTheme,
  applyPadding,
  applyScrollbar,
  applyCursorStyle,
  applyCursorBlink,
  applyUiFont,
  applyMarkdownSettings,
  applyMarkdownColorTheme,
  applyBoldBrightensAnsiColors,
  applyFoldEnabled,
} from "./settings-applier";
import type {
  AppSettings,
  UiTheme,
  UiThemePreset,
  CursorStyle,
  BellAction,
  ScrollbarMode,
  Language,
  FontCategory,
} from "./types";
import {
  MIN_FONT_SIZE,
  MAX_FONT_SIZE,
  MIN_PADDING,
  MAX_PADDING,
  MIN_SCROLLBACK_LINES,
  MAX_SCROLLBACK_LINES,
  MIN_SCROLL_SPEED,
  MAX_SCROLL_SPEED,
} from "./types";
import { t, setLocale, resolveLocale } from "../i18n/index.ts";
import {
  renderSubsectionHeader,
  renderNumberInput,
  renderTextInput,
  renderSelect,
  renderToggle,
  renderSlider,
} from "./settings-components";
import type { AddListenerFn } from "./settings-components";
import { renderFontPickerInput } from "./font-picker";
import { renderKeybindInput } from "./keybind-editor";
import type { KeybindEditorContext } from "./keybind-editor";
import { renderColorSchemeEditor } from "./color-scheme-editor";

// ============================================================
// Section Context
// ============================================================

export interface SectionContext {
  currentSettings: AppSettings;
  addContentListener: AddListenerFn;
  saveSetting: <K extends keyof AppSettings>(
    key: K,
    value: AppSettings[K],
  ) => void;
  showFontPicker: (
    category: FontCategory,
    currentValue: string,
    onSelect: (value: string) => void,
  ) => void;
  keybindCtx: KeybindEditorContext;
  reRender: () => void;
}

// ============================================================
// UI Settings Section
// ============================================================

export function renderUiSection(
  panel: HTMLElement,
  ctx: SectionContext,
): void {
  const settings = ctx.currentSettings;

  const header = document.createElement("h2");
  header.className = "settings-section-header";
  header.textContent = t("settings.ui.title");
  panel.appendChild(header);

  // -- Language --
  renderSelect(
    panel,
    {
      key: "language",
      label: t("settings.language.label"),
      value: settings.language,
      options: [
        { value: "auto", label: t("settings.language.auto") },
        { value: "en", label: t("settings.language.en") },
        { value: "ja", label: t("settings.language.ja") },
      ],
      description: t("settings.language.labelDesc"),
      onSave: (v) => {
        ctx.saveSetting("language", v as Language);
        // Apply language change
        const resolved = resolveLocale(v);
        setLocale(resolved);
        invoke("set_language", { language: resolved }).catch((err) => {
          console.warn("Failed to sync backend language:", err);
        });
        // Re-render settings panel with new language
        ctx.reRender();
      },
    },
    ctx.addContentListener,
  );

  // -- Theme subsection --
  renderSubsectionHeader(panel, t("settings.ui.theme"));

  // UI Theme (select)
  renderSelect(
    panel,
    {
      key: "ui-theme",
      label: t("settings.appearance.uiTheme"),
      value: settings.ui_theme,
      options: [
        { value: "system", label: t("settings.appearance.uiThemeSystem") },
        { value: "light", label: t("settings.appearance.uiThemeLight") },
        { value: "dark", label: t("settings.appearance.uiThemeDark") },
      ],
      description: t("settings.appearance.uiThemeDesc"),
      onSave: (v) => {
        ctx.currentSettings.ui_theme = v as UiTheme;
        applyUiTheme(v as UiTheme, ctx.currentSettings.ui_theme_preset);
        ctx.saveSetting("ui_theme", v as UiTheme);
        if (ctx.currentSettings.markdown_theme_follow_ui) {
          applyMarkdownColorTheme({
            followUi: true,
            mdTheme: ctx.currentSettings.markdown_theme,
            mdPreset: ctx.currentSettings.markdown_theme_preset,
            uiTheme: v as UiTheme,
            uiPreset: ctx.currentSettings.ui_theme_preset,
          });
        }
      },
    },
    ctx.addContentListener,
  );

  // UI Theme Preset (select)
  renderSelect(
    panel,
    {
      key: "ui-theme-preset",
      label: t("settings.appearance.uiThemePreset"),
      value: settings.ui_theme_preset,
      options: [
        { value: "purple", label: t("settings.appearance.presetPurple") },
        { value: "blue", label: t("settings.appearance.presetBlue") },
        { value: "green", label: t("settings.appearance.presetGreen") },
        { value: "orange", label: t("settings.appearance.presetOrange") },
        { value: "pink", label: t("settings.appearance.presetPink") },
      ],
      description: t("settings.appearance.uiThemePresetDesc"),
      onSave: (v) => {
        ctx.currentSettings.ui_theme_preset = v as UiThemePreset;
        applyUiTheme(ctx.currentSettings.ui_theme, v as UiThemePreset);
        ctx.saveSetting("ui_theme_preset", v as UiThemePreset);
        if (ctx.currentSettings.markdown_theme_follow_ui) {
          applyMarkdownColorTheme({
            followUi: true,
            mdTheme: ctx.currentSettings.markdown_theme,
            mdPreset: ctx.currentSettings.markdown_theme_preset,
            uiTheme: ctx.currentSettings.ui_theme,
            uiPreset: v as UiThemePreset,
          });
        }
      },
    },
    ctx.addContentListener,
  );

  // -- UI Font subsection --
  renderSubsectionHeader(panel, t("settings.ui.fontSection"));

  // UI Font Family (font picker)
  renderFontPickerInput(
    panel,
    {
      key: "ui-font-family",
      label: t("settings.ui.fontFamily"),
      value: settings.ui_font_family,
      placeholder: "Roboto",
      hint: t("settings.ui.fontFamilyHint"),
      description: t("settings.ui.fontFamilyDesc"),
      category: "ui",
      onSelect: (v) => {
        ctx.currentSettings.ui_font_family = v;
        applyUiFont(v);
        ctx.saveSetting("ui_font_family", v);
      },
    },
    ctx.addContentListener,
    (category, currentValue, onSelect) => {
      ctx.showFontPicker(category, currentValue, onSelect);
    },
  );
}

// ============================================================
// Terminal Appearance Section
// ============================================================

export function renderTerminalAppearanceSection(
  panel: HTMLElement,
  ctx: SectionContext,
): void {
  const settings = ctx.currentSettings;

  const header = document.createElement("h2");
  header.className = "settings-section-header";
  header.textContent = t("settings.terminalAppearance.title");
  panel.appendChild(header);

  // -- Font subsection --
  renderSubsectionHeader(panel, t("settings.appearance.font"));

  // Font Size (number input)
  renderNumberInput(
    panel,
    {
      key: "font-size",
      label: t("settings.appearance.fontSize"),
      value: settings.font_size,
      min: MIN_FONT_SIZE,
      max: MAX_FONT_SIZE,
      step: 1,
      unit: "pt",
      hint: t("settings.appearance.fontSizeHint", {
        min: MIN_FONT_SIZE,
        max: MAX_FONT_SIZE,
      }),
      description: t("settings.appearance.fontSizeDesc"),
      onInput: (v) => applyFontSize(v),
      onSave: (v) => ctx.saveSetting("font_size", v),
    },
    ctx.addContentListener,
  );

  // Primary Font (font picker)
  renderFontPickerInput(
    panel,
    {
      key: "font-family-primary",
      label: t("settings.appearance.fontFamilyPrimary"),
      value: settings.font_family_primary,
      placeholder: t("settings.appearance.fontFamilyPrimaryPlaceholder"),
      hint: t("settings.appearance.fontFamilyPrimaryHint"),
      description: t("settings.appearance.fontFamilyPrimaryDesc"),
      category: "primary",
      onSelect: (v) => {
        ctx.currentSettings.font_family_primary = v;
        applyCurrentFontFamily(ctx.currentSettings);
        ctx.saveSetting("font_family_primary", v);
      },
    },
    ctx.addContentListener,
    (category, currentValue, onSelect) => {
      ctx.showFontPicker(category, currentValue, onSelect);
    },
  );

  // Secondary Font (font picker)
  renderFontPickerInput(
    panel,
    {
      key: "font-family-secondary",
      label: t("settings.appearance.fontFamilySecondary"),
      value: settings.font_family_secondary,
      placeholder: t("settings.appearance.fontFamilySecondaryPlaceholder"),
      hint: t("settings.appearance.fontFamilySecondaryHint"),
      description: t("settings.appearance.fontFamilySecondaryDesc"),
      category: "secondary",
      onSelect: (v) => {
        ctx.currentSettings.font_family_secondary = v;
        applyCurrentFontFamily(ctx.currentSettings);
        ctx.saveSetting("font_family_secondary", v);
      },
    },
    ctx.addContentListener,
    (category, currentValue, onSelect) => {
      ctx.showFontPicker(category, currentValue, onSelect);
    },
  );

  // Emoji Font (font picker)
  renderFontPickerInput(
    panel,
    {
      key: "font-family-emoji",
      label: t("settings.appearance.fontFamilyEmoji"),
      value: settings.font_family_emoji,
      placeholder: t("settings.appearance.fontFamilyEmojiPlaceholder"),
      hint: t("settings.appearance.fontFamilyEmojiHint"),
      description: t("settings.appearance.fontFamilyEmojiDesc"),
      category: "emoji",
      onSelect: (v) => {
        ctx.currentSettings.font_family_emoji = v;
        applyCurrentFontFamily(ctx.currentSettings);
        ctx.saveSetting("font_family_emoji", v);
      },
    },
    ctx.addContentListener,
    (category, currentValue, onSelect) => {
      ctx.showFontPicker(category, currentValue, onSelect);
    },
  );

  // -- Color subsection --
  renderSubsectionHeader(panel, t("settings.terminalAppearance.color"));

  // Terminal Color Scheme (with inline palette editor)
  renderColorSchemeEditor(panel, ctx);

  // Bold Brightens ANSI Colors (toggle)
  renderToggle(
    panel,
    {
      key: "bold-brightens-ansi-colors",
      label: t("settings.terminal.boldBrightensAnsiColors"),
      value: settings.bold_brightens_ansi_colors,
      description: t("settings.terminal.boldBrightensAnsiColorsDesc"),
      onSave: (v) => {
        applyBoldBrightensAnsiColors(v);
        ctx.saveSetting("bold_brightens_ansi_colors", v);
      },
    },
    ctx.addContentListener,
  );

  // -- Layout subsection --
  renderSubsectionHeader(panel, t("settings.appearance.layout"));

  // Padding (number input)
  renderNumberInput(
    panel,
    {
      key: "padding",
      label: t("settings.appearance.padding"),
      value: settings.padding,
      min: MIN_PADDING,
      max: MAX_PADDING,
      step: 1,
      unit: "px",
      hint: t("settings.appearance.paddingHint", {
        min: MIN_PADDING,
        max: MAX_PADDING,
      }),
      description: t("settings.appearance.paddingDesc"),
      onInput: (v) => applyPadding(v),
      onSave: (v) => ctx.saveSetting("padding", v),
    },
    ctx.addContentListener,
  );

  // Scrollback Lines (number input)
  renderNumberInput(
    panel,
    {
      key: "scrollback-lines",
      label: t("settings.appearance.scrollbackLines"),
      value: settings.scrollback_lines,
      min: MIN_SCROLLBACK_LINES,
      max: MAX_SCROLLBACK_LINES,
      step: 1000,
      unit: "",
      hint: t("settings.appearance.scrollbackLinesHint", {
        min: MIN_SCROLLBACK_LINES,
        max: MAX_SCROLLBACK_LINES,
      }),
      description: t("settings.appearance.scrollbackLinesDesc"),
      onInput: () => {},
      onSave: (v) => ctx.saveSetting("scrollback_lines", v),
    },
    ctx.addContentListener,
  );

  // Show Scrollbar (select)
  renderSelect(
    panel,
    {
      key: "show-scrollbar",
      label: t("settings.appearance.showScrollbar"),
      value: settings.show_scrollbar,
      options: [
        { value: "auto", label: t("settings.appearance.scrollbarAuto") },
        { value: "always", label: t("settings.appearance.scrollbarAlways") },
        { value: "never", label: t("settings.appearance.scrollbarNever") },
      ],
      description: t("settings.appearance.showScrollbarDesc"),
      onSave: (v) => {
        applyScrollbar(v as ScrollbarMode);
        ctx.saveSetting("show_scrollbar", v as ScrollbarMode);
      },
    },
    ctx.addContentListener,
  );
}

// ============================================================
// Terminal Behavior Section
// ============================================================

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
      onSave: (v) => { applyFoldEnabled(v); ctx.saveSetting("fold_enabled", v); },
    },
    ctx.addContentListener,
  );
}

// ============================================================
// Keybinds Section
// ============================================================

export function renderKeybindsSection(
  panel: HTMLElement,
  ctx: SectionContext,
): void {
  const kb = ctx.currentSettings.keybinds;

  const header = document.createElement("h2");
  header.className = "settings-section-header";
  header.textContent = t("settings.keybinds.title");
  panel.appendChild(header);

  // -- Basic subsection --
  renderSubsectionHeader(panel, t("settings.keybinds.basic"));
  const basicGrid = createKeybindGrid(panel);
  renderKeybindInput(
    basicGrid,
    "copy",
    t("settings.keybinds.copy"),
    kb.copy,
    ctx.addContentListener,
    ctx.keybindCtx,
  );
  renderKeybindInput(
    basicGrid,
    "paste",
    t("settings.keybinds.paste"),
    kb.paste,
    ctx.addContentListener,
    ctx.keybindCtx,
  );
  // -- Tab Management subsection --
  renderSubsectionHeader(panel, t("settings.keybinds.tabManagement"));
  const tabGrid = createKeybindGrid(panel);
  renderKeybindInput(
    tabGrid,
    "new_tab",
    t("settings.keybinds.newTab"),
    kb.new_tab,
    ctx.addContentListener,
    ctx.keybindCtx,
  );
  renderKeybindInput(
    tabGrid,
    "close_tab",
    t("settings.keybinds.closeTab"),
    kb.close_tab,
    ctx.addContentListener,
    ctx.keybindCtx,
  );
  renderKeybindInput(
    tabGrid,
    "next_tab",
    t("settings.keybinds.nextTab"),
    kb.next_tab,
    ctx.addContentListener,
    ctx.keybindCtx,
  );
  renderKeybindInput(
    tabGrid,
    "prev_tab",
    t("settings.keybinds.prevTab"),
    kb.prev_tab,
    ctx.addContentListener,
    ctx.keybindCtx,
  );

  // -- Settings subsection --
  renderSubsectionHeader(panel, t("settings.keybinds.settingsSection"));
  const settingsGrid = createKeybindGrid(panel);
  renderKeybindInput(
    settingsGrid,
    "open_settings",
    t("settings.keybinds.openSettings"),
    kb.open_settings,
    ctx.addContentListener,
    ctx.keybindCtx,
  );
}

/**
 * Creates a keybind grid container and appends it to the panel
 */
function createKeybindGrid(panel: HTMLElement): HTMLElement {
  const grid = document.createElement("div");
  grid.className = "settings-keybind-grid";
  panel.appendChild(grid);
  return grid;
}

// ============================================================
// Notification Section
// ============================================================

export function renderNotificationSection(
  panel: HTMLElement,
  ctx: SectionContext,
): void {
  const settings = ctx.currentSettings;

  const header = document.createElement("h2");
  header.className = "settings-section-header";
  header.textContent = t("settings.notification.title");
  panel.appendChild(header);

  // -- General subsection --
  renderSubsectionHeader(panel, t("settings.notification.general"));

  // Desktop Notifications (toggle)
  renderToggle(
    panel,
    {
      key: "notification-enabled",
      label: t("settings.notification.notificationEnabled"),
      value: settings.notification_enabled,
      description: t("settings.notification.notificationEnabledDesc"),
      onSave: (v) => ctx.saveSetting("notification_enabled", v),
    },
    ctx.addContentListener,
  );

  // Tab Activity Indicator (toggle)
  renderToggle(
    panel,
    {
      key: "tab-activity-indicator",
      label: t("settings.notification.tabActivityIndicator"),
      value: settings.tab_activity_indicator,
      description: t("settings.notification.tabActivityIndicatorDesc"),
      onSave: (v) => ctx.saveSetting("tab_activity_indicator", v),
    },
    ctx.addContentListener,
  );

  // -- Triggers subsection --
  renderSubsectionHeader(panel, t("settings.notification.triggers"));

  // Notify on Process Exit (toggle)
  renderToggle(
    panel,
    {
      key: "notify-on-process-exit",
      label: t("settings.notification.notifyOnProcessExit"),
      value: settings.notify_on_process_exit,
      description: t("settings.notification.notifyOnProcessExitDesc"),
      onSave: (v) => ctx.saveSetting("notify_on_process_exit", v),
    },
    ctx.addContentListener,
  );

  // Notify on Output (toggle)
  renderToggle(
    panel,
    {
      key: "notify-on-output",
      label: t("settings.notification.notifyOnOutput"),
      value: settings.notify_on_output,
      description: t("settings.notification.notifyOnOutputDesc"),
      onSave: (v) => ctx.saveSetting("notify_on_output", v),
    },
    ctx.addContentListener,
  );

  // Notify on Bell (toggle)
  renderToggle(
    panel,
    {
      key: "notify-on-bell",
      label: t("settings.notification.notifyOnBell"),
      value: settings.notify_on_bell,
      description: t("settings.notification.notifyOnBellDesc"),
      onSave: (v) => ctx.saveSetting("notify_on_bell", v),
    },
    ctx.addContentListener,
  );
}

// ============================================================
// Helper
// ============================================================

function applyCurrentFontFamily(settings: AppSettings): void {
  applyFontFamily(
    settings.font_family_primary,
    settings.font_family_emoji,
    settings.font_family_secondary,
  );
}

// ============================================================
// Markdown Viewer Section
// ============================================================

export function renderMarkdownViewerSection(
  panel: HTMLElement,
  ctx: SectionContext,
): void {
  const settings = ctx.currentSettings;

  const header = document.createElement("h2");
  header.className = "settings-section-header";
  header.textContent = t("settings.markdownViewer.title");
  panel.appendChild(header);

  // -- Font subsection --
  renderSubsectionHeader(panel, t("settings.markdownViewer.font"));

  // Body Font Family (font picker)
  renderFontPickerInput(
    panel,
    {
      key: "markdown-body-font-family",
      label: t("settings.markdownViewer.bodyFontFamily"),
      value: settings.markdown_body_font_family,
      placeholder: "",
      hint: t("settings.markdownViewer.bodyFontFamilyHint"),
      description: t("settings.markdownViewer.bodyFontFamilyDesc"),
      category: "markdown-body",
      onSelect: (v) => {
        ctx.currentSettings.markdown_body_font_family = v;
        applyMarkdownSettings(
          v,
          ctx.currentSettings.markdown_code_font_family,
          ctx.currentSettings.markdown_emoji_font_family,
          ctx.currentSettings.markdown_font_size,
        );
        ctx.saveSetting("markdown_body_font_family", v);
      },
    },
    ctx.addContentListener,
    (category, currentValue, onSelect) => {
      ctx.showFontPicker(category, currentValue, onSelect);
    },
  );

  // Emoji Font Family (font picker) - between body and code per SPEC.md
  renderFontPickerInput(
    panel,
    {
      key: "markdown-emoji-font-family",
      label: t("settings.markdownViewer.emojiFontFamily"),
      value: settings.markdown_emoji_font_family,
      placeholder: "",
      hint: t("settings.markdownViewer.emojiFontFamilyHint"),
      description: t("settings.markdownViewer.emojiFontFamilyDesc"),
      category: "markdown-emoji",
      onSelect: (v) => {
        ctx.currentSettings.markdown_emoji_font_family = v;
        applyMarkdownSettings(
          ctx.currentSettings.markdown_body_font_family,
          ctx.currentSettings.markdown_code_font_family,
          v,
          ctx.currentSettings.markdown_font_size,
        );
        ctx.saveSetting("markdown_emoji_font_family", v);
      },
    },
    ctx.addContentListener,
    (category, currentValue, onSelect) => {
      ctx.showFontPicker(category, currentValue, onSelect);
    },
  );

  // Code Font Family (font picker)
  renderFontPickerInput(
    panel,
    {
      key: "markdown-code-font-family",
      label: t("settings.markdownViewer.codeFontFamily"),
      value: settings.markdown_code_font_family,
      placeholder: "",
      hint: t("settings.markdownViewer.codeFontFamilyHint"),
      description: t("settings.markdownViewer.codeFontFamilyDesc"),
      category: "markdown-code",
      onSelect: (v) => {
        ctx.currentSettings.markdown_code_font_family = v;
        applyMarkdownSettings(
          ctx.currentSettings.markdown_body_font_family,
          v,
          ctx.currentSettings.markdown_emoji_font_family,
          ctx.currentSettings.markdown_font_size,
        );
        ctx.saveSetting("markdown_code_font_family", v);
      },
    },
    ctx.addContentListener,
    (category, currentValue, onSelect) => {
      ctx.showFontPicker(category, currentValue, onSelect);
    },
  );

  // Font Size (number input)
  renderNumberInput(
    panel,
    {
      key: "markdown-font-size",
      label: t("settings.markdownViewer.fontSize"),
      value: settings.markdown_font_size,
      min: MIN_FONT_SIZE,
      max: MAX_FONT_SIZE,
      step: 1,
      unit: "pt",
      hint: t("settings.markdownViewer.fontSizeHint", {
        min: MIN_FONT_SIZE,
        max: MAX_FONT_SIZE,
      }),
      description: t("settings.markdownViewer.fontSizeDesc"),
      onInput: (v) => {
        applyMarkdownSettings(
          ctx.currentSettings.markdown_body_font_family,
          ctx.currentSettings.markdown_code_font_family,
          ctx.currentSettings.markdown_emoji_font_family,
          v,
        );
      },
      onSave: (v) => ctx.saveSetting("markdown_font_size", v),
    },
    ctx.addContentListener,
  );

  // -- Color Theme subsection --
  renderSubsectionHeader(panel, t("settings.markdownViewer.colorTheme"));

  // Follow UI Theme (toggle)
  renderToggle(
    panel,
    {
      key: "markdown-theme-follow-ui",
      label: t("settings.markdownViewer.followUiTheme"),
      value: settings.markdown_theme_follow_ui,
      description: t("settings.markdownViewer.followUiThemeDesc"),
      onSave: (v) => {
        ctx.currentSettings.markdown_theme_follow_ui = v;
        applyMarkdownColorTheme({
          followUi: v,
          mdTheme: ctx.currentSettings.markdown_theme,
          mdPreset: ctx.currentSettings.markdown_theme_preset,
          uiTheme: ctx.currentSettings.ui_theme,
          uiPreset: ctx.currentSettings.ui_theme_preset,
        });
        ctx.saveSetting("markdown_theme_follow_ui", v);
        ctx.reRender();
      },
    },
    ctx.addContentListener,
  );

  // Show theme/preset selectors only when follow UI is OFF
  if (!settings.markdown_theme_follow_ui) {
    // Markdown Theme (select)
    renderSelect(
      panel,
      {
        key: "markdown-theme",
        label: t("settings.markdownViewer.theme"),
        value: settings.markdown_theme,
        options: [
          {
            value: "system",
            label: t("settings.markdownViewer.themeSystem"),
          },
          { value: "light", label: t("settings.markdownViewer.themeLight") },
          { value: "dark", label: t("settings.markdownViewer.themeDark") },
        ],
        description: t("settings.markdownViewer.themeDesc"),
        onSave: (v) => {
          ctx.currentSettings.markdown_theme = v as UiTheme;
          applyMarkdownColorTheme({
            followUi: false,
            mdTheme: v as UiTheme,
            mdPreset: ctx.currentSettings.markdown_theme_preset,
            uiTheme: ctx.currentSettings.ui_theme,
            uiPreset: ctx.currentSettings.ui_theme_preset,
          });
          ctx.saveSetting("markdown_theme", v as UiTheme);
        },
      },
      ctx.addContentListener,
    );

    // Markdown Preset (select)
    renderSelect(
      panel,
      {
        key: "markdown-theme-preset",
        label: t("settings.markdownViewer.preset"),
        value: settings.markdown_theme_preset,
        options: [
          {
            value: "purple",
            label: t("settings.appearance.presetPurple"),
          },
          { value: "blue", label: t("settings.appearance.presetBlue") },
          { value: "green", label: t("settings.appearance.presetGreen") },
          {
            value: "orange",
            label: t("settings.appearance.presetOrange"),
          },
          { value: "pink", label: t("settings.appearance.presetPink") },
        ],
        description: t("settings.markdownViewer.presetDesc"),
        onSave: (v) => {
          ctx.currentSettings.markdown_theme_preset = v as UiThemePreset;
          applyMarkdownColorTheme({
            followUi: false,
            mdTheme: ctx.currentSettings.markdown_theme,
            mdPreset: v as UiThemePreset,
            uiTheme: ctx.currentSettings.ui_theme,
            uiPreset: ctx.currentSettings.ui_theme_preset,
          });
          ctx.saveSetting("markdown_theme_preset", v as UiThemePreset);
        },
      },
      ctx.addContentListener,
    );
  }
}
