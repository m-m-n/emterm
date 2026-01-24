/**
 * Tests for renderer factory.
 */
import { beforeEach, describe, expect, test } from "bun:test";
import { createRenderer, getRendererType } from "./renderer-factory.ts";
import type { ITerminalRenderer, RendererType } from "./renderer-interface.ts";

describe("renderer-factory", () => {
	describe("getRendererType", () => {
		test("returns 'dom' by default", () => {
			const type = getRendererType();
			// Default is DOM renderer
			expect(type).toBe("dom");
		});

		test("returns valid RendererType", () => {
			const type = getRendererType();
			expect(type === "dom" || type === "canvas").toBe(true);
		});
	});

	describe("createRenderer", () => {
		let container: HTMLElement;

		beforeEach(() => {
			container = document.createElement("div");
			container.style.width = "800px";
			container.style.height = "600px";
			document.body.appendChild(container);
		});

		test("creates a renderer instance", () => {
			const renderer = createRenderer(container, "monospace", 13, "dom");
			expect(renderer).toBeDefined();
			expect(typeof renderer.scheduleRender).toBe("function");
			expect(typeof renderer.forceRender).toBe("function");
			expect(typeof renderer.resize).toBe("function");
		});

		test("creates DOM renderer when type is 'dom'", () => {
			const renderer = createRenderer(container, "monospace", 13, "dom");
			expect(renderer).toBeDefined();
			expect(renderer.getFontFamily()).toBe("monospace");
		});

		test("renderer implements ITerminalRenderer interface", () => {
			const renderer = createRenderer(container, "monospace", 13, "dom");

			// Check all interface methods exist
			expect(typeof renderer.scheduleRender).toBe("function");
			expect(typeof renderer.forceRender).toBe("function");
			expect(typeof renderer.resize).toBe("function");
			expect(typeof renderer.renderSelection).toBe("function");
			expect(typeof renderer.clearSelectionHighlight).toBe("function");
			expect(typeof renderer.getCharWidth).toBe("function");
			expect(typeof renderer.getCharHeight).toBe("function");
			expect(typeof renderer.getFontFamily).toBe("function");
			expect(typeof renderer.getFontSize).toBe("function");
			expect(typeof renderer.dispose).toBe("function");
		});

		test("dispose cleans up resources", () => {
			const renderer = createRenderer(container, "monospace", 13, "dom");
			// Should not throw
			renderer.dispose();
		});
	});
});
