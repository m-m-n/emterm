/**
 * Settings Panel
 *
 * UI component for application settings with category navigation.
 */

import { SettingsService } from "./settings-service";
import {
  applySettings,
  applyFontSize,
  applyFontFamily,
  applyLineHeight,
  applyUiTheme,
  applyTerminalColorScheme,
  applyPadding,
  applyScrollbar,
  applyOpacity,
  applyCursorStyle,
  applyCursorBlink,
} from "./settings-applier";
import type {
  AppSettings, UiTheme, CursorStyle, BellAction, ScrollbarMode,
  KeybindSettings,
} from "./types";
import {
  MIN_FONT_SIZE, MAX_FONT_SIZE,
  MIN_LINE_HEIGHT, MAX_LINE_HEIGHT, LINE_HEIGHT_STEP,
  MIN_OPACITY, MAX_OPACITY, OPACITY_STEP,
  MIN_PADDING, MAX_PADDING,
  MIN_SCROLLBACK_LINES, MAX_SCROLLBACK_LINES,
  MIN_SCROLL_SPEED, MAX_SCROLL_SPEED,
} from "./types";

/**
 * Options for creating SettingsPanel
 */
export interface SettingsPanelOptions {
  /** Container element for the settings panel */
  container: HTMLElement;
}

/**
 * Category definition
 */
interface Category {
  id: string;
  label: string;
  enabled: boolean;
}

/**
 * SettingsPanel - Displays and manages application settings
 */
export class SettingsPanel {
  private container: HTMLElement;
  private navElement: HTMLElement | null = null;
  private contentElement: HTMLElement | null = null;
  private activeCategory = "appearance";
  private currentSettings: AppSettings | null = null;
  private eventListeners: Array<{
    element: EventTarget;
    type: string;
    handler: EventListener;
  }> = [];
  /** Keybind button currently in capture mode */
  private capturingKeybindButton: HTMLButtonElement | null = null;
  private capturingKeybindKey: string | null = null;
  private capturingOriginalValue: string | null = null;

  private readonly categories: Category[] = [
    { id: "appearance", label: "Appearance", enabled: true },
    { id: "terminal", label: "Terminal", enabled: true },
    { id: "keybinds", label: "Keybinds", enabled: true },
  ];

  constructor(options: SettingsPanelOptions) {
    this.container = options.container;
  }

  /**
   * Initializes the settings panel
   */
  async init(): Promise<void> {
    this.currentSettings = await SettingsService.load();
    this.render();
    this.attachEventListeners();
  }

  /**
   * Renders the settings panel UI
   */
  private render(): void {
    this.container.innerHTML = "";
    this.container.className = "settings-panel";

    // Navigation
    this.navElement = document.createElement("nav");
    this.navElement.className = "settings-nav";
    this.navElement.setAttribute("role", "tablist");
    this.navElement.setAttribute("aria-orientation", "vertical");
    this.navElement.setAttribute("aria-label", "Settings categories");
    this.renderNavigation();
    this.container.appendChild(this.navElement);

    // Content
    this.contentElement = document.createElement("main");
    this.contentElement.className = "settings-content";
    this.renderContent();
    this.container.appendChild(this.contentElement);
  }

  /**
   * Renders the category navigation with ARIA tab pattern
   */
  private renderNavigation(): void {
    if (!this.navElement) return;
    this.navElement.innerHTML = "";

    for (const category of this.categories) {
      const button = document.createElement("button");
      button.className = "settings-nav-item";
      button.textContent = category.label;
      button.dataset.categoryId = category.id;

      button.setAttribute("role", "tab");
      button.id = `tab-${category.id}`;
      button.setAttribute("aria-controls", `panel-${category.id}`);

      const isActive = category.id === this.activeCategory;
      button.setAttribute("aria-selected", String(isActive));
      button.setAttribute("tabindex", isActive ? "0" : "-1");

      if (isActive) {
        button.classList.add("active");
      }

      if (!category.enabled) {
        button.classList.add("disabled");
        button.disabled = true;
        button.setAttribute("aria-disabled", "true");
      }

      this.navElement.appendChild(button);
    }
  }

