import { describe, test, expect } from "bun:test";
import { validateSocketPath, parseMuxOsc } from "./mux-client";

describe("validateSocketPath", () => {
  test("rejects path traversal", () => {
    expect(validateSocketPath("/tmp/emterm/../etc/passwd")).toBe(false);
    expect(validateSocketPath("../../../etc/passwd")).toBe(false);
    expect(validateSocketPath("/tmp/emterm/..\\windows\\system32")).toBe(false);
  });

  test("accepts valid emterm socket paths", () => {
    expect(validateSocketPath("/run/user/1000/emterm/mux-default.sock")).toBe(true);
    expect(validateSocketPath("/tmp/emterm/mux-default.sock")).toBe(true);
  });

  test("rejects paths without emterm", () => {
    expect(validateSocketPath("/tmp/other/mux.sock")).toBe(false);
  });

  test("rejects paths without .sock extension", () => {
    expect(validateSocketPath("/tmp/emterm/something")).toBe(false);
  });
});

describe("parseMuxOsc", () => {
  test("parses attach command", () => {
    const result = parseMuxOsc("emterm", ["mux", "attach", "/tmp/emterm/mux-default.sock", "1"]);
    expect(result).toEqual({
      action: "attach",
      socketPath: "/tmp/emterm/mux-default.sock",
      sessionId: 1,
    });
  });

  test("parses detach command", () => {
    const result = parseMuxOsc("emterm", ["mux", "detach"]);
    expect(result).toEqual({ action: "detach" });
  });

  test("returns null for non-emterm verb", () => {
    expect(parseMuxOsc("other", ["mux", "attach", "/tmp/emterm/s.sock", "0"])).toBeNull();
  });

  test("returns null for non-mux params", () => {
    expect(parseMuxOsc("emterm", ["fold", "start"])).toBeNull();
  });

  test("returns null for invalid attach (missing params)", () => {
    expect(parseMuxOsc("emterm", ["mux", "attach"])).toBeNull();
  });

  test("rejects path traversal in attach", () => {
    const result = parseMuxOsc("emterm", ["mux", "attach", "/tmp/emterm/../etc.sock", "0"]);
    expect(result).toBeNull();
  });

  test("handles non-numeric session ID", () => {
    const result = parseMuxOsc("emterm", ["mux", "attach", "/tmp/emterm/mux.sock", "abc"]);
    expect(result).not.toBeNull();
    expect(result!.sessionId).toBe(0); // defaults to 0
  });

  test("returns null for unknown action", () => {
    expect(parseMuxOsc("emterm", ["mux", "unknown"])).toBeNull();
  });
});
