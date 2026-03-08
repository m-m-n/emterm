/**
 * Tests for file-drop-handler module.
 */

import { describe, test, expect } from "bun:test";
import { formatPathsForPaste } from "./file-drop-handler";

describe("formatPathsForPaste", () => {
  test("should format a single path", () => {
    const result = formatPathsForPaste(["/home/user/file.txt"]);
    expect(result).toBe("/home/user/file.txt");
  });

  test("should format multiple paths space-separated", () => {
    const result = formatPathsForPaste([
      "/home/user/file1.txt",
      "/home/user/file2.txt",
    ]);
    expect(result).toBe("/home/user/file1.txt /home/user/file2.txt");
  });

  test("should quote paths with spaces", () => {
    const result = formatPathsForPaste(["/home/user/my file.txt"]);
    expect(result).toBe('"/home/user/my file.txt"');
  });

  test("should handle mixed paths with and without spaces", () => {
    const result = formatPathsForPaste([
      "/home/user/simple.txt",
      "/home/user/path with spaces/file.txt",
    ]);
    expect(result).toBe(
      '/home/user/simple.txt "/home/user/path with spaces/file.txt"',
    );
  });

  test("should handle empty array", () => {
    const result = formatPathsForPaste([]);
    expect(result).toBe("");
  });
});
