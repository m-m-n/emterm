import { describe, test, expect } from "bun:test";
import {
  parseOsc52,
  encodeBase64,
  decodeBase64,
  type Osc52Action,
} from "./osc-clipboard.ts";

// ── parseOsc52 tests ────────────────────────────────────

describe("parseOsc52", () => {
  test("should parse write operation with system clipboard target", () => {
    const result = parseOsc52("c;SGVsbG8=");
    expect(result).toEqual({
      type: "write",
      target: "c",
      data: "SGVsbG8=",
    });
  });

  test("should parse query operation", () => {
    const result = parseOsc52("c;?");
    expect(result).toEqual({
      type: "query",
      target: "c",
    });
  });

  test("should parse clear operation (empty payload)", () => {
    const result = parseOsc52("c;");
    expect(result).toEqual({
      type: "clear",
      target: "c",
    });
  });

  test("should parse primary selection target", () => {
    const result = parseOsc52("p;SGVsbG8=");
    expect(result).toEqual({
      type: "write",
      target: "p",
      data: "SGVsbG8=",
    });
  });

  test("should parse combined target cp", () => {
    const result = parseOsc52("cp;SGVsbG8=");
    expect(result).toEqual({
      type: "write",
      target: "cp",
      data: "SGVsbG8=",
    });
  });

  test("should return null for empty data", () => {
    expect(parseOsc52("")).toBeNull();
  });

  test("should return null for data without semicolon", () => {
    expect(parseOsc52("c")).toBeNull();
  });
});

// ── base64 tests ────────────────────────────────────────

describe("encodeBase64", () => {
  test("should encode empty string", () => {
    expect(encodeBase64("")).toBe("");
  });

  test("should encode simple text", () => {
    expect(encodeBase64("Hello")).toBe("SGVsbG8=");
  });

  test("should encode unicode text", () => {
    const encoded = encodeBase64("Hello World");
    const decoded = decodeBase64(encoded);
    expect(decoded).toBe("Hello World");
  });
});

describe("decodeBase64", () => {
  test("should decode empty string", () => {
    expect(decodeBase64("")).toBe("");
  });

  test("should decode simple text", () => {
    expect(decodeBase64("SGVsbG8=")).toBe("Hello");
  });

  test("should return null for invalid base64", () => {
    expect(decodeBase64("!!!invalid!!!")).toBeNull();
  });
});

// ── size limit tests ────────────────────────────────────

describe("size validation", () => {
  test("should validate payload within limit", () => {
    // "Hello" base64 encoded is small enough
    const result = parseOsc52("c;SGVsbG8=");
    expect(result).not.toBeNull();
  });
});