  /**
   * Renders the content area for the active category
   */
  private renderContent(): void {
    if (!this.contentElement) return;
    this.contentElement.innerHTML = "";

    const panel = document.createElement("section");
    panel.className = "settings-content-panel";
    panel.setAttribute("role", "tabpanel");
    panel.id = `panel-${this.activeCategory}`;
    panel.setAttribute("aria-labelledby", `tab-${this.activeCategory}`);
    panel.setAttribute("tabindex", "0");

    this.contentElement.appendChild(panel);

    switch (this.activeCategory) {
      case "appearance":
        this.renderAppearanceSection(panel);
        break;
      case "terminal":
        this.renderTerminalSection(panel);
        break;
      case "keybinds":
        this.renderKeybindsSection(panel);
        break;
    }
  }

  // ============================================================
  // Appearance Category
  // ============================================================

  private renderAppearanceSection(panel: HTMLElement): void {
    if (!this.currentSettings) return;

    const header = document.createElement("h2");
    header.className = "settings-section-header";
    header.textContent = "Appearance";
    panel.appendChild(header);

    // -- Font subsection --
    this.renderSubsectionHeader(panel, "Font");

    // Font Size (number input)
    this.renderNumberInput(panel, {
      key: "font-size",
      label: "Font Size",
      value: this.currentSettings.font_size,
      min: MIN_FONT_SIZE,
      max: MAX_FONT_SIZE,
      step: 1,
      unit: "pt",
      hint: `Range: ${MIN_FONT_SIZE}-${MAX_FONT_SIZE}pt`,
      onInput: (v) => applyFontSize(v),
      onSave: (v) => this.saveSetting("font_size", v),
    });

    // Font Family (text input)
    this.renderTextInput(panel, {
      key: "font-family",
      label: "Font Family",
      value: this.currentSettings.font_family,
      placeholder: "monospace (default)",
      hint: "CSS font-family value",
      onSave: (v) => {
        applyFontFamily(v);
        this.saveSetting("font_family", v);
      },
    });

    // Line Height (number input)
    this.renderNumberInput(panel, {
      key: "line-height",
      label: "Line Height",
      value: this.currentSettings.line_height,
      min: MIN_LINE_HEIGHT,
      max: MAX_LINE_HEIGHT,
      step: LINE_HEIGHT_STEP,
      unit: "",
      hint: `Range: ${MIN_LINE_HEIGHT}-${MAX_LINE_HEIGHT}`,
      onInput: (v) => applyLineHeight(v),
      onSave: (v) => this.saveSetting("line_height", v),
    });

    // -- Theme & Color subsection --
    this.renderSubsectionHeader(panel, "Theme & Color");

    // UI Theme (select)
    this.renderSelect(panel, {
      key: "ui-theme",
      label: "UI Theme",
      value: this.currentSettings.ui_theme,
      options: [
        { value: "system", label: "System" },
        { value: "light", label: "Light" },
        { value: "dark", label: "Dark" },
      ],
      onSave: (v) => {
        applyUiTheme(v as UiTheme);
        this.saveSetting("ui_theme", v as UiTheme);
      },
    });

    // Terminal Color Scheme (select)
    this.renderSelect(panel, {
      key: "terminal-color-scheme",
      label: "Terminal Color Scheme",
      value: this.currentSettings.terminal_color_scheme || "default",
      options: [
        { value: "default", label: "Default" },
      ],
      onSave: (v) => {
        const scheme = v === "default" ? "" : v;
        applyTerminalColorScheme(scheme);
        this.saveSetting("terminal_color_scheme", scheme);
      },
    });

    // Opacity (slider)
    this.renderSlider(panel, {
      key: "opacity",
      label: "Opacity",
      value: this.currentSettings.opacity,
      min: MIN_OPACITY,
      max: MAX_OPACITY,
      step: OPACITY_STEP,
      hint: `Range: ${MIN_OPACITY}-${MAX_OPACITY}`,
      onInput: (v) => applyOpacity(v),
      onSave: (v) => this.saveSetting("opacity", v),
    });

    // -- Layout subsection --
    this.renderSubsectionHeader(panel, "Layout");

    // Padding (number input)
    this.renderNumberInput(panel, {
      key: "padding",
      label: "Padding",
      value: this.currentSettings.padding,
      min: MIN_PADDING,
      max: MAX_PADDING,
      step: 1,
      unit: "px",
      hint: `Range: ${MIN_PADDING}-${MAX_PADDING}px`,
      onInput: (v) => applyPadding(v),
      onSave: (v) => this.saveSetting("padding", v),
    });

    // Scrollback Lines (number input)
    this.renderNumberInput(panel, {
      key: "scrollback-lines",
      label: "Scrollback Lines",
      value: this.currentSettings.scrollback_lines,
      min: MIN_SCROLLBACK_LINES,
      max: MAX_SCROLLBACK_LINES,
      step: 1000,
      unit: "",
      hint: `Range: ${MIN_SCROLLBACK_LINES}-${MAX_SCROLLBACK_LINES}`,
      onInput: () => {},
      onSave: (v) => this.saveSetting("scrollback_lines", v),
    });

    // Show Scrollbar (select)
    this.renderSelect(panel, {
      key: "show-scrollbar",
      label: "Show Scrollbar",
      value: this.currentSettings.show_scrollbar,
      options: [
        { value: "auto", label: "Auto" },
        { value: "always", label: "Always" },
        { value: "never", label: "Never" },
      ],
      onSave: (v) => {
        applyScrollbar(v as ScrollbarMode);
        this.saveSetting("show_scrollbar", v as ScrollbarMode);
      },
    });

    // -- Rich Content subsection --
    this.renderSubsectionHeader(panel, "Rich Content");

    // Inline Images (toggle)
    this.renderToggle(panel, {
      key: "inline-images",
      label: "Inline Images",
      value: this.currentSettings.inline_images_enabled,
      onSave: (v) => this.saveSetting("inline_images_enabled", v),
    });

    // Markdown Rendering (toggle)
    this.renderToggle(panel, {
      key: "markdown-rendering",
      label: "Markdown Rendering",
      value: this.currentSettings.markdown_rendering,
      onSave: (v) => this.saveSetting("markdown_rendering", v),
    });
  }

