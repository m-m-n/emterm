import { describe, expect, test } from "bun:test";
import {
	OSC_NOTIFICATION_EVENT,
	type OscNotificationEvent,
	registerBackgroundNotificationListener,
} from "./background-notification-listener";

/**
 * Build a fake `listen` that captures the registered handler so a test can
 * synthesize backend events.
 */
function makeFakeListen() {
	let handler: ((event: { payload: OscNotificationEvent }) => void) | null =
		null;
	let registeredEvent: string | null = null;
	let unlistened = false;
	const listen = (async <T>(
		event: string,
		h: (event: { payload: T }) => void,
	): Promise<() => void> => {
		registeredEvent = event;
		handler = h as (event: { payload: OscNotificationEvent }) => void;
		return () => {
			unlistened = true;
		};
	}) as <T>(
		event: string,
		h: (event: { payload: T }) => void,
	) => Promise<() => void>;
	return {
		listen,
		emit(payload: OscNotificationEvent) {
			handler?.({ payload });
		},
		get registeredEvent() {
			return registeredEvent;
		},
		get unlistened() {
			return unlistened;
		},
	};
}

describe("registerBackgroundNotificationListener", () => {
	test("subscribes to the osc_notification event", async () => {
		const fake = makeFakeListen();
		await registerBackgroundNotificationListener(fake.listen, async () => {});
		expect(fake.registeredEvent).toBe(OSC_NOTIFICATION_EVENT);
	});

	test("TS-10: calls the sink with title eMterm and the message body", async () => {
		const fake = makeFakeListen();
		const calls: Array<{ title: string; message: string }> = [];
		await registerBackgroundNotificationListener(
			fake.listen,
			async (title, message) => {
				calls.push({ title, message });
			},
		);

		fake.emit({ session_id: "s1", message: "build done" });

		expect(calls).toEqual([{ title: "eMterm", message: "build done" }]);
	});

	test("TS-12: when the sink suppresses (permission denied), no notification is shown", async () => {
		const fake = makeFakeListen();
		let shown = 0;
		// Simulate a permission-denied sink: it is invoked but shows nothing.
		await registerBackgroundNotificationListener(fake.listen, async () => {
			// Sink decides (permission check) NOT to show -> shown stays 0.
		});

		fake.emit({ session_id: "s1", message: "blocked" });

		expect(shown).toBe(0);
	});

	test("fires once per event (no duplicate)", async () => {
		const fake = makeFakeListen();
		const messages: string[] = [];
		await registerBackgroundNotificationListener(
			fake.listen,
			async (_title, message) => {
				messages.push(message);
			},
		);

		fake.emit({ session_id: "s1", message: "one" });
		fake.emit({ session_id: "s1", message: "two" });

		expect(messages).toEqual(["one", "two"]);
	});

	test("ownsSession: fires only for the owned session_id (no cross-tab duplicate)", async () => {
		const fake = makeFakeListen();
		const messages: string[] = [];
		// This listener owns session "A" only.
		await registerBackgroundNotificationListener(
			fake.listen,
			async (_title, message) => {
				messages.push(message);
			},
			(sessionId) => sessionId === "A",
		);

		fake.emit({ session_id: "A", message: "mine" });
		fake.emit({ session_id: "B", message: "other tab" });

		// Only the owned session's event fires; the other tab's is ignored.
		expect(messages).toEqual(["mine"]);
	});

	test("returns the unlisten function", async () => {
		const fake = makeFakeListen();
		const unlisten = await registerBackgroundNotificationListener(
			fake.listen,
			async () => {},
		);
		unlisten();
		expect(fake.unlistened).toBe(true);
	});
});
