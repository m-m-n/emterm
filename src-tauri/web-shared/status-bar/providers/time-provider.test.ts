import { describe, test, expect } from "bun:test";
import { TimeProvider, formatTime } from "./time-provider";

describe("formatTime", () => {
  // Use a fixed date: 2026-03-25 14:05:09
  const testDate = new Date(2026, 2, 25, 14, 5, 9);

  test("should format HH:mm:ss", () => {
    expect(formatTime(testDate, "HH:mm:ss")).toBe("14:05:09");
  });

  test("should format HH:mm", () => {
    expect(formatTime(testDate, "HH:mm")).toBe("14:05");
  });

  test("should format hh:mm:ss (12-hour)", () => {
    expect(formatTime(testDate, "hh:mm:ss")).toBe("02:05:09");
  });

  test("should format hh:mm A (12-hour with AM/PM)", () => {
    expect(formatTime(testDate, "hh:mm A")).toBe("02:05 PM");
  });

  test("should handle midnight in 12-hour format", () => {
    const midnight = new Date(2026, 2, 25, 0, 0, 0);
    expect(formatTime(midnight, "hh:mm A")).toBe("12:00 AM");
  });

  test("should handle noon in 12-hour format", () => {
    const noon = new Date(2026, 2, 25, 12, 0, 0);
    expect(formatTime(noon, "hh:mm A")).toBe("12:00 PM");
  });

  test("should format YYYY-MM-DD", () => {
    expect(formatTime(testDate, "YYYY-MM-DD")).toBe("2026-03-25");
  });

  test("should handle unknown tokens as literal text", () => {
    expect(formatTime(testDate, "Time: HH:mm")).toBe("Time: 14:05");
  });

  test("should fall back to default format for empty string", () => {
    expect(formatTime(testDate, "")).toBe("14:05:09");
  });
});

describe("TimeProvider", () => {
  test("should return formatted time", () => {
    const provider = new TimeProvider("HH:mm:ss");
    const value = provider.getValue();
    // Should match HH:mm:ss pattern
    expect(value).toMatch(/^\d{2}:\d{2}:\d{2}$/);
    provider.dispose();
  });

  test("should use custom format", () => {
    const provider = new TimeProvider("HH:mm");
    const value = provider.getValue();
    expect(value).toMatch(/^\d{2}:\d{2}$/);
    provider.dispose();
  });

  test("should not have color", () => {
    const provider = new TimeProvider("HH:mm:ss");
    expect(provider.getColor()).toBeNull();
    provider.dispose();
  });
});
