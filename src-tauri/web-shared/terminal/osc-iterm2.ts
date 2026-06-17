/**
 * OSC 1337 iTerm2 protocol handlers.
 *
 * Supports:
 * - `OSC 1337;File=[args]:base64data ST` - Inline image display or file download
 * - `OSC 1337;SetUserVar=key=base64value ST` - Set user variable per session
 */

/**
 * Parsed File command from OSC 1337;File.
 */
export interface Iterm2FileCommand {
	type: "file";
	/** Filename (decoded from base64 name= arg). */
	name: string;
	/** File size in bytes (0 if not specified). */
	size: number;
	/** Display width (e.g., "80", "auto", "50%", "100px"). */
	width: string;
	/** Display height (e.g., "24", "auto", "50%", "100px"). */
	height: string;
	/** Whether to display inline (true) or download (false). */
	inline: boolean;
	/** Whether to preserve aspect ratio. */
	preserveAspectRatio: boolean;
	/** Base64-encoded file data. */
	base64Data: string;
}

/**
 * Parsed SetUserVar command from OSC 1337;SetUserVar.
 */
export interface Iterm2SetUserVarCommand {
	type: "set_user_var";
	/** Variable key. */
	key: string;
	/** Decoded variable value. */
	value: string;
}

export type Iterm2Command = Iterm2FileCommand | Iterm2SetUserVarCommand;

/**
 * Parse an OSC 1337 data string into a command.
 *
 * @param data - Full data after "1337;" (e.g., "File=inline=1:AAAA" or "SetUserVar=key=val")
 * @returns Parsed command, or null if unknown/invalid
 */
export function parseIterm2Command(data: string): Iterm2Command | null {
	if (!data) return null;

	if (data.startsWith("File=")) {
		return parseFileArgs(data.slice(5));
	}

	if (data.startsWith("SetUserVar=")) {
		return parseSetUserVar(data.slice(11));
	}

	return null;
}

/**
 * Parse File command arguments and base64 data.
 *
 * Format: `key1=val1;key2=val2:base64data`
 *
 * Supported keys:
 * - name: base64-encoded filename
 * - size: file size in bytes
 * - width: display width
 * - height: display height
 * - inline: 0 or 1
 * - preserveAspectRatio: 0 or 1
 *
 * @param argsAndData - Everything after "File="
 * @returns Parsed file command, or null if invalid
 */
export function parseFileArgs(argsAndData: string): Iterm2FileCommand | null {
	const colonIdx = argsAndData.indexOf(":");
	if (colonIdx === -1) return null;

	const argsPart = argsAndData.slice(0, colonIdx);
	const base64Data = argsAndData.slice(colonIdx + 1);

	// Parse key=value pairs separated by semicolons
	const args = new Map<string, string>();
	if (argsPart) {
		for (const pair of argsPart.split(";")) {
			const eqIdx = pair.indexOf("=");
			if (eqIdx > 0) {
				args.set(pair.slice(0, eqIdx), pair.slice(eqIdx + 1));
			}
		}
	}

	// Decode name from base64
	let name = "";
	const nameB64 = args.get("name");
	if (nameB64) {
		try {
			name = atob(nameB64);
		} catch {
			name = nameB64; // Use raw value if not valid base64
		}
	}

	const size = parseInt(args.get("size") ?? "0", 10) || 0;
	const width = args.get("width") ?? "";
	const height = args.get("height") ?? "";
	const inline = args.get("inline") === "1";
	const preserveAspectRatio = args.get("preserveAspectRatio") !== "0";

	return {
		type: "file",
		name,
		size,
		width,
		height,
		inline,
		preserveAspectRatio,
		base64Data,
	};
}

/**
 * Parse SetUserVar command.
 *
 * Format: `key=base64value`
 *
 * @param data - Everything after "SetUserVar="
 * @returns Parsed command, or null if invalid
 */
export function parseSetUserVar(
	data: string,
): Iterm2SetUserVarCommand | null {
	const eqIdx = data.indexOf("=");
	if (eqIdx === -1) return null;

	const key = data.slice(0, eqIdx);
	const base64Value = data.slice(eqIdx + 1);

	let value: string;
	try {
		value = base64Value ? atob(base64Value) : "";
	} catch {
		console.warn(
			`[WARN][FRONTEND] Invalid base64 in SetUserVar for key "${key}"`,
		);
		return null;
	}

	return { type: "set_user_var", key, value };
}