  // ============================================================
  // Terminal Category
  // ============================================================

  private renderTerminalSection(panel: HTMLElement): void {
    if (!this.currentSettings) return;

    const header = document.createElement("h2");
    header.className = "settings-section-header";
    header.textContent = "Terminal";
    panel.appendChild(header);

    // -- Cursor subsection --
    this.renderSubsectionHeader(panel, "Cursor");

    // Cursor Style (select)
    this.renderSelect(panel, {
      key: "cursor-style",
      label: "Cursor Style",
      value: this.currentSettings.cursor_style,
      options: [
        { value: "block", label: "Block" },
        { value: "underline", label: "Underline" },
        { value: "bar", label: "Bar" },
      ],
      onSave: (v) => {
        applyCursorStyle(v as CursorStyle);
        this.saveSetting("cursor_style", v as CursorStyle);
      },
    });

    // Cursor Blink (toggle)
    this.renderToggle(panel, {
      key: "cursor-blink",
      label: "Cursor Blink",
      value: this.currentSettings.cursor_blink,
      onSave: (v) => {
        applyCursorBlink(v);
        this.saveSetting("cursor_blink", v);
      },
    });

    // -- Shell subsection --
    this.renderSubsectionHeader(panel, "Shell");

    // Shell Path (text input)
    this.renderTextInput(panel, {
      key: "shell-path",
      label: "Shell Path",
      value: this.currentSettings.shell_path,
      placeholder: "System default",
      hint: "Applies to new tabs only",
      onSave: (v) => this.saveSetting("shell_path", v),
    });

    // Shell Arguments (text input, comma-separated)
    this.renderTextInput(panel, {
      key: "shell-args",
      label: "Shell Arguments",
      value: this.currentSettings.shell_args.join(", "),
      placeholder: "e.g. --login, -i",
      hint: "Comma-separated. Applies to new tabs only",
      onSave: (v) => {
        const args = v ? v.split(",").map((s) => s.trim()).filter(Boolean) : [];
        this.saveSetting("shell_args", args);
      },
    });

    // -- Behavior subsection --
    this.renderSubsectionHeader(panel, "Behavior");

    // Scroll Speed (slider)
    this.renderSlider(panel, {
      key: "scroll-speed",
      label: "Scroll Speed",
      value: this.currentSettings.scroll_speed,
      min: MIN_SCROLL_SPEED,
      max: MAX_SCROLL_SPEED,
      step: 1,
      hint: `Range: ${MIN_SCROLL_SPEED}-${MAX_SCROLL_SPEED}`,
      onInput: () => {},
      onSave: (v) => this.saveSetting("scroll_speed", v),
    });

    // Bell Action (select)
    this.renderSelect(panel, {
      key: "bell-action",
      label: "Bell Action",
      value: this.currentSettings.bell_action,
      options: [
        { value: "visual", label: "Visual" },
        { value: "sound", label: "Sound" },
        { value: "none", label: "None" },
      ],
      onSave: (v) => this.saveSetting("bell_action", v as BellAction),
    });

    // URL Detection (toggle)
    this.renderToggle(panel, {
      key: "url-detection",
      label: "URL Detection",
      value: this.currentSettings.url_detection,
      onSave: (v) => this.saveSetting("url_detection", v),
    });

    // Copy on Select (toggle)
    this.renderToggle(panel, {
      key: "copy-on-select",
      label: "Copy on Select",
      value: this.currentSettings.copy_on_select,
      onSave: (v) => this.saveSetting("copy_on_select", v),
    });
  }

