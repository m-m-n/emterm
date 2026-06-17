/**
 * Profile Selector
 *
 * Modal overlay for selecting a terminal profile when creating a new tab.
 * Supports keyboard navigation (arrow keys, Enter, Escape) and mouse click.
 */

import type { Profile } from "../settings/types";
import { t } from "../i18n/index.ts";

export interface ProfileSelectorOptions {
	profiles: Profile[];
	onSelect: (profile: Profile) => void;
	onCancel: () => void;
}

/**
 * Show a profile selector modal overlay.
 * Returns a cleanup function to remove the modal.
 */
export function showProfileSelector(
	options: ProfileSelectorOptions,
): () => void {
	const { profiles, onSelect, onCancel } = options;

	// Overlay
	const overlay = document.createElement("div");
	overlay.className = "profile-selector-overlay";
	overlay.setAttribute("role", "dialog");
	overlay.setAttribute("aria-label", t("settings.profiles.title"));

	// Dialog container
	const dialog = document.createElement("div");
	dialog.className = "profile-selector-dialog";

	// Title
	const title = document.createElement("h2");
	title.className = "profile-selector-title";
	title.textContent = t("settings.profiles.title");
	dialog.appendChild(title);

	// Profile list
	const list = document.createElement("div");
	list.className = "profile-selector-list";
	list.setAttribute("role", "listbox");
	list.setAttribute("tabindex", "0");

	let activeIndex = 0;

	for (let i = 0; i < profiles.length; i++) {
		const profile = profiles[i]!;
		const item = document.createElement("div");
		item.className = "profile-selector-item";
		item.setAttribute("role", "option");
		item.setAttribute("aria-selected", i === 0 ? "true" : "false");
		item.dataset.index = String(i);

		const nameEl = document.createElement("span");
		nameEl.className = "profile-selector-item-name";
		nameEl.textContent = profile.name;
		item.appendChild(nameEl);

		if (profile.is_default) {
			const badge = document.createElement("span");
			badge.className = "profile-default-badge";
			badge.textContent = t("settings.profiles.defaultBadge");
			item.appendChild(badge);
		}

		if (profile.shell_path) {
			const shellEl = document.createElement("span");
			shellEl.className = "profile-selector-item-shell";
			shellEl.textContent = profile.shell_path;
			item.appendChild(shellEl);
		}

		item.addEventListener("click", () => {
			cleanup();
			onSelect(profile);
		});

		list.appendChild(item);
	}

	dialog.appendChild(list);
	overlay.appendChild(dialog);
	document.body.appendChild(overlay);

	// Focus list for keyboard navigation
	list.focus();

	const updateActiveItem = (newIndex: number) => {
		const items = list.querySelectorAll(".profile-selector-item");
		const prev = items[activeIndex];
		if (prev) {
			prev.classList.remove("active");
			prev.setAttribute("aria-selected", "false");
		}
		activeIndex = newIndex;
		const next = items[activeIndex];
		if (next) {
			next.classList.add("active");
			next.setAttribute("aria-selected", "true");
			(next as HTMLElement).scrollIntoView({ block: "nearest" });
		}
	};

	// Set initial active
	updateActiveItem(0);

	// Keyboard navigation
	const handleKeydown = (e: KeyboardEvent) => {
		switch (e.key) {
			case "ArrowDown":
				e.preventDefault();
				updateActiveItem((activeIndex + 1) % profiles.length);
				break;
			case "ArrowUp":
				e.preventDefault();
				updateActiveItem(
					(activeIndex - 1 + profiles.length) % profiles.length,
				);
				break;
			case "Home":
				e.preventDefault();
				updateActiveItem(0);
				break;
			case "End":
				e.preventDefault();
				updateActiveItem(profiles.length - 1);
				break;
			case "Enter":
			case " ":
				e.preventDefault();
				cleanup();
				onSelect(profiles[activeIndex]!);
				break;
			case "Escape":
				e.preventDefault();
				cleanup();
				onCancel();
				break;
		}
	};

	list.addEventListener("keydown", handleKeydown);

	// Click outside to cancel
	overlay.addEventListener("click", (e) => {
		if (e.target === overlay) {
			cleanup();
			onCancel();
		}
	});

	const cleanup = () => {
		overlay.remove();
	};

	return cleanup;
}
