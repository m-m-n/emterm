/**
 * Settings Panel E2E Test - Tests settings panel functionality
 *
 * This test verifies:
 * - Gear button opens settings tab (singleton behavior)
 * - Font size input accepts valid values (8-32)
 * - Font size changes apply to terminal in real-time
 * - Settings persist after save (blur/Enter)
 * - Settings persist after app restart
 */

/** Switch to a settings category by ID */
async function switchToCategory(categoryId) {
	await browser.execute((id) => {
		const navItem = document.querySelector(`.settings-nav-item[data-category-id="${id}"]`);
		if (navItem) navItem.click();
	}, categoryId);
	await browser.pause(500);
}

/** Open settings tab and switch to Terminal Appearance category */
async function openSettingsTerminalAppearance() {
	await browser.execute(() => {
		const tabs = window.tabManager?.getTabs() || [];
		const settingsTab = tabs.find(t => t.type === "settings");
		if (settingsTab) {
			window.tabManager?.switchTab(settingsTab.id);
		} else {
			const tabBarUI = document.querySelector(".tab-button-settings");
			if (tabBarUI) tabBarUI.click();
		}
	});
	await browser.pause(1000);
	await switchToCategory("terminal-appearance");
}

describe("Settings Panel Tests", () => {
	beforeEach(async () => {
		// Wait for app to be ready
		await browser.pause(2000);
	});

	it("should open settings tab when clicking gear button", async () => {
		// Get initial tab count
		const initialCount = await browser.execute(() => {
			return window.tabManager?.getTabs().length || 0;
		});
		console.log("Initial tab count:", initialCount);

		// Find and click the settings button (gear icon)
		const settingsButton = await $(".tab-button-settings");
		await expect(settingsButton).toExist();
		console.log("Found settings button, clicking...");
		await settingsButton.click();
		await browser.pause(1000);

		// Verify settings tab was created
		const tabsAfter = await browser.execute(() => {
			const tabs = window.tabManager?.getTabs() || [];
			return tabs.map(t => ({ id: t.id, type: t.type, title: t.title }));
		});
		console.log("Tabs after click:", JSON.stringify(tabsAfter, null, 2));

		// Should have one more tab
		expect(tabsAfter.length).toBe(initialCount + 1);

		// The new tab should be of type "settings"
		const settingsTab = tabsAfter.find(t => t.type === "settings");
		expect(settingsTab).toBeDefined();
		expect(settingsTab.title).toBe("Settings");

		await browser.saveScreenshot("./screenshots/settings-01-tab-opened.png");
	});

	it("should show singleton behavior - clicking gear again switches to existing settings tab", async () => {
		// Ensure settings tab exists
		let settingsTabExists = await browser.execute(() => {
			const tabs = window.tabManager?.getTabs() || [];
			return tabs.some(t => t.type === "settings");
		});

		if (!settingsTabExists) {
			// Open settings tab first
			const settingsButton = await $(".tab-button-settings");
			await settingsButton.click();
			await browser.pause(1000);
		}

		// Get current tab count
		const countBefore = await browser.execute(() => {
			return window.tabManager?.getTabs().length || 0;
		});
		console.log("Tab count before second click:", countBefore);

		// Switch to a terminal tab first (if exists)
		await browser.execute(() => {
			const tabs = window.tabManager?.getTabs() || [];
			const terminalTab = tabs.find(t => t.type === "terminal");
			if (terminalTab) {
				window.tabManager?.switchTab(terminalTab.id);
			}
		});
		await browser.pause(500);

		// Click gear button again
		const settingsButton = await $(".tab-button-settings");
		await settingsButton.click();
		await browser.pause(500);

		// Tab count should NOT increase (singleton pattern)
		const countAfter = await browser.execute(() => {
			return window.tabManager?.getTabs().length || 0;
		});
		console.log("Tab count after second click:", countAfter);

		expect(countAfter).toBe(countBefore);

		// Active tab should be the settings tab
		const activeTab = await browser.execute(() => {
			return window.tabManager?.getActiveTab();
		});
		expect(activeTab.type).toBe("settings");

		await browser.saveScreenshot("./screenshots/settings-02-singleton.png");
	});

	it("should display font size input with current value", async () => {
		await openSettingsTerminalAppearance();

		// Find font size input
		const fontSizeInput = await $("#settings-font-size");
		await expect(fontSizeInput).toExist();

		// Get current value
		const currentValue = await fontSizeInput.getValue();
		console.log("Current font size value:", currentValue);

		// Value should be a number in valid range (8-32)
		const numValue = parseInt(currentValue, 10);
		expect(numValue).toBeGreaterThanOrEqual(8);
		expect(numValue).toBeLessThanOrEqual(32);

		// Check hint text shows range
		const hint = await $(".settings-hint");
		const hintText = await hint.getText();
		expect(hintText).toContain("8-32");

		await browser.saveScreenshot("./screenshots/settings-03-font-input.png");
	});

	it("should apply font size change to terminal in real-time", async () => {
		await openSettingsTerminalAppearance();

		// Get font size input
		const fontSizeInput = await $("#settings-font-size");
		const originalValue = await fontSizeInput.getValue();
		console.log("Original font size:", originalValue);

		// Calculate new value (toggle between 13 and 16)
		const originalNum = parseInt(originalValue, 10);
		const newValue = originalNum === 13 ? 16 : 13;
		console.log("Setting new font size:", newValue);

		// Clear and set new value with manual input event dispatch
		await fontSizeInput.click();
		await browser.keys(["Control", "a"]);
		await browser.pause(100);
		await fontSizeInput.setValue(newValue.toString());
		await browser.pause(100);

		// Manually dispatch input event (WebDriver setValue may not trigger it in Tauri WebView)
		await browser.execute((newVal) => {
			const input = document.getElementById("settings-font-size");
			if (input) {
				input.value = newVal.toString();
				input.dispatchEvent(new Event("input", { bubbles: true }));
			}
		}, newValue);
		await browser.pause(500);

		// Verify CSS variable changed
		const cssVarValue = await browser.execute(() => {
			return getComputedStyle(document.documentElement)
				.getPropertyValue("--terminal-font-size")
				.trim();
		});
		console.log("CSS variable --terminal-font-size:", cssVarValue);

		expect(cssVarValue).toBe(`${newValue}pt`);

		await browser.saveScreenshot("./screenshots/settings-04-font-changed.png");

		// Restore original value for subsequent tests
		await browser.execute((origVal) => {
			const input = document.getElementById("settings-font-size");
			if (input) {
				input.value = origVal.toString();
				input.dispatchEvent(new Event("input", { bubbles: true }));
				input.blur();
			}
		}, parseInt(originalValue, 10));
		await browser.pause(500);
	});

	it("should clamp font size to valid range", async () => {
		await openSettingsTerminalAppearance();

		const fontSizeInput = await $("#settings-font-size");
		const originalValue = await fontSizeInput.getValue();
		console.log("Original font size:", originalValue);

		// Test value below minimum (7)
		console.log("Testing value below minimum (7)...");
		await browser.execute(() => {
			const input = document.getElementById("settings-font-size");
			if (input) {
				input.value = "7";
				input.dispatchEvent(new Event("input", { bubbles: true }));
			}
		});
		await browser.pause(200);

		// Trigger blur to save (which clamps the value)
		await browser.execute(() => {
			const input = document.getElementById("settings-font-size");
			if (input) {
				// Dispatch blur event properly
				input.dispatchEvent(new FocusEvent("blur", { bubbles: true }));
			}
		});
		// Wait longer for async handleFontSizeSave to complete
		await browser.pause(1000);

		// Value should be clamped to 8 - get value directly from DOM
		const clampedLow = await browser.execute(() => {
			return document.getElementById("settings-font-size")?.value || "";
		});
		console.log("Value after setting 7 and blur:", clampedLow);
		expect(parseInt(clampedLow, 10)).toBe(8);

		// Test value above maximum (50)
		console.log("Testing value above maximum (50)...");
		await browser.execute(() => {
			const input = document.getElementById("settings-font-size");
			if (input) {
				input.value = "50";
				input.dispatchEvent(new Event("input", { bubbles: true }));
			}
		});
		await browser.pause(200);

		// Trigger blur to save (which clamps the value)
		await browser.execute(() => {
			const input = document.getElementById("settings-font-size");
			if (input) {
				// Dispatch blur event properly
				input.dispatchEvent(new FocusEvent("blur", { bubbles: true }));
			}
		});
		// Wait longer for async handleFontSizeSave to complete
		await browser.pause(1000);

		// Value should be clamped to 32 - get value directly from DOM
		const clampedHigh = await browser.execute(() => {
			return document.getElementById("settings-font-size")?.value || "";
		});
		console.log("Value after setting 50 and blur:", clampedHigh);
		expect(parseInt(clampedHigh, 10)).toBe(32);

		await browser.saveScreenshot("./screenshots/settings-05-clamped.png");

		// Restore original value
		await browser.execute((origVal) => {
			const input = document.getElementById("settings-font-size");
			if (input) {
				input.value = origVal.toString();
				input.dispatchEvent(new Event("input", { bubbles: true }));
				input.blur();
			}
		}, parseInt(originalValue, 10));
		await browser.pause(500);
	});

	it("should save settings on blur", async () => {
		await openSettingsTerminalAppearance();

		const fontSizeInput = await $("#settings-font-size");
		const originalValue = await fontSizeInput.getValue();
		const testValue = parseInt(originalValue, 10) === 13 ? 14 : 13;

		console.log(`Testing save on blur: ${originalValue} -> ${testValue}`);

		// Change value with manual event dispatch
		await browser.execute((newVal) => {
			const input = document.getElementById("settings-font-size");
			if (input) {
				input.value = newVal.toString();
				input.dispatchEvent(new Event("input", { bubbles: true }));
			}
		}, testValue);
		await browser.pause(200);

		// Trigger blur to save
		await browser.execute(() => {
			const input = document.getElementById("settings-font-size");
			if (input) input.blur();
		});
		await browser.pause(500);

		// Verify CSS variable was updated (indicating settings were applied)
		const cssVarValue = await browser.execute(() => {
			return getComputedStyle(document.documentElement)
				.getPropertyValue("--terminal-font-size")
				.trim();
		});
		console.log("CSS variable after blur:", cssVarValue);

		expect(cssVarValue).toBe(`${testValue}pt`);

		await browser.saveScreenshot("./screenshots/settings-06-saved-on-blur.png");

		// Restore original value
		await browser.execute((origVal) => {
			const input = document.getElementById("settings-font-size");
			if (input) {
				input.value = origVal.toString();
				input.dispatchEvent(new Event("input", { bubbles: true }));
				input.blur();
			}
		}, parseInt(originalValue, 10));
		await browser.pause(500);
	});

	it("should save settings on Enter key", async () => {
		await openSettingsTerminalAppearance();

		const fontSizeInput = await $("#settings-font-size");
		const originalValue = await fontSizeInput.getValue();
		const testValue = parseInt(originalValue, 10) === 15 ? 16 : 15;

		console.log(`Testing save on Enter: ${originalValue} -> ${testValue}`);

		// Change value with manual event dispatch
		await browser.execute((newVal) => {
			const input = document.getElementById("settings-font-size");
			if (input) {
				input.value = newVal.toString();
				input.dispatchEvent(new Event("input", { bubbles: true }));
			}
		}, testValue);
		await browser.pause(200);

		// Simulate Enter keypress
		await browser.execute(() => {
			const input = document.getElementById("settings-font-size");
			if (input) {
				input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
			}
		});
		await browser.pause(500);

		// Verify CSS variable was updated (indicating settings were applied)
		const cssVarValue = await browser.execute(() => {
			return getComputedStyle(document.documentElement)
				.getPropertyValue("--terminal-font-size")
				.trim();
		});
		console.log("CSS variable after Enter:", cssVarValue);

		expect(cssVarValue).toBe(`${testValue}pt`);

		await browser.saveScreenshot("./screenshots/settings-07-saved-on-enter.png");

		// Restore original value
		await browser.execute((origVal) => {
			const input = document.getElementById("settings-font-size");
			if (input) {
				input.value = origVal.toString();
				input.dispatchEvent(new Event("input", { bubbles: true }));
				input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
			}
		}, parseInt(originalValue, 10));
		await browser.pause(500);
	});

	it("should update terminal renderer when font size changes", async () => {
		// Ensure we have a terminal tab and it's active first
		await browser.execute(() => {
			const tabs = window.tabManager?.getTabs() || [];
			let terminalTab = tabs.find(t => t.type === "terminal");
			if (!terminalTab) {
				// This shouldn't happen as initial tab is terminal
				return;
			}
			window.tabManager?.switchTab(terminalTab.id);
		});
		await browser.pause(500);

		// Get initial renderer font size from terminal
		const initialFontSize = await browser.execute(() => {
			const activeTab = window.tabManager?.getActiveTab();
			if (!activeTab) return null;
			const app = window.tabManager?.getTerminalApp(activeTab.id);
			if (!app) return null;
			return app.terminalRenderer?.getFontSize();
		});
		console.log("Initial terminal renderer font size:", initialFontSize);

		// Open settings and switch to Terminal Appearance
		const settingsButton = await $(".tab-button-settings");
		await settingsButton.click();
		await browser.pause(1000);
		await switchToCategory("terminal-appearance");

		const fontSizeInput = await $("#settings-font-size");
		const originalValue = await fontSizeInput.getValue();
		const newValue = parseInt(originalValue, 10) === 13 ? 18 : 13;

		console.log(`Changing font size: ${originalValue} -> ${newValue}`);

		// Change value with manual event dispatch and trigger blur
		await browser.execute((newVal) => {
			const input = document.getElementById("settings-font-size");
			if (input) {
				input.value = newVal.toString();
				input.dispatchEvent(new Event("input", { bubbles: true }));
				input.blur();
			}
		}, newValue);
		await browser.pause(500);

		// Switch to terminal tab to verify renderer was updated
		await browser.execute(() => {
			const tabs = window.tabManager?.getTabs() || [];
			const terminalTab = tabs.find(t => t.type === "terminal");
			if (terminalTab) {
				window.tabManager?.switchTab(terminalTab.id);
			}
		});
		await browser.pause(500);

		// Check renderer font size
		const updatedFontSize = await browser.execute(() => {
			const activeTab = window.tabManager?.getActiveTab();
			if (!activeTab || activeTab.type !== "terminal") return null;
			const app = window.tabManager?.getTerminalApp(activeTab.id);
			if (!app) return null;
			return app.terminalRenderer?.getFontSize();
		});
		console.log("Updated terminal renderer font size:", updatedFontSize);

		expect(updatedFontSize).toBe(newValue);

		await browser.saveScreenshot("./screenshots/settings-08-renderer-updated.png");

		// Restore original value
		await settingsButton.click();
		await browser.pause(500);
		await browser.execute((origVal) => {
			const input = document.getElementById("settings-font-size");
			if (input) {
				input.value = origVal.toString();
				input.dispatchEvent(new Event("input", { bubbles: true }));
				input.blur();
			}
		}, parseInt(originalValue, 10));
		await browser.pause(500);
	});

	it("should display category navigation with correct items", async () => {
		// Open settings tab
		await browser.execute(() => {
			const tabs = window.tabManager?.getTabs() || [];
			const settingsTab = tabs.find(t => t.type === "settings");
			if (settingsTab) {
				window.tabManager?.switchTab(settingsTab.id);
			} else {
				const tabBarUI = document.querySelector(".tab-button-settings");
				if (tabBarUI) tabBarUI.click();
			}
		});
		await browser.pause(1000);

		// Check navigation items using browser.execute for more reliable DOM access
		const navData = await browser.execute(() => {
			const items = document.querySelectorAll(".settings-nav-item");
			const texts = [];
			const activeText = document.querySelector(".settings-nav-item.active")?.textContent || "";
			const disabledTexts = [];

			items.forEach(item => {
				texts.push(item.textContent || "");
				if (item.classList.contains("disabled")) {
					disabledTexts.push(item.textContent || "");
				}
			});

			return { texts, activeText, disabledTexts };
		});

		console.log("Navigation items:", navData.texts);
		console.log("Active nav:", navData.activeText);
		console.log("Disabled navs:", navData.disabledTexts);

		expect(navData.texts).toContain("UI Settings");
		expect(navData.texts).toContain("Terminal Appearance");
		expect(navData.texts).toContain("Terminal Behavior");
		expect(navData.texts).toContain("Keybinds");

		// Some category should be active
		expect(navData.activeText.length).toBeGreaterThan(0);

		await browser.saveScreenshot("./screenshots/settings-09-navigation.png");
	});
});
