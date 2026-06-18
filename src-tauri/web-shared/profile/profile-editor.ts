/**
 * Profile Editor
 *
 * Modal dialog for creating and editing terminal profiles.
 * Provides SHELL/SSH tab UI for mutually exclusive shell vs SSH configuration.
 */

import { invoke } from "@tauri-apps/api/core";
import type { AppSettings, Profile } from "../settings/types";
import { t } from "../i18n/index.ts";
import { createEmptyProfile } from "./types";
import { SettingsService } from "../settings/settings-service";
import {
  createMd3Select,
  type Md3SelectOption,
} from "../components/md3-select";

export interface ProfileEditorOptions {
  profile?: Profile;
  onSave: (profile: Profile) => void;
  onCancel: () => void;
}

/**
 * Show a profile editor modal overlay.
 * Returns a cleanup function to remove the modal.
 */
export function showProfileEditor(options: ProfileEditorOptions): () => void {
  const profile = options.profile
    ? { ...options.profile, shell_args: [...options.profile.shell_args] }
    : createEmptyProfile();
  const isEdit = !!options.profile;

  // Overlay
  const overlay = document.createElement("div");
  overlay.className = "profile-editor-overlay";
  overlay.setAttribute("role", "dialog");
  overlay.setAttribute(
    "aria-label",
    isEdit
      ? t("settings.profiles.editProfile")
      : t("settings.profiles.addProfile"),
  );

  // Dialog container
  const dialog = document.createElement("div");
  dialog.className = "profile-editor-dialog";

  // Title
  const title = document.createElement("h2");
  title.className = "profile-editor-title";
  title.textContent = isEdit
    ? t("settings.profiles.editProfile")
    : t("settings.profiles.addProfile");
  dialog.appendChild(title);

  // Form
  const form = document.createElement("form");
  form.className = "profile-editor-form";

  // Error message area
  const errorEl = document.createElement("div");
  errorEl.className = "profile-editor-error";
  errorEl.setAttribute("role", "alert");
  errorEl.hidden = true;
  form.appendChild(errorEl);

  // === Tab bar (above all fields) ===
  const tabBar = document.createElement("div");
  tabBar.className = "profile-editor-tabs";
  tabBar.setAttribute("role", "tablist");

  const shellTab = document.createElement("button");
  shellTab.type = "button";
  shellTab.className = "profile-editor-tab active";
  shellTab.setAttribute("role", "tab");
  shellTab.setAttribute("aria-selected", "true");
  shellTab.setAttribute("aria-controls", "profile-tab-panel-shell");
  shellTab.id = "profile-tab-shell";
  shellTab.tabIndex = 0;
  shellTab.textContent = t("settings.profiles.tabShell");

  const sshTab = document.createElement("button");
  sshTab.type = "button";
  sshTab.className = "profile-editor-tab";
  sshTab.setAttribute("role", "tab");
  sshTab.setAttribute("aria-selected", "false");
  sshTab.setAttribute("aria-controls", "profile-tab-panel-ssh");
  sshTab.id = "profile-tab-ssh";
  sshTab.tabIndex = -1;
  sshTab.textContent = t("settings.profiles.tabSsh");

  // WSL tab (added conditionally after platform detection)
  const wslTab = document.createElement("button");
  wslTab.type = "button";
  wslTab.className = "profile-editor-tab";
  wslTab.setAttribute("role", "tab");
  wslTab.setAttribute("aria-selected", "false");
  wslTab.setAttribute("aria-controls", "profile-tab-panel-wsl");
  wslTab.id = "profile-tab-wsl";
  wslTab.tabIndex = -1;
  wslTab.textContent = t("settings.profiles.tabWsl");
  wslTab.hidden = true; // Hidden until platform detected as Windows

  tabBar.appendChild(shellTab);
  tabBar.appendChild(sshTab);
  tabBar.appendChild(wslTab);
  form.appendChild(tabBar);

  // === SHELL tab panel ===
  const shellPanel = document.createElement("div");
  shellPanel.className = "profile-editor-tab-panel";
  shellPanel.setAttribute("role", "tabpanel");
  shellPanel.setAttribute("aria-labelledby", "profile-tab-shell");
  shellPanel.id = "profile-tab-panel-shell";

  // Name field (inside SHELL tab)
  const nameInput = createTextField(shellPanel, {
    id: "profile-name",
    label: t("settings.profiles.name"),
    value: profile.name,
    placeholder: t("settings.profiles.namePlaceholder"),
  });

  const shellPathInput = createTextField(shellPanel, {
    id: "profile-shell-path",
    label: t("settings.profiles.shellPath"),
    value: profile.shell_path,
    placeholder: t("settings.profiles.shellPathPlaceholder"),
    hint: t("settings.profiles.shellPathHint"),
  });

  const shellArgsInput = createTextField(shellPanel, {
    id: "profile-shell-args",
    label: t("settings.profiles.shellArgs"),
    value: profile.shell_args.join(", "),
    placeholder: t("settings.profiles.shellArgsPlaceholder"),
    hint: t("settings.profiles.shellArgsHint"),
  });

  const envVarsTextarea = createTextareaField(shellPanel, {
    id: "profile-env-vars",
    label: t("settings.profiles.envVars"),
    value: profile.env_vars,
    placeholder: t("settings.profiles.envVarsPlaceholder"),
    hint: t("settings.profiles.envVarsHint"),
    rows: 4,
  });

  const workDirInput = createTextField(shellPanel, {
    id: "profile-working-directory",
    label: t("settings.profiles.workingDirectory"),
    value: profile.working_directory,
    placeholder: t("settings.profiles.workingDirectoryPlaceholder"),
    hint: t("settings.profiles.workingDirectoryHint"),
  });

  form.appendChild(shellPanel);

  // === SSH tab panel ===
  const sshPanel = document.createElement("div");
  sshPanel.className = "profile-editor-tab-panel";
  sshPanel.setAttribute("role", "tabpanel");
  sshPanel.setAttribute("aria-labelledby", "profile-tab-ssh");
  sshPanel.id = "profile-tab-panel-ssh";
  sshPanel.hidden = true;

  const sshSelect = createSelectField(sshPanel, {
    id: "profile-ssh-connection",
    label: t("settings.profiles.sshConnection"),
    value: profile.ssh_connection_name,
    hint: t("settings.profiles.sshConnectionHint"),
    options: [{ value: "", label: t("settings.profiles.sshConnectionSelect") }],
  });

  form.appendChild(sshPanel);

  // === WSL tab panel ===
  const wslPanel = document.createElement("div");
  wslPanel.className = "profile-editor-tab-panel";
  wslPanel.setAttribute("role", "tabpanel");
  wslPanel.setAttribute("aria-labelledby", "profile-tab-wsl");
  wslPanel.id = "profile-tab-panel-wsl";
  wslPanel.hidden = true;

  const wslSelect = createSelectField(wslPanel, {
    id: "profile-wsl-distro",
    label: t("settings.profiles.wslDistribution"),
    value: profile.wsl_distro_name,
    hint: t("settings.profiles.wslDistributionHint"),
    options: [
      { value: "", label: t("settings.profiles.wslDistributionSelect") },
    ],
  });

  form.appendChild(wslPanel);

  // Tab switching logic
  let activeTab: "shell" | "ssh" | "wsl" = "shell";

  function switchTab(target: "shell" | "ssh" | "wsl") {
    if (target === activeTab) return;
    if (target === "ssh" && sshTab.classList.contains("disabled")) return;
    if (target === "wsl" && wslTab.classList.contains("disabled")) return;

    activeTab = target;

    // Deactivate all tabs
    for (const tab of [shellTab, sshTab, wslTab]) {
      tab.classList.remove("active");
      tab.setAttribute("aria-selected", "false");
      tab.tabIndex = -1;
    }
    shellPanel.hidden = true;
    sshPanel.hidden = true;
    wslPanel.hidden = true;

    // Activate target
    const tabMap = { shell: shellTab, ssh: sshTab, wsl: wslTab };
    const panelMap = { shell: shellPanel, ssh: sshPanel, wsl: wslPanel };
    tabMap[target].classList.add("active");
    tabMap[target].setAttribute("aria-selected", "true");
    tabMap[target].tabIndex = 0;
    panelMap[target].hidden = false;

    // Clear inactive values
    if (target !== "shell") {
      shellPathInput.value = "";
      shellArgsInput.value = "";
      envVarsTextarea.value = "";
      workDirInput.value = "";
    }
    if (target !== "ssh") sshSelect.value = "";
    if (target !== "wsl") wslSelect.value = "";
  }

  shellTab.addEventListener("click", () => switchTab("shell"));
  sshTab.addEventListener("click", () => switchTab("ssh"));
  wslTab.addEventListener("click", () => switchTab("wsl"));

  // Keyboard navigation (arrow keys between visible tabs)
  const tabElements = { shell: shellTab, ssh: sshTab, wsl: wslTab };
  tabBar.addEventListener("keydown", (e) => {
    if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
      e.preventDefault();
      const visibleTabs: Array<"shell" | "ssh" | "wsl"> = ["shell", "ssh"];
      if (!wslTab.hidden) visibleTabs.push("wsl");
      const idx = visibleTabs.indexOf(activeTab);
      const dir = e.key === "ArrowRight" ? 1 : -1;
      const next =
        visibleTabs[(idx + dir + visibleTabs.length) % visibleTabs.length]!;
      switchTab(next);
      tabElements[next].focus();
    }
  });

  // Load settings, platform, and WSL distributions in parallel
  let disposed = false;

  Promise.all([
    SettingsService.load(),
    invoke<string>("get_platform").catch(() => "linux"),
    invoke<string[]>("detect_wsl_distributions").catch(() => [] as string[]),
  ])
    .then(
      ([settings, platform, wslDistros]: [AppSettings, string, string[]]) => {
        if (disposed) return;

        // SSH connections
        for (const conn of settings.ssh_connections) {
          const opt = document.createElement("option");
          opt.value = conn.name;
          opt.textContent = conn.name;
          if (conn.name === profile.ssh_connection_name) {
            opt.selected = true;
          }
          sshSelect.appendChild(opt);
        }

        if (settings.ssh_connections.length === 0) {
          sshTab.classList.add("disabled");
          sshTab.setAttribute("aria-disabled", "true");
          sshTab.title = t("settings.profiles.sshTabDisabled");
        }

        // WSL distributions (Windows only)
        if (platform === "windows") {
          wslTab.hidden = false;

          for (const distro of wslDistros) {
            const opt = document.createElement("option");
            opt.value = distro;
            opt.textContent = distro;
            if (distro === profile.wsl_distro_name) {
              opt.selected = true;
            }
            wslSelect.appendChild(opt);
          }

          if (wslDistros.length === 0) {
            wslTab.classList.add("disabled");
            wslTab.setAttribute("aria-disabled", "true");
            wslTab.title = t("settings.profiles.wslTabDisabled");
          }
        }

        // Auto-select tab based on existing profile
        if (profile.wsl_distro_name && platform === "windows") {
          switchTab("wsl");
        } else if (profile.ssh_connection_name) {
          switchTab("ssh");
        }
      },
    )
    .catch(() => {
      sshTab.classList.add("disabled");
      sshTab.setAttribute("aria-disabled", "true");
    });

  // Buttons
  const btnRow = document.createElement("div");
  btnRow.className = "profile-editor-buttons";

  const cancelBtn = document.createElement("button");
  cancelBtn.type = "button";
  cancelBtn.className = "profile-editor-btn profile-editor-btn-cancel";
  cancelBtn.textContent = t("settings.profiles.cancel");

  const saveBtn = document.createElement("button");
  saveBtn.type = "submit";
  saveBtn.className = "profile-editor-btn profile-editor-btn-save";
  saveBtn.textContent = t("settings.profiles.save");

  btnRow.appendChild(cancelBtn);
  btnRow.appendChild(saveBtn);
  form.appendChild(btnRow);
  dialog.appendChild(form);
  overlay.appendChild(dialog);
  document.body.appendChild(overlay);

  // Focus name field
  nameInput.focus();

  // Handlers
  const cleanup = () => {
    disposed = true;
    overlay.remove();
  };

  cancelBtn.addEventListener("click", () => {
    cleanup();
    options.onCancel();
  });

  overlay.addEventListener("click", (e) => {
    if (e.target === overlay) {
      cleanup();
      options.onCancel();
    }
  });

  overlay.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      cleanup();
      options.onCancel();
    }
  });

  form.addEventListener("submit", (e) => {
    e.preventDefault();

    let name: string;
    if (activeTab === "shell") {
      name = nameInput.value.trim();
      if (!name) {
        errorEl.textContent = t("settings.profiles.nameRequired");
        errorEl.hidden = false;
        nameInput.focus();
        return;
      }
    } else if (activeTab === "ssh") {
      name = sshSelect.value;
      if (!name) {
        errorEl.textContent = t("settings.profiles.sshConnectionRequired");
        errorEl.hidden = false;
        sshSelect.focus();
        return;
      }
    } else {
      // WSL tab: use distro name as profile name
      name = wslSelect.value;
      if (!name) {
        errorEl.textContent = t("settings.profiles.wslDistributionRequired");
        errorEl.hidden = false;
        wslSelect.focus();
        return;
      }
    }

    const result: Profile = {
      name,
      shell_path: activeTab === "shell" ? shellPathInput.value.trim() : "",
      shell_args:
        activeTab === "shell"
          ? shellArgsInput.value
              .split(",")
              .map((s) => s.trim())
              .filter((s) => s !== "")
          : [],
      env_vars: activeTab === "shell" ? envVarsTextarea.value : "",
      working_directory: activeTab === "shell" ? workDirInput.value.trim() : "",
      is_default: profile.is_default,
      ssh_connection_name: activeTab === "ssh" ? sshSelect.value : "",
      wsl_distro_name: activeTab === "wsl" ? wslSelect.value : "",
    };

    cleanup();
    options.onSave(result);
  });

  return cleanup;
}