  // ============================================================
  // Keybinds Category
  // ============================================================

  private renderKeybindsSection(panel: HTMLElement): void {
    if (!this.currentSettings) return;
    const kb = this.currentSettings.keybinds;

    const header = document.createElement("h2");
    header.className = "settings-section-header";
    header.textContent = "Keybinds";
    panel.appendChild(header);

    // -- Basic subsection --
    this.renderSubsectionHeader(panel, "Basic");
    this.renderKeybindInput(panel, "copy", "Copy", kb.copy);
    this.renderKeybindInput(panel, "paste", "Paste", kb.paste);
    this.renderKeybindInput(panel, "select_all", "Select All", kb.select_all);
    this.renderKeybindInput(panel, "search", "Search", kb.search);

    // -- Tab Management subsection --
    this.renderSubsectionHeader(panel, "Tab Management");
    this.renderKeybindInput(panel, "new_tab", "New Tab", kb.new_tab);
    this.renderKeybindInput(panel, "close_tab", "Close Tab", kb.close_tab);
    this.renderKeybindInput(panel, "next_tab", "Next Tab", kb.next_tab);
    this.renderKeybindInput(panel, "prev_tab", "Previous Tab", kb.prev_tab);

    // -- Display subsection --
    this.renderSubsectionHeader(panel, "Display");
    this.renderKeybindInput(panel, "zoom_in", "Zoom In", kb.zoom_in);
    this.renderKeybindInput(panel, "zoom_out", "Zoom Out", kb.zoom_out);
    this.renderKeybindInput(panel, "zoom_reset", "Zoom Reset", kb.zoom_reset);
    this.renderKeybindInput(panel, "toggle_fullscreen", "Toggle Fullscreen", kb.toggle_fullscreen);

    // -- Settings subsection --
    this.renderSubsectionHeader(panel, "Settings");
    this.renderKeybindInput(panel, "open_settings", "Open Settings", kb.open_settings);
  }

