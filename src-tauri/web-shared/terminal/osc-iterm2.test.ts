/**
 * Tests for OSC 1337 iTerm2 protocol handlers.
 */
import { describe, it, expect } from "bun:test";
import {
	parseIterm2Command,
	parseFileArgs,
	parseSetUserVar,
	type Iterm2FileCommand,
	type Iterm2SetUserVarCommand,
} from "./osc-iterm2";

describe("parseIterm2Command", () => {
	it("dispatches File command", () => {
		const result = parseIterm2Command("File=inline=1:AAAA");
		expect(result).not.toBeNull();
		expect(result!.type).toBe("file");
	});

	it("dispatches SetUserVar command", () => {
		const result = parseIterm2Command("SetUserVar=mykey=dGVzdA==");
		expect(result).not.toBeNull();
		expect(result!.type).toBe("set_user_var");
	});

	it("returns null for unknown subcommand", () => {
		expect(parseIterm2Command("UnknownCmd=foo")).toBeNull();
	});

	it("returns null for empty data", () => {
		expect(parseIterm2Command("")).toBeNull();
	});
});

describe("parseFileArgs", () => {
	it("parses inline=1 with base64 data", () => {
		const result = parseFileArgs("inline=1:AAAA");
		expect(result).not.toBeNull();
		expect(result!.inline).toBe(true);
		expect(result!.base64Data).toBe("AAAA");
	});

	it("parses inline=0 (download mode)", () => {
		const result = parseFileArgs("inline=0:AAAA");
		expect(result).not.toBeNull();
		expect(result!.inline).toBe(false);
	});

	it("defaults inline to false when omitted", () => {
		const result = parseFileArgs(":AAAA");
		expect(result).not.toBeNull();
		expect(result!.inline).toBe(false);
	});

	it("parses name argument", () => {
		const result = parseFileArgs("name=dGVzdC5wbmc=;inline=1:AAAA");
		expect(result).not.toBeNull();
		expect(result!.name).toBe("test.png");
	});

	it("parses size argument", () => {
		const result = parseFileArgs("size=1024;inline=1:AAAA");
		expect(result).not.toBeNull();
		expect(result!.size).toBe(1024);
	});

	it("parses width argument", () => {
		const result = parseFileArgs("width=80;inline=1:AAAA");
		expect(result).not.toBeNull();
		expect(result!.width).toBe("80");
	});

	it("parses height argument", () => {
		const result = parseFileArgs("height=24;inline=1:AAAA");
		expect(result).not.toBeNull();
		expect(result!.height).toBe("24");
	});

	it("parses preserveAspectRatio argument", () => {
		const result = parseFileArgs(
			"preserveAspectRatio=0;inline=1:AAAA",
		);
		expect(result).not.toBeNull();
		expect(result!.preserveAspectRatio).toBe(false);
	});

	it("preserveAspectRatio defaults to true", () => {
		const result = parseFileArgs("inline=1:AAAA");
		expect(result).not.toBeNull();
		expect(result!.preserveAspectRatio).toBe(true);
	});

	it("parses all args together", () => {
		const result = parseFileArgs(
			"name=aW1nLnBuZw==;size=2048;width=auto;height=50%;preserveAspectRatio=1;inline=1:AAAA",
		);
		expect(result).not.toBeNull();
		expect(result!.name).toBe("img.png");
		expect(result!.size).toBe(2048);
		expect(result!.width).toBe("auto");
		expect(result!.height).toBe("50%");
		expect(result!.preserveAspectRatio).toBe(true);
		expect(result!.inline).toBe(true);
		expect(result!.base64Data).toBe("AAAA");
	});

	it("returns null when no colon separator", () => {
		const result = parseFileArgs("inline=1");
		expect(result).toBeNull();
	});

	it("handles empty base64 data", () => {
		const result = parseFileArgs("inline=1:");
		expect(result).not.toBeNull();
		expect(result!.base64Data).toBe("");
	});
});

describe("parseSetUserVar", () => {
	it("parses key=base64value", () => {
		const result = parseSetUserVar("mykey=dGVzdA==");
		expect(result).not.toBeNull();
		expect(result!.key).toBe("mykey");
		expect(result!.value).toBe("test");
	});

	it("parses key with empty value", () => {
		const result = parseSetUserVar("emptykey=");
		expect(result).not.toBeNull();
		expect(result!.key).toBe("emptykey");
		expect(result!.value).toBe("");
	});

	it("returns null for missing equals sign", () => {
		expect(parseSetUserVar("noequals")).toBeNull();
	});

	it("handles invalid base64 gracefully", () => {
		// Invalid base64 should still attempt decode; atob may throw
		const result = parseSetUserVar("key=!!!invalid!!!");
		// Should return null on decode failure
		expect(result).toBeNull();
	});

	it("decodes unicode value", () => {
		// "hello" in base64 is "aGVsbG8="
		const result = parseSetUserVar("greeting=aGVsbG8=");
		expect(result).not.toBeNull();
		expect(result!.key).toBe("greeting");
		expect(result!.value).toBe("hello");
	});
});
