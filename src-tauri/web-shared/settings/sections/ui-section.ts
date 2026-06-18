import { invoke } from "@tauri-apps/api/core";
import {
  applyUiTheme,
  applyUiFont,
  applyMarkdownColorTheme,
} from "../settings-applier";
import type { UiTheme, UiThemePreset, Language } from "../types";
import { t, setLocale, resolveLocale } from "../../i18n/index.ts";
import { renderSubsectionHeader, renderSelect } from "../settings-components";
import { renderFontPickerInput } from "../font-picker";
import type { SectionContext } from "./types";

export function renderUiSection(panel: HTMLElement, ctx: SectionContext): void {
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
