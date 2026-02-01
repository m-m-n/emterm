/**
 * Tests for URL detector
 */

import { describe, test, expect } from "bun:test";
import { detectUrls, findUrlAtPosition } from "./url-detector";

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
