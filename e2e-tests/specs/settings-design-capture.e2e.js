/**
 * Settings Panel Design Capture - Screenshots for design evaluation
 *
 * Captures screenshots of all settings categories and states
 * for Material Design 3 compliance review.
 */

describe("Settings Panel Design Capture", () => {
	beforeEach(async () => {
		await browser.pause(2000);
	});

	it("should capture Appearance category (full view)", async () => {
		// Open settings tab
		const settingsButton = await $(".tab-button-settings");
		await expect(settingsButton).toExist();
		await settingsButton.click();
		await browser.pause(1500);

		// Verify Appearance category is active by default
		const activeNav = await browser.execute(() => {
			return document.querySelector(".settings-nav-item.active")?.textContent || "";
		});
		expect(activeNav).toBe("Appearance");

		await browser.saveScreenshot("./screenshots/design-01-appearance-full.png");
	});

	it("should capture Appearance category scrolled to bottom", async () => {
		// Ensure settings tab is open
		await browser.execute(() => {
			const tabs = window.tabManager?.getTabs() || [];
			const settingsTab = tabs.find(t => t.type === "settings");
			if (settingsTab) {
				window.tabManager?.switchTab(settingsTab.id);
			} else {
				document.querySelector(".tab-button-settings")?.click();
			}
		});
		await browser.pause(1000);

		// Scroll content to bottom
		await browser.execute(() => {
			const content = document.querySelector(".settings-content");
			if (content) content.scrollTop = content.scrollHeight;
		});
		await browser.pause(500);

		await browser.saveScreenshot("./screenshots/design-02-appearance-scrolled.png");
	});

	it("should capture Terminal category", async () => {
		// Switch to Terminal category
		await browser.execute(() => {
			const tabs = window.tabManager?.getTabs() || [];
			const settingsTab = tabs.find(t => t.type === "settings");
			if (settingsTab) window.tabManager?.switchTab(settingsTab.id);
		});
		await browser.pause(500);

		// Click Terminal nav item
		await browser.execute(() => {
			const items = document.querySelectorAll(".settings-nav-item");
			for (const item of items) {
				if (item.textContent === "Terminal") {
					item.click();
					break;
				}
			}
		});
		await browser.pause(1000);

		// Scroll to top
		await browser.execute(() => {
			const content = document.querySelector(".settings-content");
			if (content) content.scrollTop = 0;
		});
		await browser.pause(300);

		await browser.saveScreenshot("./screenshots/design-03-terminal-full.png");
	});

	it("should capture Terminal category scrolled to bottom", async () => {
		// Scroll content to bottom
		await browser.execute(() => {
			const content = document.querySelector(".settings-content");
			if (content) content.scrollTop = content.scrollHeight;
		});
		await browser.pause(500);

		await browser.saveScreenshot("./screenshots/design-04-terminal-scrolled.png");
	});

	it("should capture Keybinds category", async () => {
		// Click Keybinds nav item
		await browser.execute(() => {
			const items = document.querySelectorAll(".settings-nav-item");
			for (const item of items) {
				if (item.textContent === "Keybinds") {
					item.click();
					break;
				}
			}
		});
		await browser.pause(1000);

		// Scroll to top
		await browser.execute(() => {
			const content = document.querySelector(".settings-content");
			if (content) content.scrollTop = 0;
		});
		await browser.pause(300);

		await browser.saveScreenshot("./screenshots/design-05-keybinds-full.png");
	});

	it("should capture Keybinds category scrolled to bottom", async () => {
		// Scroll content to bottom
		await browser.execute(() => {
			const content = document.querySelector(".settings-content");
			if (content) content.scrollTop = content.scrollHeight;
		});
		await browser.pause(500);

		await browser.saveScreenshot("./screenshots/design-06-keybinds-scrolled.png");
	});

	it("should capture input focus state", async () => {
		// Switch back to Appearance
		await browser.execute(() => {
			const items = document.querySelectorAll(".settings-nav-item");
			for (const item of items) {
				if (item.textContent === "Appearance") {
					item.click();
					break;
				}
			}
		});
		await browser.pause(1000);

		// Scroll to top and focus font size input
		await browser.execute(() => {
			const content = document.querySelector(".settings-content");
			if (content) content.scrollTop = 0;
			const input = document.getElementById("settings-font-size");
			if (input) input.focus();
		});
		await browser.pause(300);

		await browser.saveScreenshot("./screenshots/design-07-input-focus.png");
	});

	it("should capture toggle states", async () => {
		// Switch to Appearance, scroll to Rich Content section with toggles
		await browser.execute(() => {
			const items = document.querySelectorAll(".settings-nav-item");
			for (const item of items) {
				if (item.textContent === "Appearance") {
					item.click();
					break;
				}
			}
		});
		await browser.pause(1000);

		// Scroll to show toggles (Rich Content section is near bottom)
		await browser.execute(() => {
			const content = document.querySelector(".settings-content");
			if (content) content.scrollTop = content.scrollHeight;
		});
		await browser.pause(500);

		await browser.saveScreenshot("./screenshots/design-08-toggles.png");
	});

	it("should capture keybind capture mode", async () => {
		// Switch to Keybinds
		await browser.execute(() => {
			const items = document.querySelectorAll(".settings-nav-item");
			for (const item of items) {
				if (item.textContent === "Keybinds") {
					item.click();
					break;
				}
			}
		});
		await browser.pause(1000);

		// Click first keybind button to enter capture mode
		await browser.execute(() => {
			const content = document.querySelector(".settings-content");
			if (content) content.scrollTop = 0;
			const btn = document.querySelector(".settings-keybind-input");
			if (btn) btn.click();
		});
		await browser.pause(500);

		await browser.saveScreenshot("./screenshots/design-09-keybind-capture.png");

		// Cancel capture with Escape
		await browser.execute(() => {
			document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
		});
		await browser.pause(300);
	});
});
