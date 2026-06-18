/**
 * SSH Connection Editor
 *
 * Modal dialog for creating and editing SSH connection entries.
 * Follows the same pattern as profile-editor.ts.
 */

import { invoke } from "@tauri-apps/api/core";
import type { SshConnection, SshOption } from "../settings/types";
import { t } from "../i18n/index.ts";

export interface SshEditorOptions {
  connection?: SshConnection;
  onSave: (connection: SshConnection) => void;
  onCancel: () => void;
}

/**
 * Show an SSH connection editor modal overlay.
 * Returns a cleanup function to remove the modal.
 */
export function showSshEditor(options: SshEditorOptions): () => void {
  const connection: SshConnection = options.connection
    ? {
        ...options.connection,
        ssh_options: [...(options.connection.ssh_options || [])],
      }
    : {
        name: "",
        hostname: "",
        port: 22,
        username: "",
        identity_file: "",
        ssh_options: [],
      };
  const isEdit = !!options.connection;

  // Overlay
  const overlay = document.createElement("div");
  overlay.className = "profile-editor-overlay";
  overlay.setAttribute("role", "dialog");
  overlay.setAttribute(
    "aria-label",
    isEdit ? t("settings.ssh.editConnection") : t("settings.ssh.addConnection"),
  );

  // Dialog container
  const dialog = document.createElement("div");
  dialog.className = "profile-editor-dialog";

  // Title
  const title = document.createElement("h2");
  title.className = "profile-editor-title";
  title.textContent = isEdit
    ? t("settings.ssh.editConnection")
    : t("settings.ssh.addConnection");
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

  // Name field
  const nameInput = createField(form, {
    id: "ssh-name",
    label: t("settings.ssh.name"),
    value: connection.name,
    placeholder: t("settings.ssh.namePlaceholder"),
    required: true,
  });

  // Hostname field
  const hostnameInput = createField(form, {
    id: "ssh-hostname",
    label: t("settings.ssh.hostname"),
    value: connection.hostname,
    placeholder: t("settings.ssh.hostnamePlaceholder"),
    required: true,
  });

  // Port field
  const portInput = createField(form, {
    id: "ssh-port",
    label: t("settings.ssh.port"),
    value: String(connection.port),
    type: "number",
  });

  // Username field
  const usernameInput = createField(form, {
    id: "ssh-username",
    label: t("settings.ssh.username"),
    value: connection.username,
    placeholder: t("settings.ssh.usernamePlaceholder"),
  });

  // Identity file field
  const identityInput = createField(form, {
    id: "ssh-identity-file",
    label: t("settings.ssh.identityFile"),
    value: connection.identity_file,
    placeholder: t("settings.ssh.identityFilePlaceholder"),
    hint: t("settings.ssh.identityFileHint"),
  });

  // SSH Options (-o Key=Value) dynamic list
  const optionsContainer = createSshOptionsUI(form, connection.ssh_options);

  // Buttons
  const btnRow = document.createElement("div");
  btnRow.className = "profile-editor-buttons";

  const cancelBtn = document.createElement("button");
  cancelBtn.type = "button";
  cancelBtn.className = "profile-editor-btn profile-editor-btn-cancel";
  cancelBtn.textContent = t("settings.ssh.cancel");

  const saveBtn = document.createElement("button");
  saveBtn.type = "submit";
  saveBtn.className = "profile-editor-btn profile-editor-btn-save";
  saveBtn.textContent = t("settings.ssh.save");

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

  form.addEventListener("submit", async (e) => {
    e.preventDefault();

    const name = nameInput.value.trim();
    if (!name) {
      showError(errorEl, t("settings.ssh.nameRequired"));
      nameInput.focus();
      return;
    }

    const hostname = hostnameInput.value.trim();
    if (!hostname) {
      showError(errorEl, t("settings.ssh.hostnameRequired"));
      hostnameInput.focus();
      return;
    }

    const port = Number.parseInt(portInput.value, 10);
    if (Number.isNaN(port) || port < 1 || port > 65535) {
      showError(errorEl, t("settings.ssh.portRange"));
      portInput.focus();
      return;
    }

    const identityFile = identityInput.value.trim();
    if (identityFile) {
      try {
        const exists = await invoke<boolean>("validate_identity_file", {
          path: identityFile,
        });
        if (!exists) {
          showError(errorEl, t("settings.ssh.identityFileNotFound"));
          identityInput.focus();
          return;
        }
      } catch {
        // If validation fails, proceed anyway (file might be on remote)
      }
    }

    const result: SshConnection = {
      name,
      hostname,
      port,
      username: usernameInput.value.trim(),
      identity_file: identityFile,
      ssh_options: collectSshOptions(optionsContainer),
    };

    cleanup();
    options.onSave(result);
  });

  return cleanup;
}

// ============================================================
// SSH Options Dynamic Key-Value UI
// ============================================================

