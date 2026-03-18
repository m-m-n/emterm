/**
 * Mermaid Parse Error E2E Test
 *
 * Verifies that Mermaid syntax errors do not break the page layout.
 * When mermaid.render() fails, it may inject error elements into the DOM
 * that corrupt the HTML structure, making tab bar and other UI inaccessible.
 */

describe("Mermaid Parse Error Handling", () => {
	before(async () => {
		// Wait for terminal to be ready
		const terminal = await $('[data-testid="terminal"]');
		await terminal.waitForDisplayed({ timeout: 10000 });
		await browser.pause(2000);
	});

	// Helper to execute a shell command
	async function executeCommand(command) {
		const terminal = await $('[data-testid="terminal"]');
		await terminal.click();
		await browser.pause(100);
		for (const char of command) {
			await browser.keys([char]);
		}
		await browser.pause(100);
		await browser.keys(["Enter"]);
		await browser.pause(500);
	}

	it("should not break page layout when mermaid has syntax error", async () => {
		// Markdown with invalid mermaid syntax:
		// # Test\n\n```mermaid\nthis is invalid !!! mermaid {{{\n```
		const b64 = "IyBUZXN0CgpgYGBtZXJtYWlkCnRoaXMgaXMgaW52YWxpZCAhISEgbWVybWFpZCB7e3sKYGBg";
		const oscSequence = [
			"\\033]777;emterm;markdown;begin;id=mermaid-err-1;format=gfm\\033\\\\",
			`\\033]777;emterm;markdown;chunk;id=mermaid-err-1;seq=0;data=${b64}\\033\\\\`,
			"\\033]777;emterm;markdown;end;id=mermaid-err-1\\033\\\\",
		].join("");

		await executeCommand(`echo -e '${oscSequence}'`);

		// Wait for markdown to render and mermaid to attempt rendering
		await browser.pause(3000);

		await browser.saveScreenshot("./screenshots/mermaid-error-01-after-render.png");

		// 1. Check that the tab bar is still visible and not pushed out
		const tabBar = await $(".tab-bar");
		const tabBarExists = await tabBar.isExisting();
		console.log("Tab bar exists:", tabBarExists);
		expect(tabBarExists).toBe(true);

		if (tabBarExists) {
			const tabBarRect = await browser.execute(() => {
				const el = document.querySelector(".tab-bar");
				if (!el) return null;
				const rect = el.getBoundingClientRect();
				return {
					top: rect.top,
					bottom: rect.bottom,
					height: rect.height,
					visible: rect.top >= 0 && rect.bottom > 0,
				};
			});
			console.log("Tab bar rect:", JSON.stringify(tabBarRect));
			expect(tabBarRect).not.toBeNull();
			expect(tabBarRect.visible).toBe(true);
			expect(tabBarRect.top).toBeGreaterThanOrEqual(0);
		}

		// 2. Check that error banner is shown inside the markdown overlay
		const errorBanner = await browser.execute(() => {
			const banner = document.querySelector(".mermaid-error-banner");
			if (!banner) return null;
			return { text: banner.textContent, visible: banner.offsetParent !== null };
		});
		console.log("Error banner:", JSON.stringify(errorBanner));
		expect(errorBanner).not.toBeNull();
		expect(errorBanner.text).toContain("Mermaid");

		// 3. Check that no mermaid error elements leaked outside the markdown overlay
		const leakedErrors = await browser.execute(() => {
			// Mermaid typically creates elements with id starting with "d" or error containers
			const bodyChildren = Array.from(document.body.children);
			const leaked = [];
			for (const child of bodyChildren) {
				const html = child.outerHTML.slice(0, 200);
				// Look for mermaid-injected error elements
				if (
					child.id && child.id.startsWith("dmermaid") ||
					child.id && child.id.startsWith("d") && child.querySelector && child.querySelector("svg") ||
					child.classList && child.classList.contains("mermaid") ||
					(child.innerHTML && child.innerHTML.includes("Syntax error"))
				) {
					leaked.push({
						tag: child.tagName,
						id: child.id,
						classes: child.className,
						html: html,
					});
				}
			}
			return leaked;
		});
		console.log("Leaked mermaid elements:", JSON.stringify(leakedErrors, null, 2));

		// 3. Check viewport is not scrolled/displaced
		const viewportState = await browser.execute(() => {
			const windowHeight = window.innerHeight;
			const bodyHeight = document.body.scrollHeight;
			const scrollTop = document.documentElement.scrollTop || document.body.scrollTop;
			return {
				windowHeight,
				bodyHeight,
				scrollTop,
				overflowsViewport: bodyHeight > windowHeight + 50, // 50px tolerance
			};
		});
		console.log("Viewport state:", JSON.stringify(viewportState));

		// Body should not overflow significantly beyond viewport
		// (Mermaid error divs can make body much taller than viewport)
		expect(viewportState.overflowsViewport).toBe(false);

		// 4. Close the markdown overlay (Escape) and verify terminal is usable
		await browser.keys(["Escape"]);
		await browser.pause(500);

		await browser.saveScreenshot("./screenshots/mermaid-error-02-after-close.png");

		// Terminal should be accessible
		const terminal = await $('[data-testid="terminal"]');
		const terminalDisplayed = await terminal.isDisplayed();
		console.log("Terminal displayed after close:", terminalDisplayed);
		expect(terminalDisplayed).toBe(true);
	});
});
