import { describe, test, expect } from "bun:test";
import { SemanticZoneTracker } from "./semantic-zone";

describe("SemanticZoneTracker", () => {
  test("add and retrieve markers", () => {
    const tracker = new SemanticZoneTracker();
    tracker.addMarker("A", 0);
    tracker.addMarker("B", 0);
    tracker.addMarker("C", 1);
    tracker.addMarker("D", 5, 0);

    const markers = tracker.getMarkers();
    expect(markers.length).toBe(4);
    expect(markers[0]).toEqual({ type: "A", lineIndex: 0 });
    expect(markers[3]).toEqual({ type: "D", lineIndex: 5, exitCode: 0 });
  });

  test("getPromptMarkers returns only type A markers", () => {
    const tracker = new SemanticZoneTracker();
    tracker.addMarker("A", 0);
    tracker.addMarker("B", 1);
    tracker.addMarker("C", 2);
    tracker.addMarker("A", 10);
    tracker.addMarker("D", 15, 0);

    const prompts = tracker.getPromptMarkers();
    expect(prompts.length).toBe(2);
    expect(prompts[0].lineIndex).toBe(0);
    expect(prompts[1].lineIndex).toBe(10);
  });

  test("findPrevPrompt returns correct marker", () => {
    const tracker = new SemanticZoneTracker();
    tracker.addMarker("A", 0);
    tracker.addMarker("A", 10);
    tracker.addMarker("A", 20);

    const result = tracker.findPrevPrompt(15);
    expect(result).not.toBeNull();
    expect(result!.lineIndex).toBe(10);
  });

  test("findPrevPrompt returns marker at exact line", () => {
    const tracker = new SemanticZoneTracker();
    tracker.addMarker("A", 0);
    tracker.addMarker("A", 10);
    tracker.addMarker("A", 20);

    // When at line 20, should find marker at line 10 (previous, not current)
    const result = tracker.findPrevPrompt(20);
    expect(result).not.toBeNull();
    expect(result!.lineIndex).toBe(10);
  });

  test("findNextPrompt returns correct marker", () => {
    const tracker = new SemanticZoneTracker();
    tracker.addMarker("A", 0);
    tracker.addMarker("A", 10);
    tracker.addMarker("A", 20);

    const result = tracker.findNextPrompt(5);
    expect(result).not.toBeNull();
    expect(result!.lineIndex).toBe(10);
  });

  test("findNextPrompt returns marker after exact line", () => {
    const tracker = new SemanticZoneTracker();
    tracker.addMarker("A", 0);
    tracker.addMarker("A", 10);
    tracker.addMarker("A", 20);

    // When at line 10, should find marker at line 20 (next, not current)
    const result = tracker.findNextPrompt(10);
    expect(result).not.toBeNull();
    expect(result!.lineIndex).toBe(20);
  });

  test("findPrevPrompt returns null when none above", () => {
    const tracker = new SemanticZoneTracker();
    tracker.addMarker("A", 10);
    tracker.addMarker("A", 20);

    const result = tracker.findPrevPrompt(5);
    expect(result).toBeNull();
  });

  test("findNextPrompt returns null when none below", () => {
    const tracker = new SemanticZoneTracker();
    tracker.addMarker("A", 0);
    tracker.addMarker("A", 10);

    const result = tracker.findNextPrompt(15);
    expect(result).toBeNull();
  });

  test("findPrevPrompt returns null for empty tracker", () => {
    const tracker = new SemanticZoneTracker();
    expect(tracker.findPrevPrompt(10)).toBeNull();
  });

  test("findNextPrompt returns null for empty tracker", () => {
    const tracker = new SemanticZoneTracker();
    expect(tracker.findNextPrompt(10)).toBeNull();
  });

  test("pruneBeforeLine removes old markers and adjusts indices", () => {
    const tracker = new SemanticZoneTracker();
    tracker.addMarker("A", 0);
    tracker.addMarker("B", 5);
    tracker.addMarker("A", 10);
    tracker.addMarker("C", 15);

    // Prune markers before line 8, adjust remaining indices by -8
    tracker.pruneBeforeLine(8);

    const markers = tracker.getMarkers();
    expect(markers.length).toBe(2);
    expect(markers[0]).toEqual({ type: "A", lineIndex: 2 }); // 10 - 8
    expect(markers[1]).toEqual({ type: "C", lineIndex: 7 }); // 15 - 8
  });

  test("pruneBeforeLine with no markers to remove", () => {
    const tracker = new SemanticZoneTracker();
    tracker.addMarker("A", 10);
    tracker.addMarker("A", 20);

    tracker.pruneBeforeLine(5);

    const markers = tracker.getMarkers();
    expect(markers.length).toBe(2);
    expect(markers[0].lineIndex).toBe(5); // 10 - 5
    expect(markers[1].lineIndex).toBe(15); // 20 - 5
  });

  test("clear removes all markers", () => {
    const tracker = new SemanticZoneTracker();
    tracker.addMarker("A", 0);
    tracker.addMarker("A", 10);

    tracker.clear();

    expect(tracker.getMarkers().length).toBe(0);
    expect(tracker.findPrevPrompt(5)).toBeNull();
    expect(tracker.findNextPrompt(5)).toBeNull();
  });

  test("markers with non-A types are ignored by find methods", () => {
    const tracker = new SemanticZoneTracker();
    tracker.addMarker("B", 5);
    tracker.addMarker("C", 10);
    tracker.addMarker("D", 15, 0);

    expect(tracker.findPrevPrompt(20)).toBeNull();
    expect(tracker.findNextPrompt(0)).toBeNull();
  });
});
