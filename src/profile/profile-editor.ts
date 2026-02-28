/**
 * Profile Editor
 *
 * Modal dialog for creating and editing terminal profiles.
 * Provides form fields for name, shell_path, shell_args, env_vars,
 * working_directory with save/cancel actions.
 */

import type { Profile } from "../settings/types";
import { t } from "../i18n/index.ts";
import { createEmptyProfile } from "./types";

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

	// Name field
	const nameInput = createTextField(form, {
		id: "profile-name",
		label: t("settings.profiles.name"),
		value: profile.name,
		placeholder: t("settings.profiles.namePlaceholder"),
		required: true,
	});

	// Shell path field
	const shellPathInput = createTextField(form, {
		id: "profile-shell-path",
		label: t("settings.profiles.shellPath"),
		value: profile.shell_path,
		placeholder: t("settings.profiles.shellPathPlaceholder"),
		hint: t("settings.profiles.shellPathHint"),
	});

	// Shell args field
	const shellArgsInput = createTextField(form, {
		id: "profile-shell-args",
		label: t("settings.profiles.shellArgs"),
		value: profile.shell_args.join(", "),
		placeholder: t("settings.profiles.shellArgsPlaceholder"),
		hint: t("settings.profiles.shellArgsHint"),
	});

	// Env vars field (textarea)
	const envVarsTextarea = createTextareaField(form, {
		id: "profile-env-vars",
		label: t("settings.profiles.envVars"),
		value: profile.env_vars,
		placeholder: t("settings.profiles.envVarsPlaceholder"),
		hint: t("settings.profiles.envVarsHint"),
		rows: 4,
	});

	// Working directory field
	const workDirInput = createTextField(form, {
		id: "profile-working-directory",
		label: t("settings.profiles.workingDirectory"),
		value: profile.working_directory,
		placeholder: t("settings.profiles.workingDirectoryPlaceholder"),
		hint: t("settings.profiles.workingDirectoryHint"),
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

		const name = nameInput.value.trim();
		if (!name) {
			errorEl.textContent = t("settings.profiles.nameRequired");
			errorEl.hidden = false;
			nameInput.focus();
			return;
		}

		const shellArgs = shellArgsInput.value
			.split(",")
			.map((s) => s.trim())
			.filter((s) => s !== "");

		const result: Profile = {
			name,
			shell_path: shellPathInput.value.trim(),
			shell_args: shellArgs,
			env_vars: envVarsTextarea.value,
			working_directory: workDirInput.value.trim(),
			is_default: profile.is_default,
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
	form: HTMLFormElement,
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

	form.appendChild(row);
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
	form: HTMLFormElement,
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

	form.appendChild(row);
	return textarea;
}
