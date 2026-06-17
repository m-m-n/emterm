/**
 * Tests for ImeHandler - cursor visibility aware positioning
 *
 * Tests verify that IME position calculation uses bottom-left coordinates
 * when cursorVisible === false, and cursor-following coordinates when
 * cursorVisible === true.
 *
 * Note: In the Bun/happy-dom test environment, getComputedStyle returns "0px"
 * for padding values. Tests are written with this in mind (padding=0).
 * The padding logic in the production code is verified by E2E tests.
 */

import { describe, expect, it, mock, afterEach } from "bun:test";

// Polyfill DOMRect for Bun test environment (not available in happy-dom)
if (typeof globalThis.DOMRect === "undefined") {
	(globalThis as any).DOMRect = class DOMRect {
		x: number;
		y: number;
		width: number;
		height: number;
		constructor(x = 0, y = 0, width = 0, height = 0) {
			this.x = x;
			this.y = y;
			this.width = width;
			this.height = height;
		}
		get top() {
			return this.y;
		}
		get left() {
			return this.x;
		}
		get right() {
			return this.x + this.width;
		}
		get bottom() {
			return this.y + this.height;
		}
		toJSON() {
			return {
				x: this.x,
				y: this.y,
				width: this.width,
				height: this.height,
				top: this.top,
				left: this.left,
				right: this.right,
				bottom: this.bottom,
			};
		}
	};
}

// Mock external dependencies before importing the module under test
mock.module("@tauri-apps/api/core", () => ({
	invoke: mock(async () => null),
	Resource: class Resource {
		readonly rid: number;
		constructor(rid: number) {
			this.rid = rid;
		}
		close() {
			return Promise.resolve();
		}
	},
	Channel: class Channel {},
	transformCallback: () => 0,
}));

mock.module("../../settings/settings-service", () => ({
	SettingsService: {
		load: () => Promise.resolve(null),
		save: () => Promise.resolve(),
		getCached: () => null,
	},
}));

mock.module("../../clipboard", () => ({
	ClipboardManager: {},
	showPasteDialog: mock(async () => ({ confirmed: false })),
	sendTextInChunks: mock(async () => {}),
}));

mock.module("../../shared/dom-utils", () => ({
	isModalOverlayVisible: () => false,
}));

import { ImeHandler } from "./ime";
import type { PtyClient } from "../../pty/client";
import type { TerminalState } from "../../terminal/state";

/**
 * Creates a mock PtyClient
 */
function createMockPtyClient(): PtyClient {
	return {
		write: mock(() => Promise.resolve()),
		resize: mock(() => Promise.resolve()),
		onData: mock(() => {}),
		spawn: mock(() => Promise.resolve()),
		kill: mock(() => Promise.resolve()),
	} as unknown as PtyClient;
}

/**
 * Creates a mock HTMLElement container with configurable dimensions.
 * Note: getComputedStyle in happy-dom returns "0px" for padding,
 * so padding is effectively 0 in all test assertions.
 */
function createMockContainer(options?: {
	width?: number;
	height?: number;
	rectLeft?: number;
	rectTop?: number;
}): HTMLElement {
	const width = options?.width ?? 800;
	const height = options?.height ?? 480;
	const rectLeft = options?.rectLeft ?? 0;
	const rectTop = options?.rectTop ?? 0;

	const el = document.createElement("div");

	// Mock getBoundingClientRect
	el.getBoundingClientRect = () =>
		new DOMRect(rectLeft, rectTop, width, height);

	return el;
}

// Track handlers for cleanup
let activeHandler: ImeHandler | null = null;

afterEach(() => {
	if (activeHandler) {
		activeHandler.dispose();
		activeHandler = null;
	}
	// Clean up any remaining IME elements from DOM
	document.querySelectorAll(".ime-input").forEach((el) => el.remove());
	document
		.querySelectorAll("#ime-composition-view")
		.forEach((el) => el.remove());
});

