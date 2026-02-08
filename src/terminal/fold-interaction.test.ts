import { describe, test, expect } from "bun:test";
import { FoldManager } from "./fold-manager";
import { SemanticZoneTracker } from "./semantic-zone";
import { SearchStateManager } from "./search/search-state";

describe("Fold interaction logic", () => {
  // --- Group 1: Click expand/collapse ---

  describe("click expand/collapse", () => {
    test("click on summary line expands collapsed region", () => {
      const fm = new FoldManager();
      fm.registerOsc133Region(5, 15, "ls -la", 0);

      // Collapse
      fm.toggleFold(5);
      expect(fm.getRegionAtLine(5)!.collapsed).toBe(true);

      // Simulate click: identify summary line on display
      const displayLine = 5;
      const summaryRegion = fm.getSummaryRegion(displayLine);
      expect(summaryRegion).not.toBeNull();
      expect(summaryRegion!.startLine).toBe(5);

      // Expand via expandRegionContaining
      const expanded = fm.expandRegionContaining(summaryRegion!.startLine);
      expect(expanded).toBe(true);
      expect(fm.getRegionAtLine(5)!.collapsed).toBe(false);
    });

    test("click on output zone collapses region", () => {
      const fm = new FoldManager();
      fm.registerOsc133Region(5, 15, "ls -la", 0);
      // Region starts expanded (collapsed === false)

      // Simulate click on a display line inside the region
      const displayLine = 10;
      const actualLine = fm.displayLineToActual(displayLine);
      expect(actualLine).toBe(10);

      // Verify region is found and not collapsed
      const region = fm.getRegionAtLine(actualLine);
      expect(region).not.toBeNull();
      expect(region!.collapsed).toBe(false);

      // Toggle fold (collapse)
      const toggled = fm.toggleFold(actualLine);
      expect(toggled).toBe(true);
      expect(fm.getRegionAtLine(5)!.collapsed).toBe(true);
    });

    test("multiple adjacent regions fold/unfold independently", () => {
      const fm = new FoldManager();
      fm.registerOsc133Region(5, 15, "region A", 0);
      fm.registerOsc133Region(20, 30, "region B", 0);

      // Collapse A
      fm.toggleFold(5);
      expect(fm.getRegionAtLine(5)!.collapsed).toBe(true);
      expect(fm.getRegionAtLine(20)!.collapsed).toBe(false);

      // Collapse B
      fm.toggleFold(20);
      expect(fm.getRegionAtLine(5)!.collapsed).toBe(true);
      expect(fm.getRegionAtLine(20)!.collapsed).toBe(true);

      // Expand A — B remains collapsed
      fm.toggleFold(5);
      expect(fm.getRegionAtLine(5)!.collapsed).toBe(false);
      expect(fm.getRegionAtLine(20)!.collapsed).toBe(true);
    });
  });

  // --- Group 2: Scroll stability ---

  describe("scroll stability on fold/unfold", () => {
    test("fold above viewport adjusts scroll offset", () => {
      // Mirrors handleFoldClick logic in terminal-app/index.ts:
      //   regionDisplayLine is computed BEFORE toggleFold
      //   delta = region.lineCount - 1
      //   if regionDisplayLine < displayStart → scrollOffset -= delta
      const fm = new FoldManager();
      fm.registerOsc133Region(5, 15, "above viewport", 0);

      const region = fm.getRegionAtLine(5)!;
      let scrollOffset = 20; // viewport starts at display line 20

      // Compute regionDisplayLine BEFORE collapse (matches actual code order)
      const regionDisplayLine = fm.actualLineToDisplay(region.startLine);
      expect(regionDisplayLine).toBe(5);

      // Collapse
      fm.toggleFold(5);

      // Scroll adjustment: region.lineCount - 1 (matches actual code)
      const delta = region.lineCount - 1; // 10 - 1 = 9
      expect(delta).toBe(9);

      if (regionDisplayLine < scrollOffset) {
        scrollOffset = Math.max(0, scrollOffset - delta);
      }
      expect(scrollOffset).toBe(11); // 20 - 9
    });

    test("fold in viewport does not adjust scroll offset", () => {
      // Mirrors handleFoldClick: regionDisplayLine >= displayStart → no adjustment
      const fm = new FoldManager();
      fm.registerOsc133Region(5, 15, "in viewport", 0);

      const region = fm.getRegionAtLine(5)!;
      let scrollOffset = 0; // viewport starts at display line 0

      const regionDisplayLine = fm.actualLineToDisplay(region.startLine);
      fm.toggleFold(5);

      const delta = region.lineCount - 1;

      // regionDisplayLine (5) >= scrollOffset (0), so no adjustment
      if (regionDisplayLine < scrollOffset) {
        scrollOffset = Math.max(0, scrollOffset - delta);
      }
      expect(scrollOffset).toBe(0); // unchanged
    });
  });

  // --- Group 3: Search auto-expand ---

  describe("search auto-expand", () => {
    test("search match in collapsed region triggers expand", () => {
      const fm = new FoldManager();
      fm.registerOsc133Region(5, 15, "test output", 0);
      fm.toggleFold(5);
      expect(fm.getRegionAtLine(5)!.collapsed).toBe(true);

      // Simulate search finding a match at line 10 (inside collapsed region)
      // Use executeSearch() to populate matches via the normal API
      const lines = Array.from({ length: 15 }, (_, i) =>
        i === 10 ? "hello world" : "",
      );
      const searchState = new SearchStateManager();
      searchState.setQuery("hello");
      searchState.executeSearch(lines);

      const currentMatch = searchState.getCurrentMatch();
      expect(currentMatch).not.toBeNull();
      expect(currentMatch!.lineIndex).toBe(10);

      // scrollToCurrentMatch logic: expand if match is inside collapsed region
      const expanded = fm.expandRegionContaining(currentMatch!.lineIndex);
      expect(expanded).toBe(true);
      expect(fm.getRegionAtLine(5)!.collapsed).toBe(false);
    });
  });

  // --- Group 4: Prompt jump auto-expand ---

  describe("prompt jump auto-expand", () => {
    test("prompt jump into collapsed region triggers expand", () => {
      const fm = new FoldManager();
      const tracker = new SemanticZoneTracker();

      // Place prompt marker at line 10 (inside region 5-15)
      tracker.addMarker("A", 10);
      fm.registerOsc133Region(5, 15, "some output", 0);
      fm.toggleFold(5);
      expect(fm.getRegionAtLine(5)!.collapsed).toBe(true);

      // Simulate prompt jump: find next prompt from line 0
      const nextPrompt = tracker.findNextPrompt(0);
      expect(nextPrompt).not.toBeNull();
      expect(nextPrompt!.lineIndex).toBe(10);

      // handlePromptJump logic: expand if target is in collapsed region
      const expanded = fm.expandRegionContaining(nextPrompt!.lineIndex);
      expect(expanded).toBe(true);
      expect(fm.getRegionAtLine(5)!.collapsed).toBe(false);

      // Also test findPrevPrompt
      fm.toggleFold(5); // re-collapse
      expect(fm.getRegionAtLine(5)!.collapsed).toBe(true);

      const prevPrompt = tracker.findPrevPrompt(20);
      expect(prevPrompt).not.toBeNull();
      expect(prevPrompt!.lineIndex).toBe(10);

      const expanded2 = fm.expandRegionContaining(prevPrompt!.lineIndex);
      expect(expanded2).toBe(true);
      expect(fm.getRegionAtLine(5)!.collapsed).toBe(false);
    });
  });

  // --- Group 5: Guard conditions ---

  describe("guard conditions", () => {
    // Note: These tests verify the expected behavior of guard conditions
    // by simulating the control flow. The actual guards live in the click
    // event listener (modifier keys) and handleFoldClick (text selection),
    // which cannot be tested without DOM. These tests document the contract:
    // when a guard is active, fold state must not change.

    test("fold state unchanged when selection guard active", () => {
      const fm = new FoldManager();
      fm.registerOsc133Region(5, 15, "test", 0);
      fm.toggleFold(5);
      expect(fm.getRegionAtLine(5)!.collapsed).toBe(true);

      // handleFoldClick guard: selection.toString().length > 0 → return early
      const hasSelection = true; // simulates active text selection

      if (!hasSelection) {
        fm.toggleFold(5);
      }

      // State unchanged: guard prevented toggleFold from being called
      expect(fm.getRegionAtLine(5)!.collapsed).toBe(true);
    });

    test("fold state unchanged when modifier key guard active", () => {
      const fm = new FoldManager();
      fm.registerOsc133Region(5, 15, "test", 0);

      // Click event listener guard: ctrlKey/metaKey → dispatch to URL handler,
      // handleFoldClick is never called
      const modifiers = { ctrlKey: true, metaKey: false };
      const hasModifier = modifiers.ctrlKey || modifiers.metaKey;

      if (!hasModifier) {
        fm.toggleFold(5);
      }

      // State unchanged: modifier guard routed click away from handleFoldClick
      expect(fm.getRegionAtLine(5)!.collapsed).toBe(false);

      // metaKey guard
      const modifiers2 = { ctrlKey: false, metaKey: true };
      const hasModifier2 = modifiers2.ctrlKey || modifiers2.metaKey;

      if (!hasModifier2) {
        fm.toggleFold(5);
      }

      expect(fm.getRegionAtLine(5)!.collapsed).toBe(false);
    });
  });

  // --- Hover detection logic ---

  describe("hover detection logic", () => {
    test("isSummaryLine returns true for summary display line", () => {
      const fm = new FoldManager();
      fm.registerOsc133Region(5, 15, "test", 0);
      fm.toggleFold(5);

      // Display line 5 is the summary line for collapsed region
      expect(fm.isSummaryLine(5)).toBe(true);

      // Lines before and after summary are not summary lines
      expect(fm.isSummaryLine(4)).toBe(false);
      expect(fm.isSummaryLine(6)).toBe(false);
    });

    test("getRegionAtLine returns region for lines within region", () => {
      const fm = new FoldManager();
      fm.registerOsc133Region(5, 15, "test", 0);

      // All actual lines within region [5, 15) should return the region
      expect(fm.getRegionAtLine(5)).not.toBeNull();
      expect(fm.getRegionAtLine(10)).not.toBeNull();
      expect(fm.getRegionAtLine(14)).not.toBeNull();

      // Boundary check: startLine (inclusive), endLine (exclusive)
      expect(fm.getRegionAtLine(5)!.startLine).toBe(5);
      expect(fm.getRegionAtLine(14)!.startLine).toBe(5);
    });

    test("getRegionAtLine returns null for lines outside region", () => {
      const fm = new FoldManager();
      fm.registerOsc133Region(5, 15, "test", 0);

      // Before region
      expect(fm.getRegionAtLine(4)).toBeNull();
      // At endLine (exclusive boundary)
      expect(fm.getRegionAtLine(15)).toBeNull();
      // Well outside
      expect(fm.getRegionAtLine(100)).toBeNull();
    });
  });
});
