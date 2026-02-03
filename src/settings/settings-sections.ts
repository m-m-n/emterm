/**
 * Settings Sections
 *
 * Section renderers for Appearance, Terminal, and Keybinds categories.
 * Each function renders its section into the provided panel element.
 */

import { invoke } from "@tauri-apps/api/core";
import {
  applyFontSize,
  applyFontFamily,
  applyLineHeight,
  applyUiTheme,
  applyTerminalColorScheme,
  applyPadding,
  applyScrollbar,
  applyCursorStyle,
  applyCursorBlink,
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
  MIN_LINE_HEIGHT,
  MAX_LINE_HEIGHT,
  LINE_HEIGHT_STEP,
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
import {
  buildSelectOptions,
  getSchemeColors,
  isUserScheme,
  createUserSchemeFromPreset,
  updateUserSchemeColor,
  deleteUserScheme,
  duplicateScheme,
  renameUserScheme,
  type ColorKey,
} from "./color-scheme-editor";
import { validateHexColor } from "../terminal/colors";

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
// Appearance Section
// ============================================================

export function renderAppearanceSection(
  panel: HTMLElement,
  ctx: SectionContext,
): void {
  const settings = ctx.currentSettings;

  const header = document.createElement("h2");
  header.className = "settings-section-header";
  header.textContent = t("settings.appearance.title");
  panel.appendChild(header);

  // -- Language (no subsection header to avoid label duplication) --
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

  // Line Height (number input)
  renderNumberInput(
    panel,
    {
      key: "line-height",
      label: t("settings.appearance.lineHeight"),
      value: settings.line_height,
      min: MIN_LINE_HEIGHT,
      max: MAX_LINE_HEIGHT,
      step: LINE_HEIGHT_STEP,
      unit: "",
      hint: t("settings.appearance.lineHeightHint", {
        min: MIN_LINE_HEIGHT,
        max: MAX_LINE_HEIGHT,
      }),
      description: t("settings.appearance.lineHeightDesc"),
      onInput: (v) => applyLineHeight(v),
      onSave: (v) => ctx.saveSetting("line_height", v),
    },
    ctx.addContentListener,
  );

  // -- Theme & Color subsection --
  renderSubsectionHeader(panel, t("settings.appearance.themeColor"));

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
        applyUiTheme(v as UiTheme, ctx.currentSettings.ui_theme_preset);
        ctx.saveSetting("ui_theme", v as UiTheme);
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
      ],
      description: t("settings.appearance.uiThemePresetDesc"),
      onSave: (v) => {
        applyUiTheme(ctx.currentSettings.ui_theme, v as UiThemePreset);
        ctx.saveSetting("ui_theme_preset", v as UiThemePreset);
      },
    },
    ctx.addContentListener,
  );

  // Terminal Color Scheme (with inline palette editor)
  renderColorSchemeEditor(panel, ctx);

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
// Terminal Section
// ============================================================

export function renderTerminalSection(
  panel: HTMLElement,
  ctx: SectionContext,
): void {
  const settings = ctx.currentSettings;

  const header = document.createElement("h2");
  header.className = "settings-section-header";
  header.textContent = t("settings.terminal.title");
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
  renderKeybindInput(
    panel,
    "copy",
    t("settings.keybinds.copy"),
    kb.copy,
    ctx.addContentListener,
    ctx.keybindCtx,
  );
  renderKeybindInput(
    panel,
    "paste",
    t("settings.keybinds.paste"),
    kb.paste,
    ctx.addContentListener,
    ctx.keybindCtx,
  );
  renderKeybindInput(
    panel,
    "select_all",
    t("settings.keybinds.selectAll"),
    kb.select_all,
    ctx.addContentListener,
    ctx.keybindCtx,
  );
  renderKeybindInput(
    panel,
    "search",
    t("settings.keybinds.search"),
    kb.search,
    ctx.addContentListener,
    ctx.keybindCtx,
  );

  // -- Tab Management subsection --
  renderSubsectionHeader(panel, t("settings.keybinds.tabManagement"));
  renderKeybindInput(
    panel,
    "new_tab",
    t("settings.keybinds.newTab"),
    kb.new_tab,
    ctx.addContentListener,
    ctx.keybindCtx,
  );
  renderKeybindInput(
    panel,
    "close_tab",
    t("settings.keybinds.closeTab"),
    kb.close_tab,
    ctx.addContentListener,
    ctx.keybindCtx,
  );
  renderKeybindInput(
    panel,
    "next_tab",
    t("settings.keybinds.nextTab"),
    kb.next_tab,
    ctx.addContentListener,
    ctx.keybindCtx,
  );
  renderKeybindInput(
    panel,
    "prev_tab",
    t("settings.keybinds.prevTab"),
    kb.prev_tab,
    ctx.addContentListener,
    ctx.keybindCtx,
  );

  // -- Display subsection --
  renderSubsectionHeader(panel, t("settings.keybinds.display"));
  renderKeybindInput(
    panel,
    "zoom_in",
    t("settings.keybinds.zoomIn"),
    kb.zoom_in,
    ctx.addContentListener,
    ctx.keybindCtx,
  );
  renderKeybindInput(
    panel,
    "zoom_out",
    t("settings.keybinds.zoomOut"),
    kb.zoom_out,
    ctx.addContentListener,
    ctx.keybindCtx,
  );
  renderKeybindInput(
    panel,
    "zoom_reset",
    t("settings.keybinds.zoomReset"),
    kb.zoom_reset,
    ctx.addContentListener,
    ctx.keybindCtx,
  );
  renderKeybindInput(
    panel,
    "toggle_fullscreen",
    t("settings.keybinds.toggleFullscreen"),
    kb.toggle_fullscreen,
    ctx.addContentListener,
    ctx.keybindCtx,
  );

  // -- Settings subsection --
  renderSubsectionHeader(panel, t("settings.keybinds.settingsSection"));
  renderKeybindInput(
    panel,
    "open_settings",
    t("settings.keybinds.openSettings"),
    kb.open_settings,
    ctx.addContentListener,
    ctx.keybindCtx,
  );
}

