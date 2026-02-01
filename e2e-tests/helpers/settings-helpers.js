/**
 * Settings E2E Test Helpers
 *
 * Shared helper functions for operating on the settings panel.
 * All functions use browser.execute() for reliable Tauri WebView interaction.
 */

/**
 * Open the settings tab (idempotent).
 * If a settings tab already exists, switches to it.
 */
async function openSettings() {
	const opened = await browser.execute(() => {
		const tabs = window.tabManager?.getTabs() || [];
		const settingsTab = tabs.find((t) => t.type === "settings");
		if (settingsTab) {
			window.tabManager?.switchTab(settingsTab.id);
			return true;
		}
		return false;
	});

	if (!opened) {
		const settingsButton = await $(".tab-button-settings");
		await settingsButton.click();
	}
	await browser.pause(1000);
}

/**
 * Switch settings category.
 * @param {string} id - Category ID: "appearance", "terminal", or "keybinds"
 */
async function switchCategory(id) {
	const clicked = await browser.execute((categoryId) => {
		const navItem = document.querySelector(
			`.settings-nav-item[data-category-id="${categoryId}"]`,
		);
		if (!navItem) return "not_found";
		if (navItem.classList.contains("disabled")) return "disabled";
		navItem.click();
		return "ok";
	}, id);
	if (clicked === "not_found") {
		throw new Error(`Settings category "${id}" not found`);
	}
	if (clicked === "disabled") {
		throw new Error(`Settings category "${id}" is disabled`);
	}
	await browser.pause(500);
}

/**
 * Set a number input value and dispatch input event.
 * @param {string} id - Element ID (e.g., "settings-font-size")
 * @param {number} value - Value to set
 */
async function setNumberInput(id, value) {
	await browser.execute(
		(elId, val) => {
			const input = document.getElementById(elId);
			if (input) {
				input.value = String(val);
				input.dispatchEvent(new Event("input", { bubbles: true }));
			}
		},
		id,
		value,
	);
	await browser.pause(200);
}

/**
 * Trigger blur event to save a setting.
 * @param {string} id - Element ID
 */
async function blurInput(id) {
	await browser.execute((elId) => {
		const input = document.getElementById(elId);
		if (input) {
			input.dispatchEvent(new FocusEvent("blur", { bubbles: true }));
		}
	}, id);
	await browser.pause(500);
}

/**
 * Set a text input value and blur to save.
 * @param {string} id - Element ID (e.g., "settings-font-family")
 * @param {string} value - Text value to set
 */
async function setTextInput(id, value) {
	await browser.execute(
		(elId, val) => {
			const input = document.getElementById(elId);
			if (input) {
				input.value = val;
				input.dispatchEvent(new Event("input", { bubbles: true }));
				input.dispatchEvent(new FocusEvent("blur", { bubbles: true }));
			}
		},
		id,
		value,
	);
	await browser.pause(500);
}

/**
 * Set a select element's value and dispatch change event.
 * @param {string} id - Element ID (e.g., "settings-ui-theme")
 * @param {string} value - Option value to select
 */
async function setSelect(id, value) {
	await browser.execute(
		(elId, val) => {
			const select = document.getElementById(elId);
			if (select) {
				select.value = val;
				select.dispatchEvent(new Event("change", { bubbles: true }));
			}
		},
		id,
		value,
	);
	await browser.pause(500);
}

/**
 * Click a toggle button.
 * @param {string} id - Element ID (e.g., "settings-cursor-blink")
 */
async function clickToggle(id) {
	await browser.execute((elId) => {
		const button = document.getElementById(elId);
		if (button) {
			button.click();
		}
	}, id);
	await browser.pause(300);
}

/**
 * Get toggle state (aria-checked).
 * @param {string} id - Element ID
 * @returns {Promise<string>} "true" or "false"
 */
async function getToggleState(id) {
	const result = await browser.execute((elId) => {
		const button = document.getElementById(elId);
		if (!button) return null;
		return button.getAttribute("aria-checked") || "false";
	}, id);
	if (result === null) {
		throw new Error(`Toggle element "${id}" not found`);
	}
	return result;
}

/**
 * Set a slider (range input) value and dispatch input + change events.
 * @param {string} id - Element ID (e.g., "settings-opacity")
 * @param {number} value - Value to set
 */
async function setSlider(id, value) {
	await browser.execute(
		(elId, val) => {
			const input = document.getElementById(elId);
			if (input) {
				// Use nativeInputValueSetter to ensure value change triggers events reliably
				const nativeSetter = Object.getOwnPropertyDescriptor(
					window.HTMLInputElement.prototype,
					"value",
				)?.set;
				if (nativeSetter) {
					nativeSetter.call(input, String(val));
				} else {
					input.value = String(val);
				}
				input.dispatchEvent(new Event("input", { bubbles: true }));
				input.dispatchEvent(new Event("change", { bubbles: true }));
			}
		},
		id,
		value,
	);
	await browser.pause(500);
}

/**
 * Get a CSS variable value from :root.
 * @param {string} name - CSS variable name (e.g., "--terminal-opacity")
 * @returns {Promise<string>} Trimmed CSS variable value
 */
async function getCSSVariable(name) {
	return browser.execute((varName) => {
		return getComputedStyle(document.documentElement)
			.getPropertyValue(varName)
			.trim();
	}, name);
}

/**
 * Get the current data-theme attribute.
 * @returns {Promise<string>} Theme value ("dark", "light", or "")
 */
async function getTheme() {
	return browser.execute(() => {
		return document.documentElement.getAttribute("data-theme") || "";
	});
}

/**
 * Get an input element's current value.
 * @param {string} id - Element ID
 * @returns {Promise<string>} Input value
 */
async function getInputValue(id) {
	return browser.execute((elId) => {
		return document.getElementById(elId)?.value || "";
	}, id);
}

/**
 * Get a terminal renderer property from the first terminal tab.
 * @param {string} prop - Property name (e.g., "getCharHeight", "getFontSize")
 * @returns {Promise<any>} Property value
 */
async function getRendererProperty(prop) {
	return browser.execute((propName) => {
		const tabs = window.tabManager?.getTabs() || [];
		const terminalTab = tabs.find((t) => t.type === "terminal");
		if (!terminalTab) return null;
		const app = window.tabManager?.getTerminalApp(terminalTab.id);
		if (!app || !app.terminalRenderer) return null;
		const fn = app.terminalRenderer[propName];
		if (typeof fn === "function") {
			return fn.call(app.terminalRenderer);
		}
		return app.terminalRenderer[propName];
	}, prop);
}

/**
 * Get select element options as array of {value, label}.
 * @param {string} id - Select element ID
 * @returns {Promise<Array<{value: string, label: string}>>}
 */
async function getSelectOptions(id) {
	return browser.execute((elId) => {
		const select = document.getElementById(elId);
		if (!select) return [];
		return Array.from(select.options).map((opt) => ({
			value: opt.value,
			label: opt.textContent || "",
		}));
	}, id);
}

/**
 * Get select element's current value.
 * @param {string} id - Select element ID
 * @returns {Promise<string>} Current value
 */
async function getSelectValue(id) {
	return browser.execute((elId) => {
		return document.getElementById(elId)?.value || "";
	}, id);
}

export {
	openSettings,
	switchCategory,
	setNumberInput,
	blurInput,
	setTextInput,
	setSelect,
	clickToggle,
	getToggleState,
	setSlider,
	getCSSVariable,
	getTheme,
	getInputValue,
	getRendererProperty,
	getSelectOptions,
	getSelectValue,
};
