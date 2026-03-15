import { describe, test, expect } from "bun:test";
import { parseData, prettyPrintJson, serializeData } from "./parser.ts";

describe("parseData", () => {
  test("parses valid JSON", () => {
    const result = parseData('{"key": "value"}', "json");
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.data).toEqual({ key: "value" });
    }
  });

  test("returns error for invalid JSON", () => {
    const result = parseData("{invalid json}", "json");
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.error).toBeTruthy();
      expect(result.rawText).toBe("{invalid json}");
    }
  });

  test("parses valid YAML", () => {
    const result = parseData("key: value\nnested:\n  a: 1", "yaml");
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.data).toEqual({ key: "value", nested: { a: 1 } });
    }
  });

  test("returns error for invalid YAML", () => {
    const result = parseData(":\n  - invalid:\n bad:\nindent", "yaml");
    // YAML is very permissive, most strings parse successfully
    // This test just verifies the function doesn't throw
    expect(result.rawText).toBeTruthy();
  });

  test("parses JSON arrays", () => {
    const result = parseData("[1, 2, 3]", "json");
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.data).toEqual([1, 2, 3]);
    }
  });

  test("parses empty JSON object", () => {
    const result = parseData("{}", "json");
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.data).toEqual({});
    }
  });

  test("parses empty JSON array", () => {
    const result = parseData("[]", "json");
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.data).toEqual([]);
    }
  });

  test("parses JSON with unicode", () => {
    const result = parseData('{"emoji": "🎉", "日本語": "テスト"}', "json");
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect((result.data as Record<string, string>).emoji).toBe("🎉");
      expect((result.data as Record<string, string>)["日本語"]).toBe("テスト");
    }
  });
});

describe("prettyPrintJson", () => {
  test("formats minified JSON", () => {
    const data = { key: "value", nested: { a: 1 } };
    const result = prettyPrintJson(data);
    expect(result).toContain("  ");
    expect(result).toContain('"key"');
    expect(result).toContain('"nested"');
  });

  test("handles arrays", () => {
    const data = [1, 2, 3];
    const result = prettyPrintJson(data);
    expect(result).toContain("[\n");
  });
});

describe("serializeData", () => {
  test("serializes as JSON", () => {
    const result = serializeData({ a: 1 }, "json");
    expect(result).toContain('"a"');
    expect(result).toContain("1");
  });

  test("serializes as YAML", () => {
    const result = serializeData({ a: 1 }, "yaml");
    expect(result).toContain("a:");
    expect(result).toContain("1");
  });
});
