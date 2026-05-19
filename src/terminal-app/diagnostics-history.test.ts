import { describe, test, expect, beforeEach } from "bun:test";
import {
  recordHeapSample,
  recordEvent,
  formatHeapHistory,
  formatEventTimeline,
  snapshotForCrash,
  resetDiagnosticsHistoryForTests,
} from "./diagnostics-history";

beforeEach(() => {
  resetDiagnosticsHistoryForTests();
});

describe("diagnostics-history: heap samples", () => {
  test("empty history formats as empty bracket", () => {
    expect(formatHeapHistory(1000)).toBe("heap=[]");
  });

  test("formats a sample with ago/heap/recv fields", () => {
    recordHeapSample({ t: 900, heapMB: 47, recvCount: 131769, recvBytes: 49024592 });
    expect(formatHeapHistory(1000)).toBe(`heap=[-100ms:47MB/131769c/47876KB]`);
  });

  test("keeps the most recent samples after exceeding the cap", () => {
    // Push 15 samples — capacity is 12, so the first 3 must be evicted.
    for (let i = 0; i < 15; i++) {
      recordHeapSample({ t: i * 5_000, heapMB: i, recvCount: i, recvBytes: i * 1024 });
    }
    const line = formatHeapHistory(15 * 5_000);
    // Oldest retained sample is t=3*5000 (i=3 was the 4th push) with heapMB=3.
    expect(line).toContain("-60000ms:3MB/3c/3KB");
    // Freshest is t=14*5000 = 70000 with heapMB=14.
    expect(line).toContain("-5000ms:14MB/14c/14KB");
    // Evicted samples (heapMB=0..2) must not appear.
    expect(line).not.toContain(":0MB");
    expect(line).not.toContain(":1MB");
    expect(line).not.toContain(":2MB");
  });
});

describe("diagnostics-history: timeline events", () => {
  test("empty timeline formats as empty bracket", () => {
    expect(formatEventTimeline(1000)).toBe("events=[]");
  });

  test("formats events with kind+detail and time offset", () => {
    recordEvent("visibility", "visible (initial)");
    const line = formatEventTimeline();
    expect(line).toContain("visibility:visible (initial)");
  });

  test("retains the most recent 32 events", () => {
    for (let i = 0; i < 40; i++) {
      recordEvent("mux-switch", `e${i}`);
    }
    const line = formatEventTimeline();
    // First 8 events (e0..e7) must be evicted.
    for (let i = 0; i < 8; i++) {
      expect(line).not.toContain(`e${i}:`);
    }
    // Most recent must remain.
    expect(line).toContain("mux-switch:e39");
  });
});

describe("diagnostics-history: combined snapshot", () => {
  test("snapshotForCrash includes both heap and events", () => {
    recordHeapSample({ t: 0, heapMB: 12, recvCount: 1, recvBytes: 0 });
    recordEvent("recovery", "attempt=1 manual=false");
    const snap = snapshotForCrash(0);
    expect(snap).toContain("heap=[");
    expect(snap).toContain("events=[");
    expect(snap).toContain("12MB");
    expect(snap).toContain("recovery:attempt=1 manual=false");
  });
});
