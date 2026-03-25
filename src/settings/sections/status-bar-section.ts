/**
 * Status Bar Settings Section
 *
 * Renders the "Status Bar" category in the settings panel.
 * Provides enable/disable toggle, template string inputs, and appearance customization.
 */

import { t } from "../../i18n/index.ts";
import {
  renderSubsectionHeader,
  renderToggle,
  renderTextInput,
  renderNumberInput,
  renderSlider,
} from "../settings-components";
import { applyStatusBar } from "../settings-applier";
import type { SectionContext } from "./types";

export function renderStatusBarSection(
  panel: HTMLElement,
  ctx: SectionContext,
): void {
  const { currentSettings: settings } = ctx;

  const header = document.createElement("h2");
  header.className = "settings-section-header";
  header.textContent = t("settings.statusBar.title");
  panel.appendChild(header);

  // Enable/Disable toggle
  renderToggle(
    panel,
    {
      key: "statusbar-enabled",
      label: t("settings.statusBar.enabled"),
      value: settings.statusbar_enabled,
      description: t("settings.statusBar.enabledDesc"),
      onSave: (v) => { ctx.saveSetting("statusbar_enabled", v); applyStatusBar(ctx.currentSettings); },
    },
    ctx.addContentListener,
  );

  // Templates subsection
  renderSubsectionHeader(panel, t("settings.statusBar.templates"));

  // App Line 1 Left
  renderTextInput(
    panel,
    {
      key: "statusbar-app-line1-left",
      label: t("settings.statusBar.appLine1Left"),
      value: settings.statusbar_app_line1_left,
      placeholder: "{time}",
      hint: t("settings.statusBar.templateHint"),
      onSave: (v) => { ctx.saveSetting("statusbar_app_line1_left", v); applyStatusBar(ctx.currentSettings); },
    },
    ctx.addContentListener,
  );

  // App Line 1 Right
  renderTextInput(
    panel,
    {
      key: "statusbar-app-line1-right",
      label: t("settings.statusBar.appLine1Right"),
      value: settings.statusbar_app_line1_right,
      placeholder: "{cwd}",
      hint: "",
      onSave: (v) => { ctx.saveSetting("statusbar_app_line1_right", v); applyStatusBar(ctx.currentSettings); },
    },
    ctx.addContentListener,
  );

  // App Line 2 Left
  renderTextInput(
    panel,
    {
      key: "statusbar-app-line2-left",
      label: t("settings.statusBar.appLine2Left"),
      value: settings.statusbar_app_line2_left,
      placeholder: "",
      hint: t("settings.statusBar.line2Hint"),
      onSave: (v) => { ctx.saveSetting("statusbar_app_line2_left", v); applyStatusBar(ctx.currentSettings); },
    },
    ctx.addContentListener,
  );

  // App Line 2 Right
  renderTextInput(
    panel,
    {
      key: "statusbar-app-line2-right",
      label: t("settings.statusBar.appLine2Right"),
      value: settings.statusbar_app_line2_right,
      placeholder: "",
      hint: "",
      onSave: (v) => { ctx.saveSetting("statusbar_app_line2_right", v); applyStatusBar(ctx.currentSettings); },
    },
    ctx.addContentListener,
  );

  // Time format
  renderTextInput(
    panel,
    {
      key: "statusbar-time-format",
      label: t("settings.statusBar.timeFormat"),
      value: settings.statusbar_time_format,
      placeholder: "HH:mm:ss",
      hint: t("settings.statusBar.timeFormatHint"),
      onSave: (v) => { ctx.saveSetting("statusbar_time_format", v); applyStatusBar(ctx.currentSettings); },
    },
    ctx.addContentListener,
  );

  // Appearance subsection
  renderSubsectionHeader(panel, t("settings.statusBar.appearance"));

  // Background color
  renderTextInput(
    panel,
    {
      key: "statusbar-bg-color",
      label: t("settings.statusBar.bgColor"),
      value: settings.statusbar_bg_color,
      placeholder: t("settings.statusBar.colorDefault"),
      hint: "",
      onSave: (v) => { ctx.saveSetting("statusbar_bg_color", v); applyStatusBar(ctx.currentSettings); },
    },
    ctx.addContentListener,
  );

  // Foreground color
  renderTextInput(
    panel,
    {
      key: "statusbar-fg-color",
      label: t("settings.statusBar.fgColor"),
      value: settings.statusbar_fg_color,
      placeholder: t("settings.statusBar.colorDefault"),
      hint: "",
      onSave: (v) => { ctx.saveSetting("statusbar_fg_color", v); applyStatusBar(ctx.currentSettings); },
    },
    ctx.addContentListener,
  );

  // Font size
  renderNumberInput(
    panel,
    {
      key: "statusbar-font-size",
      label: t("settings.statusBar.fontSize"),
      value: settings.statusbar_font_size ?? 11,
      min: 8,
      max: 32,
      step: 1,
      unit: "pt",
      hint: t("settings.statusBar.fontSizeHint"),
      onInput: () => {},
      onSave: (v) => { ctx.saveSetting("statusbar_font_size", v); applyStatusBar(ctx.currentSettings); },
    },
    ctx.addContentListener,
  );

  // Opacity
  renderSlider(
    panel,
    {
      key: "statusbar-opacity",
      label: t("settings.statusBar.opacity"),
      value: settings.statusbar_opacity,
      min: 0,
      max: 1,
      step: 0.05,
      hint: `${Math.round(settings.statusbar_opacity * 100)}%`,
      onInput: () => {},
      onSave: (v) => { ctx.saveSetting("statusbar_opacity", v); applyStatusBar(ctx.currentSettings); },
    },
    ctx.addContentListener,
  );
}
