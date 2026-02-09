/**
 * Tests for URL detector
 */

import { describe, test, expect } from "bun:test";
import {
  detectUrls,
  findUrlAtPosition,
  detectFilePaths,
  findFilePathAtPosition,
} from "./url-detector";

describe("detectUrls", () => {
  test("should detect https URL", () => {
    const matches = detectUrls("Visit https://example.com for more");
    expect(matches).toHaveLength(1);
    expect(matches[0].url).toBe("https://example.com");
    expect(matches[0].startCol).toBe(6);
    expect(matches[0].endCol).toBe(25);
  });

  test("should detect http URL with path and query", () => {
    const matches = detectUrls("Link: http://example.com/path?q=1&r=2");
    expect(matches).toHaveLength(1);
    expect(matches[0].url).toBe("http://example.com/path?q=1&r=2");
  });

  test("should detect ftp URL", () => {
    const matches = detectUrls("Download from ftp://files.example.com/pub");
    expect(matches).toHaveLength(1);
    expect(matches[0].url).toBe("ftp://files.example.com/pub");
  });

  test("should detect file URL", () => {
    const matches = detectUrls("Open file:///tmp/file.txt");
    expect(matches).toHaveLength(1);
    expect(matches[0].url).toBe("file:///tmp/file.txt");
  });

  test("should return empty for no URLs", () => {
    const matches = detectUrls("This is just plain text");
    expect(matches).toHaveLength(0);
  });

  test("should detect multiple URLs on one line", () => {
    const matches = detectUrls("See https://a.com and https://b.com/path");
    expect(matches).toHaveLength(2);
    expect(matches[0].url).toBe("https://a.com");
    expect(matches[1].url).toBe("https://b.com/path");
  });

  test("should trim trailing punctuation", () => {
    const matches = detectUrls("Check https://example.com.");
    expect(matches).toHaveLength(1);
    expect(matches[0].url).toBe("https://example.com");
  });

  test("should trim trailing parenthesis", () => {
    const matches = detectUrls("(https://example.com/path)");
    expect(matches).toHaveLength(1);
    expect(matches[0].url).toBe("https://example.com/path");
  });

  test("should handle URL with hash fragment", () => {
    const matches = detectUrls("See https://example.com/page#section");
    expect(matches).toHaveLength(1);
    expect(matches[0].url).toBe("https://example.com/page#section");
  });
});

describe("findUrlAtPosition", () => {
  test("should find URL at position within URL", () => {
    const url = findUrlAtPosition("Visit https://example.com here", 10);
    expect(url).toBe("https://example.com");
  });

  test("should return null when position is outside URL", () => {
    const url = findUrlAtPosition("Visit https://example.com here", 0);
    expect(url).toBeNull();
  });

  test("should return null when no URLs exist", () => {
    const url = findUrlAtPosition("Just plain text", 5);
    expect(url).toBeNull();
  });

  test("should find correct URL when multiple exist", () => {
    const text = "See https://a.com and https://b.com";
    expect(findUrlAtPosition(text, 5)).toBe("https://a.com");
    expect(findUrlAtPosition(text, 22)).toBe("https://b.com");
  });
});

// ============================================================
// File Path Detection Tests
// ============================================================

