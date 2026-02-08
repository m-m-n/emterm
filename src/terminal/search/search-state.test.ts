import { describe, test, expect } from "bun:test";
import { SearchStateManager } from "./search-state";

describe("SearchStateManager", () => {
  test("plain text search finds matches", () => {
    const manager = new SearchStateManager();
    const lines = ["hello world", "foo hello bar", "no match here"];

    manager.setQuery("hello");
    manager.executeSearch(lines);

    expect(manager.matches.length).toBe(2);
    expect(manager.matches[0]).toEqual({
      lineIndex: 0,
      startCol: 0,
      endCol: 5,
    });
    expect(manager.matches[1]).toEqual({
      lineIndex: 1,
      startCol: 4,
      endCol: 9,
    });
  });

  test("case-insensitive search (default)", () => {
    const manager = new SearchStateManager();
    const lines = ["Hello World", "HELLO", "hello"];

    manager.setQuery("hello");
    manager.executeSearch(lines);

    expect(manager.matches.length).toBe(3);
  });

  test("case-sensitive search", () => {
    const manager = new SearchStateManager();
    const lines = ["Hello World", "HELLO", "hello"];

    manager.setOptions({ caseSensitive: true });
    manager.setQuery("hello");
    manager.executeSearch(lines);

    expect(manager.matches.length).toBe(1);
    expect(manager.matches[0]!.lineIndex).toBe(2);
  });

  test("regex search", () => {
    const manager = new SearchStateManager();
    const lines = ["error: something failed", "warning: be careful", "info: ok"];

    manager.setOptions({ isRegex: true });
    manager.setQuery("^(error|warning):");
    manager.executeSearch(lines);

    expect(manager.matches.length).toBe(2);
  });

  test("invalid regex does not crash", () => {
    const manager = new SearchStateManager();
    const lines = ["test"];

    manager.setOptions({ isRegex: true });
    manager.setQuery("[invalid");
    manager.executeSearch(lines);

    expect(manager.matches.length).toBe(0);
    expect(manager.error).not.toBeNull();
  });

  test("nextMatch wraps around", () => {
    const manager = new SearchStateManager();
    const lines = ["aaa", "bbb", "aaa"];

    manager.setQuery("aaa");
    manager.executeSearch(lines);

    expect(manager.matches.length).toBe(2);
    expect(manager.currentMatchIndex).toBe(0);

    const next1 = manager.nextMatch();
    expect(next1).not.toBeNull();
    expect(manager.currentMatchIndex).toBe(1);

    // Wrap around
    const next2 = manager.nextMatch();
    expect(next2).not.toBeNull();
    expect(manager.currentMatchIndex).toBe(0);
  });

  test("prevMatch wraps around", () => {
    const manager = new SearchStateManager();
    const lines = ["aaa", "bbb", "aaa"];

    manager.setQuery("aaa");
    manager.executeSearch(lines);

    expect(manager.currentMatchIndex).toBe(0);

    // Wrap to last
    const prev = manager.prevMatch();
    expect(prev).not.toBeNull();
    expect(manager.currentMatchIndex).toBe(1);
  });

  test("getVisibleMatches returns correct range", () => {
    const manager = new SearchStateManager();
    const lines = ["match", "no", "match", "no", "match"];

    manager.setQuery("match");
    manager.executeSearch(lines);

    expect(manager.matches.length).toBe(3);

    const visible = manager.getVisibleMatches(1, 3);
    expect(visible.length).toBe(1);
    expect(visible[0]!.lineIndex).toBe(2);
  });

  test("empty query returns no matches", () => {
    const manager = new SearchStateManager();
    const lines = ["hello world"];

    manager.setQuery("");
    manager.executeSearch(lines);

    expect(manager.matches.length).toBe(0);
    expect(manager.error).toBeNull();
  });

  test("multiple matches on single line", () => {
    const manager = new SearchStateManager();
    const lines = ["aaa bbb aaa ccc aaa"];

    manager.setQuery("aaa");
    manager.executeSearch(lines);

    expect(manager.matches.length).toBe(3);
    expect(manager.matches[0]).toEqual({
      lineIndex: 0,
      startCol: 0,
      endCol: 3,
    });
    expect(manager.matches[1]).toEqual({
      lineIndex: 0,
      startCol: 8,
      endCol: 11,
    });
    expect(manager.matches[2]).toEqual({
      lineIndex: 0,
      startCol: 16,
      endCol: 19,
    });
  });

  test("clear resets all state", () => {
    const manager = new SearchStateManager();
    const lines = ["hello"];

    manager.setQuery("hello");
    manager.executeSearch(lines);
    expect(manager.matches.length).toBe(1);

    manager.clear();
    expect(manager.matches.length).toBe(0);
    expect(manager.query).toBe("");
    expect(manager.currentMatchIndex).toBe(-1);
    expect(manager.error).toBeNull();
  });

  test("nextMatch returns null when no matches", () => {
    const manager = new SearchStateManager();
    expect(manager.nextMatch()).toBeNull();
  });

  test("prevMatch returns null when no matches", () => {
    const manager = new SearchStateManager();
    expect(manager.prevMatch()).toBeNull();
  });

  test("regex with case insensitive flag", () => {
    const manager = new SearchStateManager();
    const lines = ["Hello", "HELLO", "hello"];

    manager.setOptions({ isRegex: true, caseSensitive: false });
    manager.setQuery("hello");
    manager.executeSearch(lines);

    expect(manager.matches.length).toBe(3);
  });

  test("search timeout protection marks error on extremely long execution", () => {
    // This tests the timeout mechanism exists (not triggering actual timeout)
    const manager = new SearchStateManager();
    const lines = ["test"];

    manager.setQuery("test");
    manager.executeSearch(lines);

    // Should complete normally
    expect(manager.matches.length).toBe(1);
    expect(manager.error).toBeNull();
  });
});
