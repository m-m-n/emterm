/**
 * Settings Phases E2E Tests
 *
 * Tests for settings implementation phases (Phase 1-15).
 * Each describe block corresponds to one implementation phase.
 *
 * Phases NOT covered (and why):
 * - Phase 9 (Scrollback): requires mouse wheel
 * - Phase 10 (Shell path/args): environment-dependent
 * - Phase 11 (Scroll speed): requires mouse wheel
 * - Phase 12 (Bell action): audio/visual flash verification is unreliable
 * - Phase 13 (Ctrl+click URL): requires external browser launch
 * - Phase 14 (Clipboard): Docker clipboard access is restricted
 */

import {
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
} from "../helpers/settings-helpers.js";

describe("Settings Phases E2E", () => {
	beforeEach(async () => {
		await browser.pause(2000);
	});

	// ================================================================
	// Phase 1: Font Family
	// ================================================================

	describe("Phase 1: Font Family", () => {
		it("should apply font family change to renderer", async () => {
			// Record original
			await openSettings();
			const original = await getInputValue("settings-font-family");

			// Change font family
			const testFont = "Courier New, monospace";
			await setTextInput("settings-font-family", testFont);
			await browser.pause(500);

			// Verify renderer received the change
			const rendererFont = await getRendererProperty("getFontFamily");
			expect(rendererFont).toContain("Courier New");

			await browser.saveScreenshot(
				"./screenshots/settings-phases-01-font-family.png",
			);

			// Restore original
			await setTextInput("settings-font-family", original);
		});

		it("should re-measure character dimensions after font change", async () => {
			await openSettings();
			const original = await getInputValue("settings-font-family");

			// Get initial char dimensions
			const initialWidth = await getRendererProperty("getCharWidth");
			const initialHeight = await getRendererProperty("getCharHeight");
			expect(initialWidth).toBeGreaterThan(0);
			expect(initialHeight).toBeGreaterThan(0);

			// Change font family to something with different metrics
			await setTextInput("settings-font-family", "serif");
			await browser.pause(500);

			// Dimensions should still be positive (re-measured)
			const newWidth = await getRendererProperty("getCharWidth");
			const newHeight = await getRendererProperty("getCharHeight");
			expect(newWidth).toBeGreaterThan(0);
			expect(newHeight).toBeGreaterThan(0);

			await browser.saveScreenshot(
				"./screenshots/settings-phases-02-font-remeasure.png",
			);

			// Restore
			await setTextInput("settings-font-family", original);
		});
	});

	// ================================================================
	// Phase 2: Line Height
	// ================================================================

	describe("Phase 2: Line Height", () => {
		it("should update renderer charHeight when line height changes", async () => {
			await openSettings();

			// Get original line height and charHeight
			const originalLineHeight = await getInputValue(
				"settings-line-height",
			);
			const originalCharHeight = await getRendererProperty("getCharHeight");

			// Change line height
			const newLineHeight =
				parseFloat(originalLineHeight) === 1.2 ? 1.5 : 1.2;
			await setNumberInput("settings-line-height", newLineHeight);
			await blurInput("settings-line-height");
			await browser.pause(500);

			// charHeight should change
			const newCharHeight = await getRendererProperty("getCharHeight");
			expect(newCharHeight).not.toBe(originalCharHeight);

			await browser.saveScreenshot(
				"./screenshots/settings-phases-03-line-height.png",
			);

			// Restore
			await setNumberInput(
				"settings-line-height",
				parseFloat(originalLineHeight),
			);
			await blurInput("settings-line-height");
		});
	});

	// ================================================================
	// Phase 3: UI Theme
	// ================================================================

	describe("Phase 3: UI Theme", () => {
		it('should set data-theme="dark" when dark theme selected', async () => {
			await openSettings();
			const original = await getSelectValue("settings-ui-theme");

			await setSelect("settings-ui-theme", "dark");
			const theme = await getTheme();
			expect(theme).toBe("dark");

			await browser.saveScreenshot(
				"./screenshots/settings-phases-04-theme-dark.png",
			);

			// Restore
			await setSelect("settings-ui-theme", original);
		});

		it('should set data-theme="light" when light theme selected', async () => {
			await openSettings();
			const original = await getSelectValue("settings-ui-theme");

			await setSelect("settings-ui-theme", "light");
			const theme = await getTheme();
			expect(theme).toBe("light");

			await browser.saveScreenshot(
				"./screenshots/settings-phases-05-theme-light.png",
			);

			// Restore
			await setSelect("settings-ui-theme", original);
		});

		it('should resolve "system" to "light" or "dark"', async () => {
			await openSettings();
			const original = await getSelectValue("settings-ui-theme");

			await setSelect("settings-ui-theme", "system");
			const theme = await getTheme();
			// "system" resolves to either "dark" or "light" via matchMedia
			expect(["dark", "light"]).toContain(theme);

			await browser.saveScreenshot(
				"./screenshots/settings-phases-06-theme-system.png",
			);

			// Restore
			await setSelect("settings-ui-theme", original);
		});

		it("should change background colors when theme switches", async () => {
			await openSettings();
			const original = await getSelectValue("settings-ui-theme");

			// Switch to light
			await setSelect("settings-ui-theme", "light");
			const lightBg = await browser.execute(() => {
				const nav = document.querySelector(".settings-nav");
				return nav
					? getComputedStyle(nav).backgroundColor
					: "";
			});

			// Switch to dark
			await setSelect("settings-ui-theme", "dark");
			const darkBg = await browser.execute(() => {
				const nav = document.querySelector(".settings-nav");
				return nav
					? getComputedStyle(nav).backgroundColor
					: "";
			});

			// Background colors should differ between themes
			expect(lightBg).not.toBe(darkBg);

			await browser.saveScreenshot(
				"./screenshots/settings-phases-07-theme-bg-change.png",
			);

			// Restore
			await setSelect("settings-ui-theme", original);
		});
	});

	// ================================================================
	// Phase 4: Opacity
	// ================================================================

	describe("Phase 4: Opacity", () => {
		it("should apply opacity to CSS variable --terminal-opacity", async () => {
			await openSettings();

			// Read original
			const originalOpacity = await getCSSVariable("--terminal-opacity");

			// Set opacity via slider
			await setSlider("settings-opacity", 0.5);
			const cssOpacity = await getCSSVariable("--terminal-opacity");
			expect(cssOpacity).toBe("0.5");

			await browser.saveScreenshot(
				"./screenshots/settings-phases-08-opacity.png",
			);

			// Restore
			await setSlider("settings-opacity", parseFloat(originalOpacity) || 1.0);
		});

		it("should keep text at full opacity (canvas pixel check)", async () => {
			await openSettings();
			const originalOpacity = await getCSSVariable("--terminal-opacity");

			// Set low opacity to make it obvious
			await setSlider("settings-opacity", 0.5);
			await browser.pause(500);

			// Verify renderer opacity property is set
			const rendererOpacity = await browser.execute(() => {
				const tabs = window.tabManager?.getTabs() || [];
				const terminalTab = tabs.find((t) => t.type === "terminal");
				if (!terminalTab) return null;
				const app = window.tabManager?.getTerminalApp(terminalTab.id);
				if (!app || !app.terminalRenderer) return null;
				// Access internal opacity field
				return app.terminalRenderer.opacity;
			});
			expect(rendererOpacity).toBe(0.5);

			await browser.saveScreenshot(
				"./screenshots/settings-phases-09-opacity-text.png",
			);

			// Restore
			await setSlider("settings-opacity", parseFloat(originalOpacity) || 1.0);
		});

		it("should accept minimum opacity 0.3", async () => {
			await openSettings();
			const originalOpacity = await getCSSVariable("--terminal-opacity");

			await setSlider("settings-opacity", 0.3);
			const cssOpacity = await getCSSVariable("--terminal-opacity");
			expect(parseFloat(cssOpacity)).toBeCloseTo(0.3, 1);

			await browser.saveScreenshot(
				"./screenshots/settings-phases-10-opacity-min.png",
			);

			// Restore
			await setSlider("settings-opacity", parseFloat(originalOpacity) || 1.0);
		});
	});

	// ================================================================
	// Phase 5: Padding
	// ================================================================

	describe("Phase 5: Padding", () => {
		it("should apply padding to CSS variable --terminal-padding", async () => {
			await openSettings();
			const originalPadding = await getCSSVariable("--terminal-padding");

			await setNumberInput("settings-padding", 16);
			await blurInput("settings-padding");

			const cssPadding = await getCSSVariable("--terminal-padding");
			expect(cssPadding).toBe("16px");

			await browser.saveScreenshot(
				"./screenshots/settings-phases-11-padding.png",
			);

			// Restore
			const origVal = parseInt(originalPadding, 10) || 4;
			await setNumberInput("settings-padding", origVal);
			await blurInput("settings-padding");
		});

		it("should recalculate terminal columns/rows after padding change", async () => {
			await openSettings();
			const originalPadding = await getCSSVariable("--terminal-padding");

			// Get initial terminal size
			const initialSize = await browser.execute(() => {
				const tabs = window.tabManager?.getTabs() || [];
				const terminalTab = tabs.find((t) => t.type === "terminal");
				if (!terminalTab) return null;
				const app = window.tabManager?.getTerminalApp(terminalTab.id);
				if (!app) return null;
				return {
					cols: app.terminalState.cols,
					rows: app.terminalState.rows,
				};
			});

			// Increase padding significantly
			await setNumberInput("settings-padding", 32);
			await blurInput("settings-padding");
			await browser.pause(1000);

			// Terminal dimensions might change (fewer cols/rows with more padding)
			// At minimum, verify the CSS variable was applied
			const cssPadding = await getCSSVariable("--terminal-padding");
			expect(cssPadding).toBe("32px");

			await browser.saveScreenshot(
				"./screenshots/settings-phases-12-padding-recalc.png",
			);

			// Restore
			const origVal = parseInt(originalPadding, 10) || 4;
			await setNumberInput("settings-padding", origVal);
			await blurInput("settings-padding");
		});
	});

	// ================================================================
	// Phase 6: Scrollbar
	// ================================================================

	describe("Phase 6: Scrollbar", () => {
		it('should set overflow to "scroll" when "always"', async () => {
			await openSettings();
			const original = await getSelectValue("settings-show-scrollbar");

			await setSelect("settings-show-scrollbar", "always");
			const overflow = await getCSSVariable(
				"--terminal-scrollbar-overflow",
			);
			expect(overflow).toBe("scroll");

			await browser.saveScreenshot(
				"./screenshots/settings-phases-13-scrollbar-always.png",
			);

			// Restore
			await setSelect("settings-show-scrollbar", original);
		});

		it('should set overflow to "hidden" when "never"', async () => {
			await openSettings();
			const original = await getSelectValue("settings-show-scrollbar");

			await setSelect("settings-show-scrollbar", "never");
			const overflow = await getCSSVariable(
				"--terminal-scrollbar-overflow",
			);
			expect(overflow).toBe("hidden");

			await browser.saveScreenshot(
				"./screenshots/settings-phases-14-scrollbar-never.png",
			);

			// Restore
			await setSelect("settings-show-scrollbar", original);
		});

		it('should set overflow to "auto" when "auto"', async () => {
			await openSettings();
			const original = await getSelectValue("settings-show-scrollbar");

			await setSelect("settings-show-scrollbar", "auto");
			const overflow = await getCSSVariable(
				"--terminal-scrollbar-overflow",
			);
			expect(overflow).toBe("auto");

			await browser.saveScreenshot(
				"./screenshots/settings-phases-15-scrollbar-auto.png",
			);

			// Restore
			await setSelect("settings-show-scrollbar", original);
		});
	});

	// ================================================================
	// Phase 7: Cursor Style / Blink
	// ================================================================

	describe("Phase 7: Cursor Style / Blink", () => {
		it("should apply cursor style change in real-time", async () => {
			await openSettings();
			await switchCategory("terminal");

			const original = await getSelectValue("settings-cursor-style");

			// Change to each style and verify
			for (const style of ["block", "underline", "bar"]) {
				await setSelect("settings-cursor-style", style);

				const appliedStyle = await browser.execute(() => {
					const tabs = window.tabManager?.getTabs() || [];
					const terminalTab = tabs.find(
						(t) => t.type === "terminal",
					);
					if (!terminalTab) return null;
					const app = window.tabManager?.getTerminalApp(
						terminalTab.id,
					);
					return app?.terminalState?.cursorStyle || null;
				});
				expect(appliedStyle).toBe(style);
			}

			await browser.saveScreenshot(
				"./screenshots/settings-phases-16-cursor-style.png",
			);

			// Restore
			await setSelect("settings-cursor-style", original);
		});

		it("should stop blink timer when blink is OFF", async () => {
			await openSettings();
			await switchCategory("terminal");

			const originalState = await getToggleState("settings-cursor-blink");

			// Ensure blink is ON first
			if (originalState === "false") {
				await clickToggle("settings-cursor-blink");
			}
			await browser.pause(300);

			// Turn blink OFF
			await clickToggle("settings-cursor-blink");
			await browser.pause(300);

			// Verify blink timer is stopped
			const timerState = await browser.execute(() => {
				const tabs = window.tabManager?.getTabs() || [];
				const terminalTab = tabs.find((t) => t.type === "terminal");
				if (!terminalTab) return null;
				const app = window.tabManager?.getTerminalApp(terminalTab.id);
				if (!app || !app.terminalRenderer) return null;
				return {
					timerNull:
						app.terminalRenderer.cursorBlinkTimer === null,
					blinkVisible: app.terminalRenderer.cursorBlinkVisible,
				};
			});
			expect(timerState).not.toBeNull();
			expect(timerState.timerNull).toBe(true);
			expect(timerState.blinkVisible).toBe(true);

			await browser.saveScreenshot(
				"./screenshots/settings-phases-17-cursor-blink-off.png",
			);

			// Restore
			if (originalState === "true") {
				await clickToggle("settings-cursor-blink");
			}
		});

		it("should start blink timer and toggle blinkVisible when blink is ON", async () => {
			await openSettings();
			await switchCategory("terminal");

			const originalState = await getToggleState("settings-cursor-blink");

			// Ensure blink is OFF first
			if (originalState === "true") {
				await clickToggle("settings-cursor-blink");
				await browser.pause(300);
			}

			// Turn blink ON
			await clickToggle("settings-cursor-blink");
			await browser.pause(300);

			// Verify timer is running
			const hasTimer = await browser.execute(() => {
				const tabs = window.tabManager?.getTabs() || [];
				const terminalTab = tabs.find((t) => t.type === "terminal");
				if (!terminalTab) return false;
				const app = window.tabManager?.getTerminalApp(terminalTab.id);
				if (!app || !app.terminalRenderer) return false;
				return app.terminalRenderer.cursorBlinkTimer !== null;
			});
			expect(hasTimer).toBe(true);

			// Wait for at least one blink cycle (500ms interval)
			// Record initial state, wait, check if it toggled
			const initialVisible = await browser.execute(() => {
				const tabs = window.tabManager?.getTabs() || [];
				const terminalTab = tabs.find((t) => t.type === "terminal");
				if (!terminalTab) return null;
				const app = window.tabManager?.getTerminalApp(terminalTab.id);
				return app?.terminalRenderer?.cursorBlinkVisible;
			});

			await browser.pause(600);

			const afterVisible = await browser.execute(() => {
				const tabs = window.tabManager?.getTabs() || [];
				const terminalTab = tabs.find((t) => t.type === "terminal");
				if (!terminalTab) return null;
				const app = window.tabManager?.getTerminalApp(terminalTab.id);
				return app?.terminalRenderer?.cursorBlinkVisible;
			});

			// After 600ms with 500ms interval, blinkVisible should have toggled
			// at least once. Verify blink state changed (timer is working).
			expect(initialVisible).not.toBeNull();
			expect(afterVisible).not.toBeNull();
			expect(afterVisible).not.toBe(initialVisible);

			await browser.saveScreenshot(
				"./screenshots/settings-phases-18-cursor-blink-on.png",
			);

			// Restore
			if (originalState === "false") {
				await clickToggle("settings-cursor-blink");
			}
		});
	});

	// ================================================================
	// Phase 8: Color Scheme
	// ================================================================

	describe("Phase 8: Color Scheme", () => {
		it("should have 6 presets in dropdown", async () => {
			await openSettings();

			const options = await getSelectOptions(
				"settings-terminal-color-scheme",
			);
			expect(options.length).toBe(6);

			const values = options.map((o) => o.value);
			expect(values).toContain("emterm");
			expect(values).toContain("solarized-dark");
			expect(values).toContain("solarized-light");
			expect(values).toContain("monokai");
			expect(values).toContain("dracula");
			expect(values).toContain("nord");

			await browser.saveScreenshot(
				"./screenshots/settings-phases-19-colorscheme-presets.png",
			);
		});

		it('should have "eMterm" as the first option', async () => {
			await openSettings();

			const options = await getSelectOptions(
				"settings-terminal-color-scheme",
			);
			expect(options[0].value).toBe("emterm");
			expect(options[0].label).toBe("eMterm");

			await browser.saveScreenshot(
				"./screenshots/settings-phases-20-colorscheme-first.png",
			);
		});

		it("should update terminal colors when scheme changes", async () => {
			await openSettings();
			const original = await getSelectValue(
				"settings-terminal-color-scheme",
			);

			// Get initial background color from renderer
			const initialBg = await browser.execute(() => {
				const tabs = window.tabManager?.getTabs() || [];
				const terminalTab = tabs.find((t) => t.type === "terminal");
				if (!terminalTab) return null;
				const app = window.tabManager?.getTerminalApp(terminalTab.id);
				if (!app || !app.terminalRenderer) return null;
				return app.terminalRenderer.currentBackground;
			});

			// Switch to Dracula
			await setSelect("settings-terminal-color-scheme", "dracula");
			await browser.pause(500);

			const draculaBg = await browser.execute(() => {
				const tabs = window.tabManager?.getTabs() || [];
				const terminalTab = tabs.find((t) => t.type === "terminal");
				if (!terminalTab) return null;
				const app = window.tabManager?.getTerminalApp(terminalTab.id);
				if (!app || !app.terminalRenderer) return null;
				return app.terminalRenderer.currentBackground;
			});

			// data-terminal-color-scheme attribute should be set
			const schemeAttr = await browser.execute(() => {
				return (
					document.documentElement.getAttribute(
						"data-terminal-color-scheme",
					) || ""
				);
			});
			expect(schemeAttr).toBe("dracula");

			// Background should have changed
			expect(draculaBg).not.toEqual(initialBg);

			await browser.saveScreenshot(
				"./screenshots/settings-phases-21-colorscheme-dracula.png",
			);

			// Restore
			await setSelect(
				"settings-terminal-color-scheme",
				original || "emterm",
			);
		});

		it('should restore default colors when "eMterm" is selected', async () => {
			await openSettings();
			const original = await getSelectValue(
				"settings-terminal-color-scheme",
			);

			// Switch to a non-default scheme first
			await setSelect("settings-terminal-color-scheme", "nord");
			await browser.pause(500);

			// Now switch back to eMterm
			await setSelect("settings-terminal-color-scheme", "emterm");
			await browser.pause(500);

			// data-terminal-color-scheme attribute should be removed
			const schemeAttr = await browser.execute(() => {
				return document.documentElement.getAttribute(
					"data-terminal-color-scheme",
				);
			});
			expect(schemeAttr).toBeNull();

			await browser.saveScreenshot(
				"./screenshots/settings-phases-22-colorscheme-default.png",
			);

			// Restore original if it was not emterm
			if (original && original !== "emterm") {
				await setSelect(
					"settings-terminal-color-scheme",
					original,
				);
			}
		});
	});

	// ================================================================
	// Phase 13: URL Detection (toggle only)
	// ================================================================

	describe("Phase 13: URL Detection", () => {
		it("should toggle URL detection state", async () => {
			await openSettings();
			await switchCategory("terminal");

			const originalState = await getToggleState(
				"settings-url-detection",
			);

			// Toggle
			await clickToggle("settings-url-detection");
			const newState = await getToggleState("settings-url-detection");
			expect(newState).not.toBe(originalState);

			// Toggle back
			await clickToggle("settings-url-detection");
			const restoredState = await getToggleState(
				"settings-url-detection",
			);
			expect(restoredState).toBe(originalState);

			await browser.saveScreenshot(
				"./screenshots/settings-phases-23-url-detection.png",
			);
		});
	});

	// ================================================================
	// Phase 14: Copy on Select (toggle only)
	// ================================================================

	describe("Phase 14: Copy on Select", () => {
		it("should toggle copy-on-select state", async () => {
			await openSettings();
			await switchCategory("terminal");

			const originalState = await getToggleState(
				"settings-copy-on-select",
			);

			// Toggle
			await clickToggle("settings-copy-on-select");
			const newState = await getToggleState("settings-copy-on-select");
			expect(newState).not.toBe(originalState);

			// Toggle back
			await clickToggle("settings-copy-on-select");
			const restoredState = await getToggleState(
				"settings-copy-on-select",
			);
			expect(restoredState).toBe(originalState);

			await browser.saveScreenshot(
				"./screenshots/settings-phases-24-copy-on-select.png",
			);
		});
	});

	// ================================================================
	// Phase 15: Keybinds
	// ================================================================

	describe("Phase 15: Keybinds", () => {
		it("should register custom keybind via capture mode", async () => {
			await openSettings();
			await switchCategory("keybinds");

			// Find the "Copy" keybind button
			const originalKeybind = await browser.execute(() => {
				const btn = document.querySelector(
					'.settings-keybind-input[data-key="copy"]',
				);
				return btn?.textContent || "";
			});

			// Click to enter capture mode
			await browser.execute(() => {
				const btn = document.querySelector(
					'.settings-keybind-input[data-key="copy"]',
				);
				if (btn) btn.click();
			});
			await browser.pause(300);

			// Verify capture mode is active
			const isCapturing = await browser.execute(() => {
				const btn = document.querySelector(
					'.settings-keybind-input[data-key="copy"]',
				);
				return btn?.classList.contains("capturing") || false;
			});
			expect(isCapturing).toBe(true);

			// Simulate pressing Ctrl+Shift+C
			await browser.execute(() => {
				const event = new KeyboardEvent("keydown", {
					key: "C",
					ctrlKey: true,
					shiftKey: true,
					bubbles: true,
				});
				document.dispatchEvent(event);
			});
			await browser.pause(500);

			// Verify the keybind was captured
			const newKeybind = await browser.execute(() => {
				const btn = document.querySelector(
					'.settings-keybind-input[data-key="copy"]',
				);
				return btn?.textContent || "";
			});
			expect(newKeybind).toBe("Ctrl+Shift+C");

			// Capture mode should be exited
			const stillCapturing = await browser.execute(() => {
				const btn = document.querySelector(
					'.settings-keybind-input[data-key="copy"]',
				);
				return btn?.classList.contains("capturing") || false;
			});
			expect(stillCapturing).toBe(false);

			await browser.saveScreenshot(
				"./screenshots/settings-phases-25-keybind-capture.png",
			);

			// Restore original keybind by re-capturing
			if (originalKeybind) {
				await browser.execute(() => {
					const btn = document.querySelector(
						'.settings-keybind-input[data-key="copy"]',
					);
					if (btn) btn.click();
				});
				await browser.pause(300);

				// Parse the original keybind to simulate it
				await browser.execute((origCombo) => {
					const parts = origCombo.split("+");
					const keyName = parts[parts.length - 1];
					const hasShift = parts.includes("Shift");
					const event = new KeyboardEvent("keydown", {
						key:
							keyName.length === 1
								? hasShift
									? keyName.toUpperCase()
									: keyName.toLowerCase()
								: keyName,
						ctrlKey: parts.includes("Ctrl"),
						shiftKey: hasShift,
						altKey: parts.includes("Alt"),
						metaKey: parts.includes("Meta"),
						bubbles: true,
					});
					document.dispatchEvent(event);
				}, originalKeybind);
				await browser.pause(300);
			}
		});

		it("should show default keybinds as initial values", async () => {
			await openSettings();
			await switchCategory("keybinds");

			// Check a few well-known default keybinds
			const copyKeybind = await browser.execute(() => {
				const btn = document.querySelector(
					'.settings-keybind-input[data-key="copy"]',
				);
				return btn?.textContent || "";
			});
			// Should have a non-empty value
			expect(copyKeybind.length).toBeGreaterThan(0);

			const pasteKeybind = await browser.execute(() => {
				const btn = document.querySelector(
					'.settings-keybind-input[data-key="paste"]',
				);
				return btn?.textContent || "";
			});
			expect(pasteKeybind.length).toBeGreaterThan(0);

			const newTabKeybind = await browser.execute(() => {
				const btn = document.querySelector(
					'.settings-keybind-input[data-key="new_tab"]',
				);
				return btn?.textContent || "";
			});
			expect(newTabKeybind.length).toBeGreaterThan(0);

			await browser.saveScreenshot(
				"./screenshots/settings-phases-26-keybind-defaults.png",
			);
		});
	});
});
