/**
 * Tests for renderer factory.
 *
 * Note: CanvasRenderer integration tests require a full Canvas 2D API
 * which is not available in happy-dom. These tests verify the factory
 * function signatures and basic behavior.
 */
import { describe, expect, test } from "bun:test";
import { createRenderer, createRendererAsync } from "./renderer-factory.ts";

describe("renderer-factory", () => {
	describe("createRenderer", () => {
		// These tests require Canvas 2D context which is not available in happy-dom
		test.todo("creates a renderer instance");
		test.todo("creates CanvasRenderer");
		test.todo("renderer implements ITerminalRenderer interface");
		test.todo("dispose cleans up resources");
	});

	describe("createRendererAsync", () => {
		test.todo("creates a renderer instance asynchronously");
	});

	describe("exports", () => {
		test("createRenderer is a function", () => {
			expect(typeof createRenderer).toBe("function");
		});

		test("createRendererAsync is a function", () => {
			expect(typeof createRendererAsync).toBe("function");
		});
	});
});
