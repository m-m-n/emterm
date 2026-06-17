/**
 * Listener for backend-originated `osc_notification` events.
 *
 * The backend reader thread emits an `osc_notification` event when an
 * `OSC 9 ; <message>` desktop-notification sequence is recognized on the
 * hidden (background) PTY processing path — i.e. while the window is
 * minimized/hidden and live bytes are not streaming to the WASM parser.
 *
 * This module subscribes to that event and fires the OS desktop notification
 * through the same permission-gated sink the foreground path uses
 * (`sendNotification`). It is a fire-and-forget side effect: the backend keeps
 * these out of the resume/replay stream, so window restore never re-fires.
 */

import { sendNotification } from "./osc-notification";

/** Payload shape for the `osc_notification` event (mirrors the Rust struct). */
export interface OscNotificationEvent {
	session_id: string;
	message: string;
}

/** Minimal `listen` signature so this module is unit-testable without Tauri. */
export type ListenFn = <T>(
	event: string,
	handler: (event: { payload: T }) => void,
) => Promise<() => void>;

/** Notification sink signature (matches `sendNotification`). */
export type NotificationSink = (title: string, message: string) => Promise<void>;

/**
 * Predicate deciding whether an event belongs to this listener's owner.
 *
 * The `osc_notification` event is emitted app-globally (`app.emit`), so every
 * `TerminalApp` instance's listener receives every event. Without scoping, a
 * single backend OSC 9 would fire one OS notification per open tab. The owner
 * passes a predicate that matches the backend PTY `session_id` it owns, so each
 * event fires exactly once (on the owning tab's listener).
 */
export type OwnsSession = (sessionId: string) => boolean;

/** Event name emitted by the backend reader thread. */
export const OSC_NOTIFICATION_EVENT = "osc_notification";

/** Notification title used for all background OSC 9 notifications. */
const NOTIFICATION_TITLE = "eMterm";

/**
 * Subscribe to the backend `osc_notification` event and fire the OS desktop
 * notification for each message via the provided sink.
 *
 * @param listen - Tauri `listen` function (injected for testability).
 * @param sink - Notification sink; defaults to the shared `sendNotification`.
 * @param ownsSession - Optional predicate; when supplied, the event fires only
 *   if `ownsSession(event.payload.session_id)` is true. When omitted, every
 *   event fires (legacy / unscoped behavior). Owners pass a predicate matching
 *   their PTY session so a global `app.emit` fires exactly once per OSC 9
 *   instead of once per open tab.
 * @returns The unlisten function returned by `listen`.
 */
export async function registerBackgroundNotificationListener(
	listen: ListenFn,
	sink: NotificationSink = sendNotification,
	ownsSession: OwnsSession | null = null,
): Promise<() => void> {
	return listen<OscNotificationEvent>(
		OSC_NOTIFICATION_EVENT,
		(event: { payload: OscNotificationEvent }) => {
			const { session_id, message } = event.payload;
			// Only fire for events belonging to this listener's owner. A global
			// app.emit reaches every tab's listener; scoping by session_id keeps
			// a single OSC 9 from firing one notification per open tab.
			if (ownsSession && !ownsSession(session_id)) return;
			// Fire the OS notification (permission gating happens inside the sink).
			void sink(NOTIFICATION_TITLE, message);
		},
	);
}