// ============================================================
// Field helpers
// ============================================================

interface TextFieldOptions {
  id: string;
  label: string;
  value: string;
  placeholder?: string;
  hint?: string;
  required?: boolean;
}

function createTextField(
  container: HTMLElement,
  opts: TextFieldOptions,
): HTMLInputElement {
  const row = document.createElement("div");
  row.className = "profile-editor-field";

  const label = document.createElement("label");
  label.className = "profile-editor-label";
  label.htmlFor = opts.id;
  label.textContent = opts.label;
  row.appendChild(label);

  const input = document.createElement("input");
  input.type = "text";
  input.id = opts.id;
  input.className = "profile-editor-input";
  input.value = opts.value;
  if (opts.placeholder) input.placeholder = opts.placeholder;
  if (opts.required) input.required = true;
  row.appendChild(input);

  if (opts.hint) {
    const hint = document.createElement("span");
    hint.className = "profile-editor-hint";
    hint.textContent = opts.hint;
    row.appendChild(hint);
  }

  container.appendChild(row);
  return input;
}

interface TextareaFieldOptions {
  id: string;
  label: string;
  value: string;
  placeholder?: string;
  hint?: string;
  rows?: number;
}

function createTextareaField(
  container: HTMLElement,
  opts: TextareaFieldOptions,
): HTMLTextAreaElement {
  const row = document.createElement("div");
  row.className = "profile-editor-field";

  const label = document.createElement("label");
  label.className = "profile-editor-label";
  label.htmlFor = opts.id;
  label.textContent = opts.label;
  row.appendChild(label);

  const textarea = document.createElement("textarea");
  textarea.id = opts.id;
  textarea.className = "profile-editor-textarea";
  textarea.value = opts.value;
  if (opts.placeholder) textarea.placeholder = opts.placeholder;
  if (opts.rows) textarea.rows = opts.rows;
  row.appendChild(textarea);

  if (opts.hint) {
    const hint = document.createElement("span");
    hint.className = "profile-editor-hint";
    hint.textContent = opts.hint;
    row.appendChild(hint);
  }

  container.appendChild(row);
  return textarea;
}

