import { describe, test, expect } from "bun:test";
import { highlightJson, highlightYaml } from "./highlighter.ts";

describe("highlightJson", () => {
  test("highlights keys", () => {
    const result = highlightJson('{"key": "value"}');
    expect(result).toContain('class="dv-key"');
    expect(result).toContain('"key"');
  });

  test("highlights strings", () => {
    const result = highlightJson('{"k": "hello"}');
    expect(result).toContain('class="dv-string"');
  });

  test("highlights numbers", () => {
    const result = highlightJson('{"k": 42}');
    expect(result).toContain('class="dv-number"');
    expect(result).toContain("42");
  });

  test("highlights booleans", () => {
    const result = highlightJson('{"k": true}');
    expect(result).toContain('class="dv-boolean"');
    expect(result).toContain("true");
  });

  test("highlights null", () => {
    const result = highlightJson('{"k": null}');
    expect(result).toContain('class="dv-null"');
    expect(result).toContain("null");
  });

  test("highlights punctuation", () => {
    const result = highlightJson("{}");
    expect(result).toContain('class="dv-punct"');
  });

  test("sanitizes HTML in values", () => {
    const result = highlightJson('{"k": "<script>alert(1)</script>"}');
    expect(result).not.toContain("<script>");
    expect(result).toContain("&lt;script&gt;");
  });
});

describe("highlightYaml", () => {
  test("highlights keys", () => {
    const result = highlightYaml("key: value");
    expect(result).toContain('class="dv-key"');
    expect(result).toContain("key");
  });

  test("highlights string values", () => {
    const result = highlightYaml('key: "hello"');
    expect(result).toContain('class="dv-string"');
  });

  test("highlights numbers", () => {
    const result = highlightYaml("port: 8080");
    expect(result).toContain('class="dv-number"');
    expect(result).toContain("8080");
  });

  test("highlights booleans", () => {
    const result = highlightYaml("enabled: true");
    expect(result).toContain('class="dv-boolean"');
  });

  test("highlights null", () => {
    const result = highlightYaml("value: null");
    expect(result).toContain('class="dv-null"');
  });

  test("highlights comments", () => {
    const result = highlightYaml("# This is a comment");
    expect(result).toContain('class="dv-comment"');
  });

  test("highlights list items", () => {
    const result = highlightYaml("- item1");
    expect(result).toContain('class="dv-punct"');
  });
});
