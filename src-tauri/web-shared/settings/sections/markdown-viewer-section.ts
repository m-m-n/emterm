import {
  applyMarkdownSettings,
  applyMarkdownColorTheme,
} from "../settings-applier";
import type { UiTheme, UiThemePreset } from "../types";
import { MIN_FONT_SIZE, MAX_FONT_SIZE } from "../types";
import { t } from "../../i18n/index.ts";
import {
  renderSubsectionHeader,
  renderNumberInput,
  renderSelect,
  renderToggle,
} from "../settings-components";
import { renderFontPickerInput } from "../font-picker";
import type { SectionContext } from "./types";

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

  // Color Emoji Font Family (font picker) - between body and code per SPEC.md
  renderFontPickerInput(
    panel,
    {
      key: "markdown-emoji-font-family-color",
      label: t("settings.markdownViewer.emojiFontFamilyColor"),
      value: settings.markdown_emoji_font_family_color,
      placeholder: "",
      hint: t("settings.markdownViewer.emojiFontFamilyColorHint"),
      description: t("settings.markdownViewer.emojiFontFamilyColorDesc"),
      category: "markdown-emoji-color",
      onSelect: (v) => {
        ctx.currentSettings.markdown_emoji_font_family_color = v;
        applyMarkdownSettings(
          ctx.currentSettings.markdown_body_font_family,
          ctx.currentSettings.markdown_code_font_family,
          v,
          ctx.currentSettings.markdown_font_size,
        );
        ctx.saveSetting("markdown_emoji_font_family_color", v);
      },
    },
    ctx.addContentListener,
    (category, currentValue, onSelect) => {
      ctx.showFontPicker(category, currentValue, onSelect);
    },
  );

  // Monochrome Emoji Font Family (font picker)
  renderFontPickerInput(
    panel,
    {
      key: "markdown-emoji-font-family-monochrome",
      label: t("settings.markdownViewer.emojiFontFamilyMonochrome"),
      value: settings.markdown_emoji_font_family_monochrome,
      placeholder: "",
      hint: t("settings.markdownViewer.emojiFontFamilyMonochromeHint"),
      description: t("settings.markdownViewer.emojiFontFamilyMonochromeDesc"),
      category: "markdown-emoji-monochrome",
      onSelect: (v) => {
        ctx.currentSettings.markdown_emoji_font_family_monochrome = v;
        // Monochrome side does not yet feed the CSS preview helper;
        // the dispatch lives in the renderer (presentation.rs). For now,
        // just persist the value so settings round-trip.
        ctx.saveSetting("markdown_emoji_font_family_monochrome", v);
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