  // ============================================================
  // UI Control Renderers
  // ============================================================

  private renderSubsectionHeader(panel: HTMLElement, text: string): void {
    const h3 = document.createElement("h3");
    h3.className = "settings-subsection-header";
    h3.textContent = text;
    panel.appendChild(h3);
  }

  private renderNumberInput(panel: HTMLElement, opts: {
    key: string;
    label: string;
    value: number;
    min: number;
    max: number;
    step: number;
    unit: string;
    hint: string;
    onInput: (value: number) => void;
    onSave: (value: number) => void;
  }): void {
    const row = document.createElement("div");
    row.className = "settings-row";

    const label = document.createElement("label");
    label.className = "settings-label";
    label.htmlFor = `settings-${opts.key}`;
    label.textContent = opts.label;
    row.appendChild(label);

    const inputGroup = document.createElement("div");
    inputGroup.className = "settings-input-group";

    const input = document.createElement("input");
    input.type = "number";
    input.id = `settings-${opts.key}`;
    input.className = "settings-number-input";
    input.min = String(opts.min);
    input.max = String(opts.max);
    input.step = String(opts.step);
    input.value = String(opts.value);
    inputGroup.appendChild(input);

    if (opts.unit) {
      const unit = document.createElement("span");
      unit.className = "settings-unit";
      unit.textContent = opts.unit;
      inputGroup.appendChild(unit);
    }

    row.appendChild(inputGroup);

    const hint = document.createElement("span");
    hint.className = "settings-hint";
    hint.textContent = opts.hint;
    row.appendChild(hint);

    panel.appendChild(row);

    // Event listeners
    let lastSavedValue = opts.value;

    const inputHandler = () => {
      const v = Number(input.value);
      if (v >= opts.min && v <= opts.max) {
        opts.onInput(v);
      }
    };
    this.addContentListener(input, "input", inputHandler);

    const saveHandler = () => {
      let v = Number(input.value);
      if (isNaN(v)) {
        v = lastSavedValue;
      }
      v = Math.max(opts.min, Math.min(opts.max, v));
      input.value = String(v);
      if (v !== lastSavedValue) {
        lastSavedValue = v;
        opts.onSave(v);
      }
    };
    this.addContentListener(input, "blur", saveHandler);
    this.addContentListener(input, "keydown", (e: Event) => {
      if ((e as KeyboardEvent).key === "Enter") saveHandler();
    });
  }

  private renderTextInput(panel: HTMLElement, opts: {
    key: string;
    label: string;
    value: string;
    placeholder: string;
    hint: string;
    onSave: (value: string) => void;
  }): void {
    const row = document.createElement("div");
    row.className = "settings-row";

    const label = document.createElement("label");
    label.className = "settings-label";
    label.htmlFor = `settings-${opts.key}`;
    label.textContent = opts.label;
    row.appendChild(label);

    const input = document.createElement("input");
    input.type = "text";
    input.id = `settings-${opts.key}`;
    input.className = "settings-text-input";
    input.value = opts.value;
    input.placeholder = opts.placeholder;
    row.appendChild(input);

    const hint = document.createElement("span");
    hint.className = "settings-hint";
    hint.textContent = opts.hint;
    row.appendChild(hint);

    panel.appendChild(row);

    // Save on blur/Enter
    let lastSaved = opts.value;
    const saveHandler = () => {
      const v = input.value;
      if (v !== lastSaved) {
        lastSaved = v;
        opts.onSave(v);
      }
    };
    this.addContentListener(input, "blur", saveHandler);
    this.addContentListener(input, "keydown", (e: Event) => {
      if ((e as KeyboardEvent).key === "Enter") saveHandler();
    });
  }

