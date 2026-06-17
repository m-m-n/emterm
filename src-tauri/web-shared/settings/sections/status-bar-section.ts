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

  // Custom Commands subsection
  renderSubsectionHeader(panel, t("settings.statusBar.customCommands"));

  const commands = settings.statusbar_custom_commands ?? {};
  const commandEntries = Object.entries(commands);

  // Existing command list (editable inline)
  if (commandEntries.length === 0) {
    const empty = document.createElement("p");
    empty.className = "ssh-empty-state";
    empty.textContent = t("settings.statusBar.noCommands");
    panel.appendChild(empty);
  } else {
    for (const [name, cmd] of commandEntries) {
      const row = document.createElement("div");
      row.className = "statusbar-cmd-form";

      const nameInput = document.createElement("input");
      nameInput.type = "text";
      nameInput.className = "settings-text-input";
      nameInput.value = name;
      ctx.addContentListener(nameInput, "change", () => {
        const newName = nameInput.value.trim();
        if (!newName || newName === name) return;
        // Rename: remove old key, add new key
        const { [name]: cmdVal, ...rest } = ctx.currentSettings.statusbar_custom_commands;
        if (!cmdVal) return;
        ctx.currentSettings.statusbar_custom_commands = { ...rest, [newName]: cmdVal };
        ctx.saveSetting("statusbar_custom_commands", ctx.currentSettings.statusbar_custom_commands);
        applyStatusBar(ctx.currentSettings);
        ctx.reRender();
      });

      const execInput = document.createElement("input");
      execInput.type = "text";
      execInput.className = "settings-text-input";
      execInput.value = cmd.executable;
      ctx.addContentListener(execInput, "change", () => {
        const val = execInput.value.trim();
        if (!val) return;
        ctx.currentSettings.statusbar_custom_commands[name]!.executable = val;
        ctx.saveSetting("statusbar_custom_commands", { ...ctx.currentSettings.statusbar_custom_commands });
        applyStatusBar(ctx.currentSettings);
      });

      const intervalInput = document.createElement("input");
      intervalInput.type = "number";
      intervalInput.className = "settings-number-input";
      intervalInput.value = String(cmd.interval_ms);
      intervalInput.min = "100";
      intervalInput.title = t("settings.statusBar.commandIntervalHint");
      ctx.addContentListener(intervalInput, "change", () => {
        const val = parseInt(intervalInput.value, 10) || 1000;
        ctx.currentSettings.statusbar_custom_commands[name]!.interval_ms = val;
        ctx.saveSetting("statusbar_custom_commands", { ...ctx.currentSettings.statusbar_custom_commands });
        applyStatusBar(ctx.currentSettings);
      });

      const intervalLabel = document.createElement("span");
      intervalLabel.className = "statusbar-cmd-interval-label";
      intervalLabel.textContent = t("settings.statusBar.commandIntervalUnit");

      const intervalGroup = document.createElement("span");
      intervalGroup.className = "statusbar-cmd-interval-group";
      intervalGroup.appendChild(intervalInput);
      intervalGroup.appendChild(intervalLabel);

      const deleteBtn = document.createElement("button");
      deleteBtn.className = "profile-action-btn";
      deleteBtn.textContent = t("settings.statusBar.deleteCommand");
      ctx.addContentListener(deleteBtn, "click", () => {
        const { [name]: _, ...rest } = ctx.currentSettings.statusbar_custom_commands;
        ctx.currentSettings.statusbar_custom_commands = rest;
        ctx.saveSetting("statusbar_custom_commands", rest);
        applyStatusBar(ctx.currentSettings);
        ctx.reRender();
      });

      row.appendChild(nameInput);
      row.appendChild(execInput);
      row.appendChild(intervalGroup);
      row.appendChild(deleteBtn);
      panel.appendChild(row);
    }
  }

  // Inline add form
  const form = document.createElement("div");
  form.className = "statusbar-cmd-form";

  const nameInput = document.createElement("input");
  nameInput.type = "text";
  nameInput.className = "settings-text-input";
  nameInput.placeholder = t("settings.statusBar.commandNamePlaceholder");

  const execInput = document.createElement("input");
  execInput.type = "text";
  execInput.className = "settings-text-input";
  execInput.placeholder = t("settings.statusBar.commandExecutablePlaceholder");

  const intervalLabel = document.createElement("span");
  intervalLabel.className = "statusbar-cmd-interval-label";
  intervalLabel.textContent = t("settings.statusBar.commandIntervalUnit");

  const intervalInput = document.createElement("input");
  intervalInput.type = "number";
  intervalInput.className = "settings-number-input";
  intervalInput.placeholder = "1000";
  intervalInput.min = "100";
  intervalInput.title = t("settings.statusBar.commandIntervalHint");

  const addBtn = document.createElement("button");
  addBtn.className = "profile-action-btn";
  addBtn.textContent = t("settings.statusBar.addCommand");
  ctx.addContentListener(addBtn, "click", () => {
    const name = nameInput.value.trim();
    const executable = execInput.value.trim();
    const interval = parseInt(intervalInput.value, 10) || 1000;
    if (!name || !executable) return;

    ctx.currentSettings.statusbar_custom_commands = {
      ...ctx.currentSettings.statusbar_custom_commands,
      [name]: { executable, interval_ms: interval },
    };
    ctx.saveSetting("statusbar_custom_commands", ctx.currentSettings.statusbar_custom_commands);
    applyStatusBar(ctx.currentSettings);
    ctx.reRender();
  });

  form.appendChild(nameInput);
  form.appendChild(execInput);
  const intervalGroup = document.createElement("span");
  intervalGroup.className = "statusbar-cmd-interval-group";
  intervalGroup.appendChild(intervalInput);
  intervalGroup.appendChild(intervalLabel);
  form.appendChild(intervalGroup);
  form.appendChild(addBtn);
  panel.appendChild(form);

  // Appearance subsection
  renderSubsectionHeader(panel, t("settings.statusBar.appearance"));

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
}
