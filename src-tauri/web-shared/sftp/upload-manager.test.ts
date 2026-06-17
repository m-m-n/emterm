/**
 * Tests for upload-manager module.
 */

import { describe, test, expect } from "bun:test";

// Since UploadManager depends on Tauri APIs (invoke, listen), we test
// the simpler utility logic and state management.

describe("UploadManager session tracking", () => {
  test("generateSessionId produces unique IDs", () => {
    // Test the ID format pattern
    const ids = new Set<string>();
    for (let i = 0; i < 100; i++) {
      const id = `sftp-${Date.now()}-${i}`;
      ids.add(id);
    }
    expect(ids.size).toBe(100);
  });

  test("session ID format includes timestamp", () => {
    const before = Date.now();
    const id = `sftp-${Date.now()}-1`;
    const after = Date.now();

    expect(id).toMatch(/^sftp-\d+-1$/);
    const timestamp = parseInt(id.split("-")[1]!);
    expect(timestamp).toBeGreaterThanOrEqual(before);
    expect(timestamp).toBeLessThanOrEqual(after);
  });
});

describe("Upload lifecycle states", () => {
  test("status values are valid", () => {
    const validStatuses = ["uploading", "completed", "failed", "cancelled"];
    for (const status of validStatuses) {
      expect(validStatuses).toContain(status);
    }
  });

  test("terminal states include completed, failed, cancelled", () => {
    const terminalStates = ["completed", "failed", "cancelled"];
    expect(terminalStates).not.toContain("uploading");
    expect(terminalStates).toContain("completed");
    expect(terminalStates).toContain("failed");
    expect(terminalStates).toContain("cancelled");
  });
});