describe("detectFilePaths", () => {
  test("FP-01: relative path with line number", () => {
    const matches = detectFilePaths("src/foo.ts:42");
    expect(matches).toHaveLength(1);
    expect(matches[0].path).toBe("src/foo.ts");
    expect(matches[0].line).toBe(42);
    expect(matches[0].col).toBe(1);
  });

  test("FP-02: relative path with line and column", () => {
    const matches = detectFilePaths("src/foo.ts:42:10");
    expect(matches).toHaveLength(1);
    expect(matches[0].path).toBe("src/foo.ts");
    expect(matches[0].line).toBe(42);
    expect(matches[0].col).toBe(10);
  });

  test("FP-03: absolute path", () => {
    const matches = detectFilePaths("/home/user/file.rs:10");
    expect(matches).toHaveLength(1);
    expect(matches[0].path).toBe("/home/user/file.rs");
    expect(matches[0].line).toBe(10);
    expect(matches[0].col).toBe(1);
  });

  test("FP-04: dot-relative path", () => {
    const matches = detectFilePaths("./src/foo.ts:42");
    expect(matches).toHaveLength(1);
    expect(matches[0].path).toBe("./src/foo.ts");
    expect(matches[0].line).toBe(42);
  });

  test("FP-05: parent-relative path", () => {
    const matches = detectFilePaths("../lib/bar.py:5:3");
    expect(matches).toHaveLength(1);
    expect(matches[0].path).toBe("../lib/bar.py");
    expect(matches[0].line).toBe(5);
    expect(matches[0].col).toBe(3);
  });

  test("FP-06: should NOT detect http URL", () => {
    const matches = detectFilePaths("http://example.com:8080");
    expect(matches).toHaveLength(0);
  });

  test("FP-07: should NOT detect https URL", () => {
    const matches = detectFilePaths("https://example.com/path:443");
    expect(matches).toHaveLength(0);
  });

  test("FP-08: should NOT detect time pattern", () => {
    const matches = detectFilePaths("12:30:45");
    expect(matches).toHaveLength(0);
  });

  test("FP-09: should NOT detect path without line number", () => {
    const matches = detectFilePaths("foo.ts");
    expect(matches).toHaveLength(0);
  });

  test("FP-10: path at end of line", () => {
    const matches = detectFilePaths("error in src/foo.ts:42");
    expect(matches).toHaveLength(1);
    expect(matches[0].path).toBe("src/foo.ts");
    expect(matches[0].line).toBe(42);
  });

  test("FP-11: path at start of line", () => {
    const matches = detectFilePaths("src/foo.ts:42: error msg");
    expect(matches).toHaveLength(1);
    expect(matches[0].path).toBe("src/foo.ts");
    expect(matches[0].line).toBe(42);
  });

  test("FP-12: multiple paths on one line", () => {
    const matches = detectFilePaths(
      "errors in src/foo.ts:42 and src/bar.rs:10:5",
    );
    expect(matches).toHaveLength(2);
    expect(matches[0].path).toBe("src/foo.ts");
    expect(matches[0].line).toBe(42);
    expect(matches[1].path).toBe("src/bar.rs");
    expect(matches[1].line).toBe(10);
    expect(matches[1].col).toBe(5);
  });

  test("should have correct startCol and endCol", () => {
    const matches = detectFilePaths("error in src/foo.ts:42:10 here");
    expect(matches).toHaveLength(1);
    expect(matches[0].startCol).toBe(9);
    expect(matches[0].endCol).toBe(25); // "src/foo.ts:42:10".length = 16, 9+16=25
  });

  test("should NOT detect ftp URL", () => {
    const matches = detectFilePaths("ftp://files.example.com:21/pub");
    expect(matches).toHaveLength(0);
  });

  test("should NOT detect file URL", () => {
    const matches = detectFilePaths("file:///tmp/test.txt:10");
    expect(matches).toHaveLength(0);
  });

  test("should detect path with @ in directory name", () => {
    const matches = detectFilePaths("node_modules/@types/node/index.d.ts:42");
    expect(matches).toHaveLength(1);
    expect(matches[0].path).toBe("node_modules/@types/node/index.d.ts");
    expect(matches[0].line).toBe(42);
  });

  test("should detect path with hyphen in filename", () => {
    const matches = detectFilePaths("src/my-component.tsx:15");
    expect(matches).toHaveLength(1);
    expect(matches[0].path).toBe("src/my-component.tsx");
    expect(matches[0].line).toBe(15);
  });

  test("should detect deeply nested path", () => {
    const matches = detectFilePaths(
      "src/terminal/handlers/osc_handlers.ts:123",
    );
    expect(matches).toHaveLength(1);
    expect(matches[0].path).toBe("src/terminal/handlers/osc_handlers.ts");
    expect(matches[0].line).toBe(123);
  });

  test("should trim trailing punctuation from match", () => {
    // Common in error messages: "src/foo.ts:42."
    const matches = detectFilePaths("error at src/foo.ts:42.");
    expect(matches).toHaveLength(1);
    expect(matches[0].path).toBe("src/foo.ts");
    expect(matches[0].line).toBe(42);
  });

  test("should handle path in parentheses", () => {
    const matches = detectFilePaths("(src/foo.ts:42)");
    expect(matches).toHaveLength(1);
    expect(matches[0].path).toBe("src/foo.ts");
    expect(matches[0].line).toBe(42);
  });
});

describe("findFilePathAtPosition", () => {
  test("FP-13: should find file path at position", () => {
    const match = findFilePathAtPosition("error in src/foo.ts:42 here", 12);
    expect(match).not.toBeNull();
    expect(match!.path).toBe("src/foo.ts");
    expect(match!.line).toBe(42);
  });

  test("FP-14: should return null when position is outside path", () => {
    const match = findFilePathAtPosition("error in src/foo.ts:42 here", 0);
    expect(match).toBeNull();
  });

  test("should return null when no file paths exist", () => {
    const match = findFilePathAtPosition("Just plain text", 5);
    expect(match).toBeNull();
  });

  test("should find correct path when multiple exist", () => {
    const text = "src/a.ts:10 and src/b.rs:20";
    const match1 = findFilePathAtPosition(text, 3);
    expect(match1).not.toBeNull();
    expect(match1!.path).toBe("src/a.ts");

    const match2 = findFilePathAtPosition(text, 18);
    expect(match2).not.toBeNull();
    expect(match2!.path).toBe("src/b.rs");
  });
});
