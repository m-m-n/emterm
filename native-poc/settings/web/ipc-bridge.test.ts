/**
 * Tests for the Tauri-invoke → wry IPC bridge (settings child window).
 *
 * The bridge is exercised against happy-dom's `window` with a stubbed
 * `window.ipc` channel standing in for wry.
 */

import { afterEach, describe, expect, test } from "bun:test";

import { installTauriInvokeBridge, pendingCallCount } from "./ipc-bridge.ts";

function installStubChannel(): string[] {
	const sent: string[] = [];
	window.ipc = {
		postMessage(message: string) {
			sent.push(message);
		},
	};
	return sent;
}

afterEach(() => {
	// The bridge install is idempotent via __EMTERM_SETTINGS_IPC__; reset
	// the globals so each test sees a fresh window (bun test isolation).
	delete window.__EMTERM_SETTINGS_IPC__;
	delete window.__TAURI_INTERNALS__;
	delete window.ipc;
});

describe("settings ipc bridge", () => {
	test("invoke posts an {id, cmd, args} JSON message", async () => {
		const sent = installStubChannel();
		installTauriInvokeBridge();

		const promise = window.__TAURI_INTERNALS__?.invoke("load_settings", {
			a: 1,
		});
		expect(sent.length).toBe(1);
		const msg = JSON.parse(sent[0] ?? "");
		expect(typeof msg.id).toBe("number");
		expect(msg.cmd).toBe("load_settings");
		expect(msg.args).toEqual({ a: 1 });

		// Resolve so the in-flight promise does not leak across tests.
		window.__EMTERM_SETTINGS_IPC__?.resolve(msg.id, true, null);
		await promise;
	});

	test("missing args defaults to an empty object", async () => {
		const sent = installStubChannel();
		installTauriInvokeBridge();

		const promise = window.__TAURI_INTERNALS__?.invoke("get_platform");
		const msg = JSON.parse(sent[0] ?? "");
		expect(msg.args).toEqual({});
		window.__EMTERM_SETTINGS_IPC__?.resolve(msg.id, true, "linux");
		await expect(promise).resolves.toBe("linux");
	});

	test("ok reply resolves and err reply rejects, clearing the pending slot", async () => {
		const sent = installStubChannel();
		installTauriInvokeBridge();

		const okCall = window.__TAURI_INTERNALS__?.invoke("load_settings");
		const errCall = window.__TAURI_INTERNALS__?.invoke("save_settings");
		expect(pendingCallCount()).toBe(2);

		const okId = JSON.parse(sent[0] ?? "").id as number;
		const errId = JSON.parse(sent[1] ?? "").id as number;
		window.__EMTERM_SETTINGS_IPC__?.resolve(okId, true, { font_size: 13 });
		window.__EMTERM_SETTINGS_IPC__?.resolve(errId, false, "boom");

		await expect(okCall).resolves.toEqual({ font_size: 13 });
		await expect(errCall).rejects.toBe("boom");
		expect(pendingCallCount()).toBe(0);
	});

	test("a reply for an unknown id is ignored", () => {
		installStubChannel();
		installTauriInvokeBridge();
		// Must not throw.
		window.__EMTERM_SETTINGS_IPC__?.resolve(99_999, true, null);
		expect(pendingCallCount()).toBe(0);
	});

	test("invoke rejects when the wry channel is absent", async () => {
		installStubChannel();
		installTauriInvokeBridge();
		delete window.ipc;
		await expect(
			window.__TAURI_INTERNALS__?.invoke("load_settings"),
		).rejects.toBeInstanceOf(Error);
	});
});
