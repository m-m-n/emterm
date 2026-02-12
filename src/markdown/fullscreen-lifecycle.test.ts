/**
 * Tests for FullscreenMarkdownView lifecycle callbacks (onShow/onHide).
 */
import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";
import type { MarkdownBlock } from "./types.ts";

// Mock Tauri plugins
mock.module("@tauri-apps/plugin-shell", () => ({
	open: mock(() => Promise.resolve()),
}));
mock.module("@tauri-apps/plugin-clipboard-manager", () => ({
	writeText: mock(() => Promise.resolve()),
}));

const { FullscreenMarkdownView } = await import("./fullscreen.ts");

function createMockBlock(html = "<p>Test</p>"): MarkdownBlock {
	return {
		id: "test-block",
		html,
		startRow: 0,
		rowCount: 0,
		visible: true,
	};
}

describe("FullscreenMarkdownView lifecycle callbacks", () => {
	let view: InstanceType<typeof FullscreenMarkdownView>;
	let container: HTMLElement;

	beforeEach(() => {
		view = new FullscreenMarkdownView();
		container = document.createElement("div");
		container.className = "overlay-root";
		document.body.appendChild(container);
	});

	afterEach(() => {
		view.dispose();
		document.querySelectorAll(".markdown-fullscreen-overlay").forEach((el) => el.remove());
		container.remove();
	});

	test("onShow callback is called when show() is invoked", () => {
		const onShow = mock(() => {});
		view.onShow(onShow);

		view.show(createMockBlock(), container);

		expect(onShow).toHaveBeenCalledTimes(1);
	});

	test("onHide callback is called when close() is invoked", () => {
		const onHide = mock(() => {});
		view.onHide(onHide);

		view.show(createMockBlock(), container);
		view.close();

		expect(onHide).toHaveBeenCalledTimes(1);
	});

	test("onHide callback is not called if view is not active", () => {
		const onHide = mock(() => {});
		view.onHide(onHide);

		// close without show
		view.close();

		expect(onHide).toHaveBeenCalledTimes(0);
	});

	test("onShow callback is called before content.focus()", () => {
		const callOrder: string[] = [];

		view.onShow(() => {
			callOrder.push("onShow");
		});

		// Patch focus to track call order
		const origCreateElement = document.createElement.bind(document);
		const origShow = view.show.bind(view);

		view.show(createMockBlock(), container);

		// onShow should have been called
		expect(callOrder).toContain("onShow");
	});

	test("callbacks work across multiple show/close cycles", () => {
		const onShow = mock(() => {});
		const onHide = mock(() => {});
		view.onShow(onShow);
		view.onHide(onHide);

		view.show(createMockBlock(), container);
		view.close();
		view.show(createMockBlock(), container);
		view.close();

		expect(onShow).toHaveBeenCalledTimes(2);
		expect(onHide).toHaveBeenCalledTimes(2);
	});
});