  private renderSelect(panel: HTMLElement, opts: {
    key: string;
    label: string;
    value: string;
    options: Array<{ value: string; label: string }>;
    onSave: (value: string) => void;
  }): void {
    const row = document.createElement("div");
    row.className = "settings-row";

    const label = document.createElement("label");
    label.className = "settings-label";
    label.htmlFor = `settings-${opts.key}`;
    label.textContent = opts.label;
    row.appendChild(label);

    const select = document.createElement("select");
    select.id = `settings-${opts.key}`;
    select.className = "settings-select";

    for (const opt of opts.options) {
      const option = document.createElement("option");
      option.value = opt.value;
      option.textContent = opt.label;
      if (opt.value === opts.value) option.selected = true;
      select.appendChild(option);
    }
    row.appendChild(select);

    panel.appendChild(row);

    // Save on change
    this.addContentListener(select, "change", () => {
      opts.onSave(select.value);
    });
  }

  private renderToggle(panel: HTMLElement, opts: {
    key: string;
    label: string;
    value: boolean;
    onSave: (value: boolean) => void;
  }): void {
    const row = document.createElement("div");
    row.className = "settings-row settings-row-toggle";

    const label = document.createElement("label");
    label.className = "settings-label";
    label.htmlFor = `settings-${opts.key}`;
    label.textContent = opts.label;
    row.appendChild(label);

    const button = document.createElement("button");
    button.id = `settings-${opts.key}`;
    button.className = "settings-toggle";
    button.setAttribute("role", "switch");
    button.setAttribute("aria-checked", String(opts.value));
    if (opts.value) button.classList.add("on");

    const track = document.createElement("span");
    track.className = "settings-toggle-track";
    const thumb = document.createElement("span");
    thumb.className = "settings-toggle-thumb";
    track.appendChild(thumb);
    button.appendChild(track);

    row.appendChild(button);
    panel.appendChild(row);

    // Toggle on click
    let currentValue = opts.value;
    this.addContentListener(button, "click", () => {
      currentValue = !currentValue;
      button.setAttribute("aria-checked", String(currentValue));
      button.classList.toggle("on", currentValue);
      opts.onSave(currentValue);
    });
  }

  private renderSlider(panel: HTMLElement, opts: {
    key: string;
    label: string;
    value: number;
    min: number;
    max: number;
    step: number;
    hint: string;
    onInput: (value: number) => void;
    onSave: (value: number) => void;
  }): void {
    const row = document.createElement("div");
    row.className = "settings-row";

    const label = document.createElement("label");
    label.className = "settings-label";
    label.htmlFor = `settings-${opts.key}`;
    label.textContent = opts.label;
    row.appendChild(label);

    const sliderGroup = document.createElement("div");
    sliderGroup.className = "settings-slider-group";

    const input = document.createElement("input");
    input.type = "range";
    input.id = `settings-${opts.key}`;
    input.className = "settings-slider";
    input.min = String(opts.min);
    input.max = String(opts.max);
    input.step = String(opts.step);
    input.value = String(opts.value);
    sliderGroup.appendChild(input);

    const valueDisplay = document.createElement("span");
    valueDisplay.className = "settings-slider-value";
    valueDisplay.textContent = String(opts.value);
    sliderGroup.appendChild(valueDisplay);

    row.appendChild(sliderGroup);

    const hint = document.createElement("span");
    hint.className = "settings-hint";
    hint.textContent = opts.hint;
    row.appendChild(hint);

    panel.appendChild(row);

    // Event listeners
    this.addContentListener(input, "input", () => {
      const v = Number(input.value);
      valueDisplay.textContent = String(v);
      opts.onInput(v);
    });
    this.addContentListener(input, "change", () => {
      const v = Number(input.value);
      opts.onSave(v);
    });
  }

