/**
 * Settings Panel Design Capture V2 - Post-Codex-review verification
 *
 * Captures screenshots after applying Codex review fixes:
 * 1. Select box arrow color -> currentColor
 * 2. Subsection spacing -> 36px
 * 3. Keybind chip min-height -> 44px
 * 4. Hex input size increase
 * 5. :focus-visible styling
 * 6. Support text contrast enhancement
 */

describe("Settings Panel Design Capture V2", () => {
	beforeEach(async () => {
		await browser.pause(2000);
	});

	it("should capture Appearance category (full view) - v2", async () => {
		// Open settings tab
		const settingsButton = await $(".tab-button-settings");
		await expect(settingsButton).toExist();
		await settingsButton.click();
		await browser.pause(1500);

		// Verify Appearance category is active by default
		const activeNav = await browser.execute(() => {
			return document.querySelector(".settings-nav-item.active")?.textContent || "";
		});
		expect(activeNav).toBe("UI Settings");

		await browser.saveScreenshot("./screenshots/v2-01-appearance-full.png");
	});

	it("should capture Appearance category scrolled to bottom - v2", async () => {
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

		await browser.saveScreenshot("./screenshots/v2-02-appearance-scrolled.png");
	});

	it("should capture Font and Theme subsection spacing - v2", async () => {
		// Ensure settings tab is open
		await browser.execute(() => {
			const tabs = window.tabManager?.getTabs() || [];
			const settingsTab = tabs.find(t => t.type === "settings");
			if (settingsTab) {
				window.tabManager?.switchTab(settingsTab.id);
			}
		});
		await browser.pause(500);

		// Scroll to show Font section and Theme & Color section boundary
		await browser.execute(() => {
			const content = document.querySelector(".settings-content");
			if (content) {
				// Find "Theme & Color" heading and scroll to position it in view
				const themeHeading = Array.from(document.querySelectorAll(".settings-subsection-title"))
					.find(h => h.textContent.includes("Theme"));
				if (themeHeading) {
					const offset = themeHeading.offsetTop - 100; // Show some content above
					content.scrollTop = offset;
				}
			}
		});
		await browser.pause(500);

		await browser.saveScreenshot("./screenshots/v2-03-subsection-spacing.png");
	});

	it("should capture Terminal category - v2", async () => {
		// Switch to Terminal category
		await browser.execute(() => {
			const tabs = window.tabManager?.getTabs() || [];
			const settingsTab = tabs.find(t => t.type === "settings");
			if (settingsTab) window.tabManager?.switchTab(settingsTab.id);
		});
		await browser.pause(500);

		// Click Terminal Appearance nav item
		await browser.execute(() => {
			const navItem = document.querySelector('.settings-nav-item[data-category-id="terminal-appearance"]');
			if (navItem) navItem.click();
		});
		await browser.pause(1000);

		// Scroll to top
		await browser.execute(() => {
			const content = document.querySelector(".settings-content");
			if (content) content.scrollTop = 0;
		});
		await browser.pause(300);

		await browser.saveScreenshot("./screenshots/v2-04-terminal-full.png");
	});

	it("should capture Keybinds category - v2", async () => {
		// Click Keybinds nav item
		await browser.execute(() => {
			const navItem = document.querySelector('.settings-nav-item[data-category-id="keybinds"]');
			if (navItem) navItem.click();
		});
		await browser.pause(1000);

		// Scroll to top
		await browser.execute(() => {
			const content = document.querySelector(".settings-content");
			if (content) content.scrollTop = 0;
		});
		await browser.pause(300);

		await browser.saveScreenshot("./screenshots/v2-05-keybinds-full.png");
	});

	it("should capture input focus state with :focus-visible - v2", async () => {
		// Switch to Terminal Appearance (font-size is there)
		await browser.execute(() => {
			const navItem = document.querySelector('.settings-nav-item[data-category-id="terminal-appearance"]');
			if (navItem) navItem.click();
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

		await browser.saveScreenshot("./screenshots/v2-06-input-focus.png");
	});

	it("should capture keybind chip min-height (44px) - v2", async () => {
		// Switch to Keybinds
		await browser.execute(() => {
			const navItem = document.querySelector('.settings-nav-item[data-category-id="keybinds"]');
			if (navItem) navItem.click();
		});
		await browser.pause(1000);

		// Scroll to top to show keybind chips
		await browser.execute(() => {
			const content = document.querySelector(".settings-content");
			if (content) content.scrollTop = 0;
		});
		await browser.pause(300);

		await browser.saveScreenshot("./screenshots/v2-07-keybind-chips.png");
	});
});