describe("ImeHandler", () => {
	describe("updatePosition (textarea mode)", () => {
		describe("when cursorVisible === true", () => {
			it("should position textarea at cursor location", () => {
				const container = createMockContainer();

				const handler = new ImeHandler({
					container,
					ptyClient: createMockPtyClient(),
					getState: () =>
						({
							cursorCol: 10,
							cursorRow: 5,
							cursorVisible: true,
						}) as unknown as TerminalState,
					charSize: { width: 8, height: 16 },
				});
				activeHandler = handler;

				handler.init();
				handler.updatePosition();

				const textarea = document.querySelector(
					"textarea.ime-input",
				) as HTMLTextAreaElement;
				expect(textarea).not.toBeNull();

				// In test env, padding=0, so:
				// x = cursorCol * charWidth + 0 = 10 * 8 = 80
				// y = cursorRow * charHeight + 0 = 5 * 16 = 80
				// left = rect.left(0) + x = 80
				// top = rect.top(0) + y = 80
				expect(textarea!.style.left).toBe("80px");
				expect(textarea!.style.top).toBe("80px");
			});
		});

		describe("when cursorVisible === false", () => {
			it("should position textarea at last row of terminal grid", () => {
				const container = createMockContainer({
					width: 800,
					height: 480,
				});

				// rows=30 (480/16), lastRow=29, y=29*16=464
				const handler = new ImeHandler({
					container,
					ptyClient: createMockPtyClient(),
					getState: () =>
						({
							cursorCol: 10,
							cursorRow: 5,
							cursorVisible: false,
							rows: 30,
						}) as unknown as TerminalState,
					charSize: { width: 8, height: 16 },
				});
				activeHandler = handler;

				handler.init();
				handler.updatePosition();

				const textarea = document.querySelector(
					"textarea.ime-input",
				) as HTMLTextAreaElement;
				expect(textarea).not.toBeNull();

				// Last row position: padding=0 in test env
				// x = paddingLeft(0) = 0
				// y = (rows-1) * charHeight = 29 * 16 = 464
				expect(textarea!.style.left).toBe("0px");
				expect(textarea!.style.top).toBe("464px");
			});

			it("should ignore cursor position when cursor is hidden", () => {
				const container = createMockContainer({
					width: 800,
					height: 480,
				});

				const handler = new ImeHandler({
					container,
					ptyClient: createMockPtyClient(),
					getState: () =>
						({
							cursorCol: 40,
							cursorRow: 12,
							cursorVisible: false,
							rows: 30,
						}) as unknown as TerminalState,
					charSize: { width: 8, height: 16 },
				});
				activeHandler = handler;

				handler.init();
				handler.updatePosition();

				const textarea = document.querySelector(
					"textarea.ime-input",
				) as HTMLTextAreaElement;
				expect(textarea).not.toBeNull();

				// Should be at last row regardless of cursor position
				// Not at cursor pos (40*8=320, 12*16=192)
				expect(textarea!.style.left).toBe("0px");
				expect(textarea!.style.top).toBe("464px");
			});
		});
	});

	describe("updateCompositionView", () => {
		describe("when cursorVisible === true", () => {
			it("should position composition view at cursor location", () => {
				const container = createMockContainer();

				const handler = new ImeHandler({
					container,
					ptyClient: createMockPtyClient(),
					getState: () =>
						({
							cursorCol: 10,
							cursorRow: 5,
							cursorVisible: true,
						}) as unknown as TerminalState,
					charSize: { width: 8, height: 16 },
				});
				activeHandler = handler;

				handler.init();

				const compositionView = document.getElementById(
					"ime-composition-view",
				);
				expect(compositionView).not.toBeNull();

				// Access private method via type assertion
				(
					handler as unknown as {
						updateCompositionView: (text: string) => void;
					}
				).updateCompositionView("test");

				// In test env, padding=0:
				// x = rect.left(0) + cursorCol(10) * charWidth(8) + 0 = 80
				// y = rect.top(0) + cursorRow(5) * charHeight(16) + 0 = 80
				expect(compositionView!.style.left).toBe("80px");
				expect(compositionView!.style.top).toBe("80px");
				expect(compositionView!.style.display).toBe("block");
			});
		});

		describe("when cursorVisible === false", () => {
			it("should position composition view at last row of terminal grid", () => {
				const container = createMockContainer({
					width: 800,
					height: 480,
				});

				const handler = new ImeHandler({
					container,
					ptyClient: createMockPtyClient(),
					getState: () =>
						({
							cursorCol: 10,
							cursorRow: 5,
							cursorVisible: false,
							rows: 30,
						}) as unknown as TerminalState,
					charSize: { width: 8, height: 16 },
				});
				activeHandler = handler;

				handler.init();

				const compositionView = document.getElementById(
					"ime-composition-view",
				);
				expect(compositionView).not.toBeNull();

				(
					handler as unknown as {
						updateCompositionView: (text: string) => void;
					}
				).updateCompositionView("test");

				// Last row: padding=0
				// x = rect.left(0) + 0 = 0
				// y = rect.top(0) + 29 * 16 = 464
				expect(compositionView!.style.left).toBe("0px");
				expect(compositionView!.style.top).toBe("464px");
				expect(compositionView!.style.display).toBe("block");
			});
		});

		describe("empty text hides composition view", () => {
			it("should hide composition view when text is empty", () => {
				const container = createMockContainer();

				const handler = new ImeHandler({
					container,
					ptyClient: createMockPtyClient(),
					getState: () =>
						({
							cursorCol: 0,
							cursorRow: 0,
							cursorVisible: false,
							rows: 30,
						}) as unknown as TerminalState,
					charSize: { width: 8, height: 16 },
				});
				activeHandler = handler;

				handler.init();

				const compositionView = document.getElementById(
					"ime-composition-view",
				);
				expect(compositionView).not.toBeNull();

				(
					handler as unknown as {
						updateCompositionView: (text: string) => void;
					}
				).updateCompositionView("");

				expect(compositionView!.style.display).toBe("none");
			});
		});
	});

	describe("cursor visibility toggle", () => {
		it("should update position when cursor visibility changes", () => {
			let cursorVisible = true;

			const container = createMockContainer({
				width: 800,
				height: 480,
			});

			const handler = new ImeHandler({
				container,
				ptyClient: createMockPtyClient(),
				getState: () =>
					({
						cursorCol: 10,
						cursorRow: 5,
						cursorVisible,
						rows: 30,
					}) as unknown as TerminalState,
				charSize: { width: 8, height: 16 },
			});
			activeHandler = handler;

			handler.init();

			// Initially cursor visible - should position at cursor
			handler.updatePosition();
			const textarea = document.querySelector(
				"textarea.ime-input",
			) as HTMLTextAreaElement;
			// padding=0: x=10*8=80, y=5*16=80
			expect(textarea!.style.left).toBe("80px");
			expect(textarea!.style.top).toBe("80px");

			// Toggle cursor to hidden - should position at last row
			cursorVisible = false;
			handler.updatePosition();
			// padding=0: x=0, y=(30-1)*16=464
			expect(textarea!.style.left).toBe("0px");
			expect(textarea!.style.top).toBe("464px");

			// Toggle cursor back to visible - should position at cursor
			cursorVisible = true;
			handler.updatePosition();
			expect(textarea!.style.left).toBe("80px");
			expect(textarea!.style.top).toBe("80px");
		});
	});
});
