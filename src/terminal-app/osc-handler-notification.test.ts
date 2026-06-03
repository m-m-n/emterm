/**
 * FR3 regression: a non-active regular tab (mux off, window visible) fires an
 * OSC 9 desktop notification through the existing foreground handler.
 *
 * In the multi-tab model the per-window VisibilityController only marks the
 * backend hidden when the whole window is hidden/minimized — it is NOT driven
 * by tab switching. So a non-active tab in a visible window keeps streaming
 * its PTY bytes through the normal reader -> WASM -> `handleOscCallback` path.
 * `handleOscCallback` has no active-tab parameter and no active-tab gate: it
 * fires `sendNotification` for OSC 9 regardless of which tab is displayed.
 *
 * These tests lock that guarantee: invoking `handleOscCallback(ctx, 9, ...)`
 * (the exact call a non-active tab makes) fires the notification, and a
 * progress sequence does not. The OS-notification permission gate (FR6) is
 * exercised by mocking the Tauri notification plugin.
 */
import { afterEach, describe, expect, mock, test } from "bun:test";
import { handleOscCallback, type OscHandlerContext } from "./osc-handler";

// Captured calls into the mocked Tauri notification plugin.
let sentNotifications: Array<{ title?: string; body?: string }> = [];
let permissionGranted = true;

mock.module("@tauri-apps/plugin-notification", () => ({
	isPermissionGranted: async () => permissionGranted,
	sendNotification: (opts: { title?: string; body?: string }) => {
		sentNotifications.push(opts);
	},
}));

/**
 * Build a minimal OscHandlerContext sufficient for the OSC 9 branch. The OSC 9
 * notification path only reads `ctx.state` truthiness and (for progress) a few
 * state fields; the notification branch needs nothing else.
 */
function makeContext(): OscHandlerContext {
	const state = {
		title: "Terminal",
		_progressState: 0,
		_progressPercentage: -1,
	};
	return {
		state,
		renderer: null,
		ptyClient: null,
		oscColorHandler: {} as never,
		cursorShapeStack: {} as never,
		imageHandler: null,
		downloadManager: null,
		terminalRoot: null,
		titleChangeCallback: null,
		lastWindowTitle: "",
		setLastWindowTitle: () => {},
		muxAttachCallback: null,
		muxDetachCallback: null,
		statusBarOscCallback: null,
	} as unknown as OscHandlerContext;
}

/** Allow the dynamic import + async permission check inside the sink to settle. */
async function flushAsync(): Promise<void> {
	await new Promise((resolve) => setTimeout(resolve, 0));
}

describe("OSC 9 foreground handler (FR3 non-active tab)", () => {
	afterEach(() => {
		sentNotifications = [];
		permissionGranted = true;
	});

	test("TS-13: OSC 9 notification fires for any tab (no active-tab gate)", async () => {
		const ctx = makeContext();

		// This is exactly the call a non-active tab's WASM parser makes.
		handleOscCallback(ctx, 9, "build done");
		await flushAsync();

		expect(sentNotifications).toEqual([{ title: "eMterm", body: "build done" }]);
	});

	test("SC-4: OSC 9;4 progress does NOT fire a notification", async () => {
		const ctx = makeContext();

		handleOscCallback(ctx, 9, "4;1;50");
		await flushAsync();

		expect(sentNotifications).toEqual([]);
	});

	test("FR6: notification suppressed when permission is not granted", async () => {
		permissionGranted = false;
		const ctx = makeContext();

		handleOscCallback(ctx, 9, "should not appear");
		await flushAsync();

		expect(sentNotifications).toEqual([]);
	});
});
