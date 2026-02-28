/**
 * Profile-related frontend types and helpers.
 *
 * Provides env var parsing, default flag management,
 * and profile duplication utilities.
 */

import type { Profile } from "../settings/types";

/**
 * Parse a multi-line KEY=VALUE string into a key-value map.
 *
 * Rules:
 * - Each line is treated as one entry
 * - Lines without '=' are skipped
 * - Empty lines and whitespace-only lines are skipped
 * - The key is everything before the first '='
 * - The value is everything after the first '=' (may contain '=')
 * - Keys are trimmed of leading/trailing whitespace
 * - Values are preserved as-is (after the first '=')
 * - Empty keys (after trim) are skipped
 */
export function parseEnvVars(text: string): Record<string, string> {
	const result: Record<string, string> = {};
	for (const line of text.split("\n")) {
		const trimmed = line.trim();
		if (trimmed === "" || !trimmed.includes("=")) continue;
		const eqIndex = trimmed.indexOf("=");
		const key = trimmed.substring(0, eqIndex).trim();
		const value = trimmed.substring(eqIndex + 1);
		if (key === "") continue;
		result[key] = value;
	}
	return result;
}

/**
 * Ensure at most one profile has is_default=true.
 * Sets the profile at `defaultIndex` as default and clears all others.
 * If `defaultIndex` is -1, clears all defaults.
 */
export function ensureSingleDefault(
	profiles: Profile[],
	defaultIndex: number,
): void {
	for (let i = 0; i < profiles.length; i++) {
		const p = profiles[i];
		if (p) p.is_default = i === defaultIndex;
	}
}

/**
 * Create a duplicate of a profile with "(Copy)" appended to the name.
 * If the name already ends with "(Copy)", appends " (Copy)" again.
 * The duplicate is always non-default.
 */
export function duplicateProfile(profile: Profile): Profile {
	return {
		name: `${profile.name} (Copy)`,
		shell_path: profile.shell_path,
		shell_args: [...profile.shell_args],
		env_vars: profile.env_vars,
		working_directory: profile.working_directory,
		is_default: false,
	};
}

/**
 * Create a new empty profile with default values.
 */
export function createEmptyProfile(): Profile {
	return {
		name: "",
		shell_path: "",
		shell_args: [],
		env_vars: "",
		working_directory: "",
		is_default: false,
	};
}