interface SelectFieldOptions {
  id: string;
  label: string;
  value: string;
  hint?: string;
  options: Array<{ value: string; label: string }>;
}

interface Md3SelectFieldProxy {
  get value(): string;
  set value(v: string);
  focus(): void;
  appendChild(opt: {
    value: string;
    textContent: string | null;
    selected?: boolean;
  }): void;
  /** Access to underlying md3 select for option updates */
  _md3: ReturnType<typeof createMd3Select>;
  _options: Md3SelectOption[];
}

function createSelectField(
  container: HTMLElement,
  opts: SelectFieldOptions,
): Md3SelectFieldProxy {
  const row = document.createElement("div");
  row.className = "profile-editor-field";

  const label = document.createElement("label");
  label.className = "profile-editor-label";
  label.htmlFor = opts.id;
  label.textContent = opts.label;
  row.appendChild(label);

  const currentOptions: Md3SelectOption[] = opts.options.map((o) => ({
    value: o.value,
    label: o.label,
  }));

  const md3 = createMd3Select({
    id: opts.id,
    options: currentOptions,
    value: opts.value,
    onChange: () => {},
  });
  row.appendChild(md3.element);

  if (opts.hint) {
    const hint = document.createElement("span");
    hint.className = "profile-editor-hint";
    hint.textContent = opts.hint;
    row.appendChild(hint);
  }

  container.appendChild(row);

  return {
    get value() {
      return md3.getValue();
    },
    set value(v: string) {
      md3.setValue(v);
    },
    focus() {
      const trigger = md3.element.querySelector(
        ".md3-select-trigger",
      ) as HTMLElement | null;
      trigger?.focus();
    },
    appendChild(opt) {
      currentOptions.push({ value: opt.value, label: opt.textContent ?? "" });
      const newValue = opt.selected ? opt.value : md3.getValue();
      md3.updateOptions(currentOptions, newValue);
    },
    _md3: md3,
    _options: currentOptions,
  };
}