// ============================================================
// Color Scheme Editor
// ============================================================

/** Debounce timer for color changes */
let colorSaveTimer: ReturnType<typeof setTimeout> | null = null;

function renderColorSchemeEditor(
  panel: HTMLElement,
  ctx: SectionContext,
): void {
  const settings = ctx.currentSettings;
  const currentScheme = settings.terminal_color_scheme || "emterm";

  // Container
  const container = document.createElement("div");
  container.className = "settings-row";
  container.dataset.key = "terminal-color-scheme-editor";
  panel.appendChild(container);

  // Label and description
  const labelDiv = document.createElement("div");
  labelDiv.className = "settings-label";
  labelDiv.innerHTML = `
    <span class="label-text">${t("settings.appearance.colorScheme")}</span>
    <span class="label-desc">${t("settings.appearance.colorSchemeDesc")}</span>
  `;
  container.appendChild(labelDiv);

  // Control area
  const controlDiv = document.createElement("div");
  controlDiv.className = "settings-control color-scheme-editor";
  container.appendChild(controlDiv);

  // Select box
  const select = document.createElement("select");
  select.className = "settings-select";
  const options = buildSelectOptions(settings.custom_color_schemes);
  for (const opt of options) {
    const optEl = document.createElement("option");
    optEl.value = opt.value;
    optEl.textContent = opt.label;
    if (opt.value === currentScheme) {
      optEl.selected = true;
    }
    select.appendChild(optEl);
  }
  controlDiv.appendChild(select);

  // Action buttons container
  const actionsDiv = document.createElement("div");
  actionsDiv.className = "color-scheme-actions";
  controlDiv.appendChild(actionsDiv);

  // Palette container
  const paletteDiv = document.createElement("div");
  paletteDiv.className = "color-palette-editor";
  controlDiv.appendChild(paletteDiv);

  // Render functions
  const renderPalette = () => {
    paletteDiv.innerHTML = "";
    const schemeColors = getSchemeColors(
      select.value,
      settings.custom_color_schemes,
    );
    if (!schemeColors) return;

    // Special colors section
    const specialDiv = document.createElement("div");
    specialDiv.className = "color-palette-special";
    paletteDiv.appendChild(specialDiv);

    renderColorInput(
      specialDiv,
      t("settings.appearance.colorSchemeForeground"),
      schemeColors.foreground,
      "foreground",
    );
    renderColorInput(
      specialDiv,
      t("settings.appearance.colorSchemeBackground"),
      schemeColors.background,
      "background",
    );
    renderColorInput(
      specialDiv,
      t("settings.appearance.colorSchemeCursor"),
      schemeColors.cursor,
      "cursor",
    );
    renderColorInput(
      specialDiv,
      t("settings.appearance.colorSchemeSelection"),
      schemeColors.selection,
      "selection",
    );

    // ANSI colors section
    const ansiLabel = document.createElement("div");
    ansiLabel.className = "color-palette-label";
    ansiLabel.textContent = t("settings.appearance.colorSchemeStandardColors");
    paletteDiv.appendChild(ansiLabel);

    const standardGrid = document.createElement("div");
    standardGrid.className = "color-palette-grid";
    paletteDiv.appendChild(standardGrid);
    for (let i = 0; i < 8; i++) {
      const color = schemeColors.ansi_colors[i] || "#000000";
      renderColorInput(
        standardGrid,
        String(i),
        color,
        `ansi_${i}` as ColorKey,
        true,
      );
    }

    const brightLabel = document.createElement("div");
    brightLabel.className = "color-palette-label";
    brightLabel.textContent = t("settings.appearance.colorSchemeBrightColors");
    paletteDiv.appendChild(brightLabel);

    const brightGrid = document.createElement("div");
    brightGrid.className = "color-palette-grid";
    paletteDiv.appendChild(brightGrid);
    for (let i = 8; i < 16; i++) {
      const color = schemeColors.ansi_colors[i] || "#000000";
      renderColorInput(
        brightGrid,
        String(i),
        color,
        `ansi_${i}` as ColorKey,
        true,
      );
    }
  };

  const renderActions = () => {
    actionsDiv.innerHTML = "";
    const isUser = isUserScheme(select.value, settings.custom_color_schemes);

    // Duplicate button (always visible)
    const dupBtn = document.createElement("button");
    dupBtn.className = "settings-button settings-button-secondary";
    dupBtn.textContent = t("settings.appearance.colorSchemeDuplicate");
    dupBtn.onclick = () => handleDuplicate();
    actionsDiv.appendChild(dupBtn);

    if (isUser) {
      // Delete button (user schemes only)
      const delBtn = document.createElement("button");
      delBtn.className = "settings-button settings-button-danger";
      delBtn.textContent = t("settings.appearance.colorSchemeDelete");
      delBtn.onclick = () => handleDelete();
      actionsDiv.appendChild(delBtn);

      // Rename field (user schemes only)
      const renameDiv = document.createElement("div");
      renameDiv.className = "color-scheme-rename";
      actionsDiv.appendChild(renameDiv);

      const renameLabel = document.createElement("span");
      renameLabel.textContent =
        t("settings.appearance.colorSchemeRename") + ":";
      renameDiv.appendChild(renameLabel);

      const renameInput = document.createElement("input");
      renameInput.type = "text";
      renameInput.className = "settings-input";
      renameInput.value = select.value;
      renameDiv.appendChild(renameInput);

      renameInput.onblur = () => handleRename(renameInput.value);
      renameInput.onkeydown = (e) => {
        if (e.key === "Enter") {
          renameInput.blur();
        }
      };
    }
  };

  const renderColorInput = (
    parent: HTMLElement,
    label: string,
    value: string,
    colorKey: ColorKey,
    compact = false,
  ) => {
    const row = document.createElement("div");
    row.className = compact ? "color-input-compact" : "color-input-row";

    if (!compact) {
      const labelEl = document.createElement("span");
      labelEl.className = "color-input-label";
      labelEl.textContent = label;
      row.appendChild(labelEl);
    }

    const inputGroup = document.createElement("div");
    inputGroup.className = "color-input-group";
    row.appendChild(inputGroup);

    const colorPicker = document.createElement("input");
    colorPicker.type = "color";
    colorPicker.className = "color-picker";
    colorPicker.value = value;
    colorPicker.title = compact ? label : "";
    inputGroup.appendChild(colorPicker);

    const hexInput = document.createElement("input");
    hexInput.type = "text";
    hexInput.className = "color-hex-input";
    hexInput.value = value;
    hexInput.maxLength = 7;
    inputGroup.appendChild(hexInput);

    // Store original value for change detection
    let originalValue = value.toLowerCase();

    // Use 'change' event instead of 'input' - fires only on commit (not during drag)
    colorPicker.onchange = () => {
      const newValue = colorPicker.value.toLowerCase();
      hexInput.value = newValue;
      if (newValue !== originalValue) {
        handleColorChange(colorKey, newValue);
        originalValue = newValue; // Update after successful change
      }
    };

    // Sync hex input -> color picker (on blur)
    hexInput.onblur = () => {
      const newValue = hexInput.value.toLowerCase();
      if (validateHexColor(newValue)) {
        colorPicker.value = newValue;
        if (newValue !== originalValue) {
          handleColorChange(colorKey, newValue);
          originalValue = newValue; // Update after successful change
        }
      } else {
        // Revert to picker value
        hexInput.value = colorPicker.value;
      }
    };

    parent.appendChild(row);
  };

  // Event handlers
  const handleSelectChange = () => {
    const newScheme = select.value === "emterm" ? "" : select.value;
    applyTerminalColorScheme(newScheme, settings.custom_color_schemes);
    ctx.saveSetting("terminal_color_scheme", newScheme);
    settings.terminal_color_scheme = newScheme;
    renderActions();
    renderPalette();
  };

  const handleColorChange = (colorKey: ColorKey, newValue: string) => {
    const currentSchemeName = select.value;
    const isUser = isUserScheme(
      currentSchemeName,
      settings.custom_color_schemes,
    );

    if (!isUser) {
      // Auto-copy: create user scheme from preset
      const newScheme = createUserSchemeFromPreset(
        currentSchemeName,
        settings.custom_color_schemes,
      );
      if (newScheme) {
        // Apply the color change to the new scheme
        const updated = updateUserSchemeColor(newScheme, colorKey, newValue);
        settings.custom_color_schemes = [
          ...settings.custom_color_schemes,
          updated,
        ];
        settings.terminal_color_scheme = updated.name;

        // Update select box
        refreshSelectOptions();
        select.value = updated.name;

        // Save
        debouncedSave();
        applyTerminalColorScheme(updated.name, settings.custom_color_schemes);
        renderActions();
      }
    } else {
      // Update existing user scheme
      const schemeIndex = settings.custom_color_schemes.findIndex(
        (s) => s.name === currentSchemeName,
      );
      if (schemeIndex >= 0) {
        const existingScheme = settings.custom_color_schemes[schemeIndex];
        if (existingScheme) {
          const updated = updateUserSchemeColor(
            existingScheme,
            colorKey,
            newValue,
          );
          settings.custom_color_schemes[schemeIndex] = updated;
          debouncedSave();
          applyTerminalColorScheme(
            currentSchemeName,
            settings.custom_color_schemes,
          );
        }
      }
    }
  };

  const handleDuplicate = () => {
    const newScheme = duplicateScheme(
      select.value,
      settings.custom_color_schemes,
    );
    if (newScheme) {
      settings.custom_color_schemes = [
        ...settings.custom_color_schemes,
        newScheme,
      ];
      settings.terminal_color_scheme = newScheme.name;
      refreshSelectOptions();
      select.value = newScheme.name;
      ctx.saveSetting("custom_color_schemes", settings.custom_color_schemes);
      ctx.saveSetting("terminal_color_scheme", newScheme.name);
      applyTerminalColorScheme(newScheme.name, settings.custom_color_schemes);
      renderActions();
      renderPalette();
    }
  };

  const handleDelete = () => {
    const schemeName = select.value;
    settings.custom_color_schemes = deleteUserScheme(
      settings.custom_color_schemes,
      schemeName,
    );
    settings.terminal_color_scheme = "";
    refreshSelectOptions();
    select.value = "emterm";
    ctx.saveSetting("custom_color_schemes", settings.custom_color_schemes);
    ctx.saveSetting("terminal_color_scheme", "");
    applyTerminalColorScheme("", settings.custom_color_schemes);
    renderActions();
    renderPalette();
  };

  const handleRename = (newName: string) => {
    const oldName = select.value;
    const result = renameUserScheme(
      settings.custom_color_schemes,
      oldName,
      newName,
    );
    if (result.success && result.schemes) {
      settings.custom_color_schemes = result.schemes;
      if (settings.terminal_color_scheme === oldName) {
        settings.terminal_color_scheme = newName.trim();
      }
      refreshSelectOptions();
      select.value = newName.trim();
      ctx.saveSetting("custom_color_schemes", settings.custom_color_schemes);
      ctx.saveSetting("terminal_color_scheme", settings.terminal_color_scheme);
    }
  };

  const refreshSelectOptions = () => {
    select.innerHTML = "";
    const options = buildSelectOptions(settings.custom_color_schemes);
    for (const opt of options) {
      const optEl = document.createElement("option");
      optEl.value = opt.value;
      optEl.textContent = opt.label;
      select.appendChild(optEl);
    }
  };

  const debouncedSave = () => {
    if (colorSaveTimer) {
      clearTimeout(colorSaveTimer);
    }
    colorSaveTimer = setTimeout(() => {
      ctx.saveSetting("custom_color_schemes", settings.custom_color_schemes);
      ctx.saveSetting("terminal_color_scheme", settings.terminal_color_scheme);
    }, 300);
  };

  // Wire up select change
  select.onchange = handleSelectChange;
  ctx.addContentListener(select, "change", handleSelectChange);

  // Initial render
  renderActions();
  renderPalette();
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