  private renderKeybindInput(
    panel: HTMLElement,
    keybindKey: string,
    label: string,
    currentValue: string,
  ): void {
    const row = document.createElement("div");
    row.className = "settings-row settings-row-keybind";

    const labelEl = document.createElement("label");
    labelEl.className = "settings-label";
    labelEl.textContent = label;
    row.appendChild(labelEl);

    const button = document.createElement("button");
    button.className = "settings-keybind-input";
    button.dataset.key = keybindKey;
    button.textContent = currentValue;
    row.appendChild(button);

    panel.appendChild(row);

    // Click to enter capture mode
    this.addContentListener(button, "click", () => {
      this.enterKeybindCapture(button, keybindKey, currentValue);
    });
  }

  // ============================================================
  // Keybind Capture
  // ============================================================

  private enterKeybindCapture(button: HTMLButtonElement, key: string, originalValue: string): void {
    // Cancel any existing capture
    if (this.capturingKeybindButton) {
      this.exitKeybindCapture(true);
    }

    this.capturingKeybindButton = button;
    this.capturingKeybindKey = key;
    this.capturingOriginalValue = originalValue;

    button.classList.add("capturing");
    button.textContent = "Press a key...";
    button.focus();

    // Capture keydown
    const keydownHandler = (e: Event) => {
      const ke = e as KeyboardEvent;
      e.preventDefault();
      e.stopPropagation();

      // Escape cancels
      if (ke.key === "Escape") {
        this.exitKeybindCapture(true);
        return;
      }

      // Ignore bare modifier keys
      if (["Control", "Shift", "Alt", "Meta"].includes(ke.key)) {
        return;
      }

      // Build key combination string
      const parts: string[] = [];
      if (ke.ctrlKey) parts.push("Ctrl");
      if (ke.shiftKey) parts.push("Shift");
      if (ke.altKey) parts.push("Alt");
      if (ke.metaKey) parts.push("Meta");

      // Normalize key name
      let keyName = ke.key;
      if (keyName === " ") keyName = "Space";
      else if (keyName === "+") keyName = "Plus";
      else if (keyName === "-") keyName = "Minus";
      else if (keyName.length === 1) keyName = keyName.toUpperCase();

      parts.push(keyName);
      const combo = parts.join("+");

      // Update button and save
      button.textContent = combo;
      this.saveKeybind(key, combo);
      this.exitKeybindCapture(false);
    };

    // Use capture phase to intercept before other handlers
    document.addEventListener("keydown", keydownHandler, true);

    // Store cleanup
    this.eventListeners.push({
      element: document,
      type: "keydown",
      handler: keydownHandler,
    });
  }

  private exitKeybindCapture(cancelled: boolean): void {
    if (!this.capturingKeybindButton) return;

    if (cancelled && this.capturingOriginalValue !== null) {
      this.capturingKeybindButton.textContent = this.capturingOriginalValue;
    }

    this.capturingKeybindButton.classList.remove("capturing");
    this.capturingKeybindButton = null;
    this.capturingKeybindKey = null;
    this.capturingOriginalValue = null;

    // Remove the capture keydown listener
    this.eventListeners = this.eventListeners.filter((listener) => {
      if (listener.element === document && listener.type === "keydown") {
        listener.element.removeEventListener(listener.type, listener.handler, true);
        return false;
      }
      return true;
    });
  }

  private async saveKeybind(key: string, value: string): Promise<void> {
    if (!this.currentSettings) return;
    (this.currentSettings.keybinds as any)[key] = value;
    try {
      await SettingsService.save(this.currentSettings);
    } catch (error) {
      console.error("Failed to save keybind:", error);
    }
  }

  // ============================================================
  // Settings Save Helper
  // ============================================================

  private async saveSetting<K extends keyof AppSettings>(
    key: K,
    value: AppSettings[K],
  ): Promise<void> {
    if (!this.currentSettings) return;
    this.currentSettings[key] = value;
    try {
      await SettingsService.save(this.currentSettings);
    } catch (error) {
      console.error("Failed to save setting:", error);
    }
  }

  // ============================================================
  // Event Listener Management
  // ============================================================

