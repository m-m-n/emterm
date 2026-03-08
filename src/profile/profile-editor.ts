/**
 * Profile Editor
 *
 * Modal dialog for creating and editing terminal profiles.
 * Provides SHELL/SSH tab UI for mutually exclusive shell vs SSH configuration.
 */

import type { AppSettings, Profile } from "../settings/types";
import { t } from "../i18n/index.ts";
import { createEmptyProfile } from "./types";
import { SettingsService } from "../settings/settings-service";
import { createMd3Select, type Md3SelectOption } from "../components/md3-select";

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

	tabBar.appendChild(shellTab);
	tabBar.appendChild(sshTab);
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

	// Tab switching logic
	let activeTab: "shell" | "ssh" = "shell";

	function switchTab(target: "shell" | "ssh") {
		if (target === activeTab) return;
		if (target === "ssh" && sshTab.classList.contains("disabled")) return;

		activeTab = target;

		if (target === "shell") {
			shellTab.classList.add("active");
			shellTab.setAttribute("aria-selected", "true");
			shellTab.tabIndex = 0;
			sshTab.classList.remove("active");
			sshTab.setAttribute("aria-selected", "false");
			sshTab.tabIndex = -1;
			shellPanel.hidden = false;
			sshPanel.hidden = true;
			// Clear SSH value
			sshSelect.value = "";
		} else {
			sshTab.classList.add("active");
			sshTab.setAttribute("aria-selected", "true");
			sshTab.tabIndex = 0;
			shellTab.classList.remove("active");
			shellTab.setAttribute("aria-selected", "false");
			shellTab.tabIndex = -1;
			sshPanel.hidden = false;
			shellPanel.hidden = true;
			// Clear shell values
			shellPathInput.value = "";
			shellArgsInput.value = "";
			envVarsTextarea.value = "";
			workDirInput.value = "";
		}
	}

	shellTab.addEventListener("click", () => switchTab("shell"));
	sshTab.addEventListener("click", () => switchTab("ssh"));

	// Keyboard navigation (arrow keys between tabs)
	tabBar.addEventListener("keydown", (e) => {
		if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
			e.preventDefault();
			const target = activeTab === "shell" ? "ssh" : "shell";
			switchTab(target);
			if (target === "shell") shellTab.focus();
			else sshTab.focus();
		}
	});

	// Load SSH connections and determine initial tab
	SettingsService.load()
		.then((settings: AppSettings) => {
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
				// Disable SSH tab
				sshTab.classList.add("disabled");
				sshTab.setAttribute("aria-disabled", "true");
				sshTab.title = t("settings.profiles.sshTabDisabled");
			} else if (profile.ssh_connection_name) {
				// Auto-select SSH tab for existing SSH profiles
				switchTab("ssh");
			}
		})
		.catch(() => {
			// Settings load failed - disable SSH tab
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
		} else {
			// SSH tab: use SSH connection name as profile name
			name = sshSelect.value;
			if (!name) {
				errorEl.textContent = t("settings.profiles.sshConnectionRequired");
				errorEl.hidden = false;
				sshSelect.focus();
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
			working_directory:
				activeTab === "shell" ? workDirInput.value.trim() : "",
			is_default: profile.is_default,
			ssh_connection_name: activeTab === "ssh" ? sshSelect.value : "",
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
	appendChild(opt: { value: string; textContent: string | null; selected?: boolean }): void;
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

	const currentOptions: Md3SelectOption[] = opts.options.map(o => ({
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
		get value() { return md3.getValue(); },
		set value(v: string) { md3.setValue(v); },
		focus() {
			const trigger = md3.element.querySelector(".md3-select-trigger") as HTMLElement | null;
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
