/**
 * OSC 9 notification and progress bar handler.
 *
 * Supports:
 * - `OSC 9 ; message ST` - Desktop notification
 * - `OSC 9 ; 4 ; state ; percentage ST` - Progress bar indicator
 *
 * Progress states:
 *   0 = remove, 1 = normal, 2 = paused, 3 = indeterminate, 4 = error
 */

/**
 * Progress state values.
 * 0=remove, 1=normal, 2=error, 3=indeterminate, 4=warning
 */
export type ProgressState = 0 | 1 | 2 | 3 | 4;

/** Notification action from OSC 9. */
export interface Osc9Notification {
	type: "notification";
	message: string;
}

/** Progress bar action from OSC 9;4. */
export interface Osc9Progress {
	type: "progress";
	state: ProgressState;
	/** Percentage 0-100, or -1 for indeterminate. */
	percentage: number;
}

export type Osc9Action = Osc9Notification | Osc9Progress;

/**
 * Parse OSC 9 data string into an action.
 *
 * @param data - The OSC data after "9;"
 * @returns Parsed action, or null if invalid progress format
 */
export function parseOsc9(data: string): Osc9Action | null {
	// Check for progress format: "4;state[;percentage]"
	if (data.startsWith("4;")) {
		const parts = data.split(";");
		const stateNum = parseInt(parts[1]!, 10);
		if (isNaN(stateNum) || stateNum < 0 || stateNum > 4) {
			return null;
		}
		const state = stateNum as ProgressState;

		let percentage = -1;
		if (parts.length >= 3 && parts[2] !== undefined && parts[2] !== "") {
			const pct = parseInt(parts[2], 10);
			if (!isNaN(pct)) {
				percentage = Math.max(0, Math.min(100, pct));
			}
		}

		return { type: "progress", state, percentage };
	}

	// Plain notification message
	return { type: "notification", message: data };
}

/**
 * Send an OS-native desktop notification via Tauri plugin.
 *
 * @param title - Notification title
 * @param message - Notification body text
 */
export async function sendNotification(
	title: string,
	message: string,
): Promise<void> {
	try {
		const { sendNotification, isPermissionGranted } = await import(
			"@tauri-apps/plugin-notification"
		);
		const permitted = await isPermissionGranted();
		if (permitted) {
			sendNotification({ title, body: message });
		}
	} catch (error) {
		console.error("Failed to send notification:", error);
	}
}
