import {
  applyFontSize,
  applyFontFamily,
  applyPadding,
  applyScrollbar,
  applyBoldBrightensAnsiColors,
} from "../settings-applier";
import type { AppSettings, ScrollbarMode } from "../types";
import {
  MIN_FONT_SIZE,
  MAX_FONT_SIZE,
  MIN_PADDING,
  MAX_PADDING,
  MIN_SCROLLBACK_LINES,
  MAX_SCROLLBACK_LINES,
} from "../types";
import { t } from "../../i18n/index.ts";
import {
  renderSubsectionHeader,
  renderNumberInput,
  renderSelect,
  renderToggle,
} from "../settings-components";
import { renderFontPickerInput } from "../font-picker";
import { renderColorSchemeEditor } from "../color-scheme-editor";
import type { SectionContext } from "./types";

function applyCurrentFontFamily(settings: AppSettings): void {
  applyFontFamily(settings.font_family_primary, settings.font_family_secondary);
}

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
