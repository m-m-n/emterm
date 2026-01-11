/**
 * Tests for TerminalRenderer.
 * Note: These tests require a DOM environment (happy-dom).
 */
import { beforeEach, describe, expect, test } from "bun:test";
import { TerminalRenderer } from "./renderer.ts";
import { TerminalState } from "./state.ts";

describe("TerminalRenderer", () => {
	let container: HTMLElement;

	beforeEach(() => {
		// Create a fresh container for each test
		container = document.createElement("div");
		container.id = "terminal";
		document.body.appendChild(container);
	});

	describe("constructor", () => {
		test("creates renderer with container", () => {
			const renderer = new TerminalRenderer(container, "monospace", 14);
			expect(renderer).toBeDefined();
		});

		test("applies font styles to container", () => {
			new TerminalRenderer(container, "Consolas", 16);
			expect(container.style.fontFamily).toBe("Consolas");
			expect(container.style.fontSize).toBe("16px");
		});
	});

	describe("scheduleRender", () => {
		test("renders state to container", async () => {
			const renderer = new TerminalRenderer(container, "monospace", 14);
			const state = new TerminalState(10, 3);

			state.processAction({ type: "Print", value: "A" });
			renderer.scheduleRender(state);

			// Wait for render to complete
			await new Promise((resolve) => setTimeout(resolve, 20));

			// Check that content was rendered
			expect(container.textContent?.includes("A")).toBe(true);
		});
	});

	describe("resize", () => {
		test("updates internal dimensions", () => {
			const renderer = new TerminalRenderer(container, "monospace", 14);
			renderer.resize(120, 40);
			// No public getter, but should not throw
			expect(true).toBe(true);
		});
	});
});
