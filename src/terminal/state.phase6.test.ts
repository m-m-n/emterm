/**
 * Tests for TerminalState Phase 6 - OSC and Device Status Reports.
 */
import { describe, expect, test } from "bun:test";
import type {
	CsiAction,
	OscAction,
	TerminalAction,
} from "../types/terminal.ts";
import { TerminalState } from "./state.ts";

describe("TerminalState Phase 6", () => {
	describe("OSC - Window Title", () => {
		test("OSC 2 sets window title", () => {
			const state = new TerminalState(80, 24);
			const action: TerminalAction = {
				type: "Osc",
				value: { action: "SetTitle", data: "My Terminal" },
			};
			state.processAction(action);

			expect(state.title).toBe("My Terminal");
		});

		test("OSC 0 sets title and icon name", () => {
			const state = new TerminalState(80, 24);
			const action: TerminalAction = {
				type: "Osc",
				value: { action: "SetTitleAndIcon", data: "Terminal Title" },
			};
			state.processAction(action);

			expect(state.title).toBe("Terminal Title");
			expect(state.iconName).toBe("Terminal Title");
		});

		test("OSC 1 sets icon name only", () => {
			const state = new TerminalState(80, 24);
			state.processAction({
				type: "Osc",
				value: { action: "SetTitle", data: "Title" },
			});
			state.processAction({
				type: "Osc",
				value: { action: "SetIconName", data: "Icon" },
			});

			expect(state.title).toBe("Title");
			expect(state.iconName).toBe("Icon");
		});
	});

	describe("OSC - Working Directory", () => {
		test("OSC 7 sets working directory", () => {
			const state = new TerminalState(80, 24);
			const action: TerminalAction = {
				type: "Osc",
				value: { action: "SetWorkingDirectory", data: "file:///home/user" },
			};
			state.processAction(action);

			expect(state.workingDirectory).toBe("file:///home/user");
		});
	});

	describe("OSC - Hyperlink", () => {
		test("OSC 8 starts hyperlink", () => {
			const state = new TerminalState(80, 24);
			const action: TerminalAction = {
				type: "Osc",
				value: {
					action: "Hyperlink",
					params: "id=1",
					uri: "https://example.com",
				},
			};
			state.processAction(action);

			expect(state.activeHyperlink).toEqual({
				params: "id=1",
				uri: "https://example.com",
			});
		});

		test("OSC 8 with empty URI ends hyperlink", () => {
			const state = new TerminalState(80, 24);
			state.processAction({
				type: "Osc",
				value: {
					action: "Hyperlink",
					params: "id=1",
					uri: "https://example.com",
				},
			});
			state.processAction({
				type: "Osc",
				value: { action: "Hyperlink", params: "", uri: "" },
			});

			expect(state.activeHyperlink).toBeNull();
		});
	});

	describe("Device Status Report", () => {
		test("DSR 5 returns device OK response", () => {
			const state = new TerminalState(80, 24);
			const action: TerminalAction = {
				type: "Csi",
				value: { action: "DeviceStatusReport", data: 5 },
			};
			state.processAction(action);

			const response = state.takePendingResponse();
			expect(response).not.toBeNull();

			// CSI 0 n = ESC [ 0 n = 0x1b 0x5b 0x30 0x6e
			const expected = new Uint8Array([0x1b, 0x5b, 0x30, 0x6e]);
			expect(response).toEqual(expected);
		});

		test("DSR 6 returns cursor position", () => {
			const state = new TerminalState(80, 24);

			// Move cursor to row 10, col 20 (0-indexed: 9, 19)
			state.processAction({
				type: "Csi",
				value: { action: "CursorPosition", data: { row: 10, col: 20 } },
			});

			// Request cursor position
			state.processAction({
				type: "Csi",
				value: { action: "DeviceStatusReport", data: 6 },
			});

			const response = state.takePendingResponse();
			expect(response).not.toBeNull();

			// Response should be CSI row ; col R
			// Row and col are 1-indexed, so 10;20R
			const responseStr = new TextDecoder().decode(response!);
			expect(responseStr).toBe("\x1b[10;20R");
		});

		test("DSR 6 returns (1,1) for cursor at origin", () => {
			const state = new TerminalState(80, 24);

			state.processAction({
				type: "Csi",
				value: { action: "DeviceStatusReport", data: 6 },
			});

			const response = state.takePendingResponse();
			const responseStr = new TextDecoder().decode(response!);
			expect(responseStr).toBe("\x1b[1;1R");
		});

		test("takePendingResponse clears the response", () => {
			const state = new TerminalState(80, 24);
			state.processAction({
				type: "Csi",
				value: { action: "DeviceStatusReport", data: 5 },
			});

			const first = state.takePendingResponse();
			expect(first).not.toBeNull();

			const second = state.takePendingResponse();
			expect(second).toBeNull();
		});
	});

	describe("Device Attributes", () => {
		test("Primary DA returns VT500 response", () => {
			const state = new TerminalState(80, 24);
			state.processAction({
				type: "Csi",
				value: { action: "PrimaryDeviceAttributes" },
			});

			const response = state.takePendingResponse();
			expect(response).not.toBeNull();

			const responseStr = new TextDecoder().decode(response!);
			// Should start with CSI ? and end with c
			expect(responseStr).toMatch(/^\x1b\[\?.*c$/);
			// Should contain 65 (VT500)
			expect(responseStr).toContain("65");
		});

		test("Secondary DA returns terminal info", () => {
			const state = new TerminalState(80, 24);
			state.processAction({
				type: "Csi",
				value: { action: "SecondaryDeviceAttributes" },
			});

			const response = state.takePendingResponse();
			expect(response).not.toBeNull();

			const responseStr = new TextDecoder().decode(response!);
			// Should start with CSI > and end with c
			expect(responseStr).toMatch(/^\x1b\[>.*c$/);
			// Should contain 65 (VT500 series)
			expect(responseStr).toContain("65");
		});
	});

	describe("Reset", () => {
		test("reset clears OSC state", () => {
			const state = new TerminalState(80, 24);

			state.processAction({
				type: "Osc",
				value: { action: "SetTitle", data: "Test Title" },
			});
			state.processAction({
				type: "Osc",
				value: { action: "SetWorkingDirectory", data: "file:///test" },
			});
			state.processAction({
				type: "Osc",
				value: { action: "Hyperlink", params: "id=1", uri: "https://test.com" },
			});

			// Reset via ESC c
			state.processAction({
				type: "Esc",
				value: { action: "ResetToInitialState" },
			});

			expect(state.title).toBe("");
			expect(state.iconName).toBe("");
			expect(state.workingDirectory).toBe("");
			expect(state.activeHyperlink).toBeNull();
		});
	});

	describe("OSC Unknown", () => {
		test("Unknown OSC is ignored without error", () => {
			const state = new TerminalState(80, 24);

			// This should not throw
			state.processAction({
				type: "Osc",
				value: { action: "Unknown", ps: 999, data: "some data" },
			});

			// State should be unaffected
			expect(state.title).toBe("");
		});
	});

	describe("OSC - Color Palette", () => {
		test("SetColorPalette is handled without error", () => {
			const state = new TerminalState(80, 24);

			// This should not throw
			state.processAction({
				type: "Osc",
				value: { action: "SetColorPalette", index: 0, color: "rgb:00/00/00" },
			});

			// State should be unaffected (placeholder implementation)
			expect(state.title).toBe("");
		});
	});

	describe("OSC - Foreground/Background Colors", () => {
		test("SetForegroundColor is handled without error", () => {
			const state = new TerminalState(80, 24);

			state.processAction({
				type: "Osc",
				value: { action: "SetForegroundColor", data: "#ffffff" },
			});

			// Should not throw
			expect(state.title).toBe("");
		});

		test("SetBackgroundColor is handled without error", () => {
			const state = new TerminalState(80, 24);

			state.processAction({
				type: "Osc",
				value: { action: "SetBackgroundColor", data: "#000000" },
			});

			// Should not throw
			expect(state.title).toBe("");
		});
	});

	describe("OSC - eMterm Extension", () => {
		test("EmtermExtension is handled without error", () => {
			const state = new TerminalState(80, 24);

			state.processAction({
				type: "Osc",
				value: {
					action: "EmtermExtension",
					data: { verb: "markdown", params: ["title", "body"] },
				},
			});

			// Should not throw (placeholder implementation)
			expect(state.title).toBe("");
		});
	});
});