function createSshOptionsUI(
  form: HTMLFormElement,
  initialOptions: SshOption[],
): HTMLElement {
  const section = document.createElement("div");
  section.className = "profile-editor-field";

  const label = document.createElement("label");
  label.className = "profile-editor-label";
  label.textContent = t("settings.ssh.sshOptions");
  section.appendChild(label);

  const hint = document.createElement("span");
  hint.className = "profile-editor-hint";
  hint.textContent = t("settings.ssh.sshOptionsHint");
  section.appendChild(hint);

  const listContainer = document.createElement("div");
  listContainer.className = "ssh-options-list";
  section.appendChild(listContainer);

  // Add existing options as committed rows (with remove button)
  for (const opt of initialOptions) {
    addCommittedRow(listContainer, opt.key, opt.value);
  }

  // Always end with an empty "add" row
  addAddRow(listContainer);

  form.appendChild(section);
  return listContainer;
}

/** Row with filled values and a remove (×) button */
function addCommittedRow(
  container: HTMLElement,
  key: string,
  value: string,
): void {
  const row = document.createElement("div");
  row.className = "ssh-option-row";

  const keyInput = document.createElement("input");
  keyInput.type = "text";
  keyInput.className = "profile-editor-input ssh-option-key";
  keyInput.value = key;
  keyInput.placeholder = "Key";
  row.appendChild(keyInput);

  const eqSpan = document.createElement("span");
  eqSpan.className = "ssh-option-eq";
  eqSpan.textContent = "=";
  row.appendChild(eqSpan);

  const valueInput = document.createElement("input");
  valueInput.type = "text";
  valueInput.className = "profile-editor-input ssh-option-value";
  valueInput.value = value;
  valueInput.placeholder = "Value";
  row.appendChild(valueInput);

  const removeBtn = document.createElement("button");
  removeBtn.type = "button";
  removeBtn.className = "profile-action-btn ssh-option-remove-btn";
  removeBtn.textContent = "\u00D7";
  removeBtn.addEventListener("click", () => row.remove());
  row.appendChild(removeBtn);

  container.appendChild(row);
}

/** Empty row with an add (+) button; clicking converts it to a committed row and appends a new add row */
function addAddRow(container: HTMLElement): void {
  const row = document.createElement("div");
  row.className = "ssh-option-row";

  const keyInput = document.createElement("input");
  keyInput.type = "text";
  keyInput.className = "profile-editor-input ssh-option-key";
  keyInput.placeholder = "Key";
  row.appendChild(keyInput);

  const eqSpan = document.createElement("span");
  eqSpan.className = "ssh-option-eq";
  eqSpan.textContent = "=";
  row.appendChild(eqSpan);

  const valueInput = document.createElement("input");
  valueInput.type = "text";
  valueInput.className = "profile-editor-input ssh-option-value";
  valueInput.placeholder = "Value";
  row.appendChild(valueInput);

  const addBtn = document.createElement("button");
  addBtn.type = "button";
  addBtn.className = "profile-action-btn ssh-option-add-btn";
  addBtn.textContent = "+";
  addBtn.addEventListener("click", () => {
    // Convert this row's + button to × (remove) button
    const removeBtn = document.createElement("button");
    removeBtn.type = "button";
    removeBtn.className = "profile-action-btn ssh-option-remove-btn";
    removeBtn.textContent = "\u00D7";
    removeBtn.addEventListener("click", () => row.remove());
    addBtn.replaceWith(removeBtn);

    // Append a new empty add row
    addAddRow(container);
  });
  row.appendChild(addBtn);

  container.appendChild(row);
}

function collectSshOptions(container: HTMLElement): SshOption[] {
  const options: SshOption[] = [];
  for (const row of container.querySelectorAll(".ssh-option-row")) {
    const keyInput = row.querySelector(".ssh-option-key") as HTMLInputElement;
    const valueInput = row.querySelector(
      ".ssh-option-value",
    ) as HTMLInputElement;
    const key = keyInput?.value.trim() ?? "";
    const value = valueInput?.value.trim() ?? "";
    if (key && value) {
      options.push({ key, value });
    }
  }
  return options;
}

// ============================================================
// Helpers
// ============================================================

interface FieldOptions {
  id: string;
  label: string;
  value: string;
  placeholder?: string;
  hint?: string;
  required?: boolean;
  type?: string;
}

function createField(
  form: HTMLFormElement,
  opts: FieldOptions,
): HTMLInputElement {
  const row = document.createElement("div");
  row.className = "profile-editor-field";

  const label = document.createElement("label");
  label.className = "profile-editor-label";
  label.htmlFor = opts.id;
  label.textContent = opts.label;
  row.appendChild(label);

  const input = document.createElement("input");
  input.type = opts.type || "text";
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

  form.appendChild(row);
  return input;
}

function showError(errorEl: HTMLElement, message: string): void {
  errorEl.textContent = message;
  errorEl.hidden = false;
}