  private contentListeners: Array<{
    element: EventTarget;
    type: string;
    handler: EventListener;
    capture?: boolean;
  }> = [];

  private addContentListener(
    element: EventTarget,
    type: string,
    handler: EventListener,
    capture?: boolean,
  ): void {
    element.addEventListener(type, handler, capture);
    this.contentListeners.push({ element, type, handler, capture });
  }

  private detachContentListeners(): void {
    for (const l of this.contentListeners) {
      l.element.removeEventListener(l.type, l.handler, l.capture);
    }
    this.contentListeners = [];
  }

  private attachEventListeners(): void {
    // Navigation click
    if (this.navElement) {
      const navClickHandler = (e: Event) => {
        const target = e.target as HTMLElement;
        if (
          target.classList.contains("settings-nav-item") &&
          !target.classList.contains("disabled")
        ) {
          const categoryId = target.dataset.categoryId;
          if (categoryId && categoryId !== this.activeCategory) {
            this.switchCategory(categoryId);
          }
        }
      };
      this.navElement.addEventListener("click", navClickHandler);
      this.eventListeners.push({
        element: this.navElement,
        type: "click",
        handler: navClickHandler,
      });

      // Keyboard navigation
      const navKeydownHandler = (e: Event) => {
        this.handleNavKeydown(e as KeyboardEvent);
      };
      this.navElement.addEventListener("keydown", navKeydownHandler);
      this.eventListeners.push({
        element: this.navElement,
        type: "keydown",
        handler: navKeydownHandler,
      });
    }
  }

  private handleNavKeydown(e: KeyboardEvent): void {
    const target = e.target as HTMLElement;
    if (!target.classList.contains("settings-nav-item")) return;

    const enabledCategories = this.categories.filter((c) => c.enabled);
    const currentIndex = enabledCategories.findIndex(
      (c) => c.id === target.dataset.categoryId,
    );
    if (currentIndex === -1) return;

    let newIndex = currentIndex;

    switch (e.key) {
      case "ArrowDown":
      case "ArrowRight":
        e.preventDefault();
        newIndex = (currentIndex + 1) % enabledCategories.length;
        break;
      case "ArrowUp":
      case "ArrowLeft":
        e.preventDefault();
        newIndex =
          (currentIndex - 1 + enabledCategories.length) %
          enabledCategories.length;
        break;
      case "Home":
        e.preventDefault();
        newIndex = 0;
        break;
      case "End":
        e.preventDefault();
        newIndex = enabledCategories.length - 1;
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        if (
          target.dataset.categoryId &&
          target.dataset.categoryId !== this.activeCategory
        ) {
          this.switchCategory(target.dataset.categoryId);
        }
        return;
      default:
        return;
    }

    const newCategory = enabledCategories[newIndex];
    if (!newCategory) return;

    const newTab = this.navElement?.querySelector(
      `[data-category-id="${newCategory.id}"]`,
    ) as HTMLElement | null;
    if (newTab) {
      target.setAttribute("tabindex", "-1");
      newTab.setAttribute("tabindex", "0");
      newTab.focus();
    }
  }

  private switchCategory(categoryId: string): void {
    // Cancel any keybind capture
    if (this.capturingKeybindButton) {
      this.exitKeybindCapture(true);
    }

    this.detachContentListeners();
    this.activeCategory = categoryId;
    this.renderNavigation();
    this.renderContent();
  }

  // ============================================================
  // Public API
  // ============================================================

  getPanelElement(): HTMLElement {
    return this.container;
  }

  dispose(): void {
    if (this.capturingKeybindButton) {
      this.exitKeybindCapture(true);
    }

    this.detachContentListeners();

    for (const listener of this.eventListeners) {
      listener.element.removeEventListener(listener.type, listener.handler);
    }
    this.eventListeners = [];

    this.container.innerHTML = "";
    this.navElement = null;
    this.contentElement = null;
  }
}
