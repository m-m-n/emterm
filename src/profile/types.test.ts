import { describe, test, expect } from "bun:test";
import {
	parseEnvVars,
	ensureSingleDefault,
	duplicateProfile,
	createEmptyProfile,
} from "./types";
import type { Profile } from "../settings/types";

describe("parseEnvVars", () => {
	test("should parse valid KEY=VALUE pairs", () => {
		const result = parseEnvVars("FOO=bar\nBAZ=qux");
		expect(result).toEqual({ FOO: "bar", BAZ: "qux" });
	});

	test("should handle values containing '='", () => {
		const result = parseEnvVars("PATH=/usr/bin:/bin\nFOO=a=b=c");
		expect(result).toEqual({ PATH: "/usr/bin:/bin", FOO: "a=b=c" });
	});

	test("should skip empty lines", () => {
		const result = parseEnvVars("FOO=bar\n\nBAZ=qux\n");
		expect(result).toEqual({ FOO: "bar", BAZ: "qux" });
	});

	test("should skip lines without '='", () => {
		const result = parseEnvVars("FOO=bar\nINVALID\nBAZ=qux");
		expect(result).toEqual({ FOO: "bar", BAZ: "qux" });
	});

	test("should skip whitespace-only lines", () => {
		const result = parseEnvVars("FOO=bar\n   \nBAZ=qux");
		expect(result).toEqual({ FOO: "bar", BAZ: "qux" });
	});

	test("should trim keys but preserve value after '='", () => {
		const result = parseEnvVars("  FOO  =  bar  ");
		// line.trim() removes line-level whitespace, then value is kept as-is after '='
		expect(result).toEqual({ FOO: "  bar" });
	});

	test("should preserve leading whitespace in values", () => {
		const result = parseEnvVars("TOKEN= abc");
		expect(result).toEqual({ TOKEN: " abc" });
	});

	test("should skip empty keys", () => {
		const result = parseEnvVars("=value\nFOO=bar");
		expect(result).toEqual({ FOO: "bar" });
	});

	test("should return empty object for empty string", () => {
		expect(parseEnvVars("")).toEqual({});
	});

	test("should handle value with empty string", () => {
		const result = parseEnvVars("FOO=");
		expect(result).toEqual({ FOO: "" });
	});
});

describe("ensureSingleDefault", () => {
	test("should set specified index as default and clear others", () => {
		const profiles: Profile[] = [
			{ name: "A", shell_path: "", shell_args: [], env_vars: "", working_directory: "", is_default: true },
			{ name: "B", shell_path: "", shell_args: [], env_vars: "", working_directory: "", is_default: false },
			{ name: "C", shell_path: "", shell_args: [], env_vars: "", working_directory: "", is_default: false },
		];
		ensureSingleDefault(profiles, 1);
		expect(profiles[0].is_default).toBe(false);
		expect(profiles[1].is_default).toBe(true);
		expect(profiles[2].is_default).toBe(false);
	});

	test("should clear all defaults when index is -1", () => {
		const profiles: Profile[] = [
			{ name: "A", shell_path: "", shell_args: [], env_vars: "", working_directory: "", is_default: true },
			{ name: "B", shell_path: "", shell_args: [], env_vars: "", working_directory: "", is_default: true },
		];
		ensureSingleDefault(profiles, -1);
		expect(profiles[0].is_default).toBe(false);
		expect(profiles[1].is_default).toBe(false);
	});

	test("should work with empty array", () => {
		const profiles: Profile[] = [];
		ensureSingleDefault(profiles, -1);
		expect(profiles.length).toBe(0);
	});
});

describe("duplicateProfile", () => {
	test("should create copy with '(Copy)' suffix", () => {
		const original: Profile = {
			name: "My Shell",
			shell_path: "/bin/zsh",
			shell_args: ["--login"],
			env_vars: "FOO=bar",
			working_directory: "/tmp",
			is_default: true,
		};
		const copy = duplicateProfile(original);
		expect(copy.name).toBe("My Shell (Copy)");
		expect(copy.shell_path).toBe("/bin/zsh");
		expect(copy.shell_args).toEqual(["--login"]);
		expect(copy.env_vars).toBe("FOO=bar");
		expect(copy.working_directory).toBe("/tmp");
		expect(copy.is_default).toBe(false);
	});

	test("should not share shell_args reference", () => {
		const original: Profile = {
			name: "A",
			shell_path: "",
			shell_args: ["--login"],
			env_vars: "",
			working_directory: "",
			is_default: false,
		};
		const copy = duplicateProfile(original);
		copy.shell_args.push("--extra");
		expect(original.shell_args).toEqual(["--login"]);
	});
});

describe("createEmptyProfile", () => {
	test("should create profile with empty fields", () => {
		const p = createEmptyProfile();
		expect(p.name).toBe("");
		expect(p.shell_path).toBe("");
		expect(p.shell_args).toEqual([]);
		expect(p.env_vars).toBe("");
		expect(p.working_directory).toBe("");
		expect(p.is_default).toBe(false);
	});
});
