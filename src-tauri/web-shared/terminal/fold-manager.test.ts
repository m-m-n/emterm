import { describe, test, expect } from "bun:test";
import { FoldManager } from "./fold-manager";

describe("FoldManager", () => {
  // --- Registration ---

  test("T1-1: register OSC 133 region with correct properties", () => {
    const fm = new FoldManager();
    fm.registerOsc133Region(5, 15, "ls -la", 0);

    const region = fm.getRegionAtLine(5);
    expect(region).not.toBeNull();
    expect(region!.source).toBe("osc133");
    expect(region!.startLine).toBe(5);
    expect(region!.endLine).toBe(15);
    expect(region!.commandText).toBe("ls -la");
    expect(region!.exitCode).toBe(0);
    expect(region!.lineCount).toBe(10);
    expect(region!.collapsed).toBe(false);
    expect(region!.label).toBeUndefined();
  });

  test("T1-2: register custom region with label, no exitCode", () => {
    const fm = new FoldManager();
    fm.registerCustomRegion(10, 30, "Build Output");

    const region = fm.getRegionAtLine(10);
    expect(region).not.toBeNull();
    expect(region!.source).toBe("custom");
    expect(region!.label).toBe("Build Output");
    expect(region!.exitCode).toBeUndefined();
    expect(region!.commandText).toBeUndefined();
    expect(region!.lineCount).toBe(20);
  });

  test("T1-16: region with 0 lines is not registered", () => {
    const fm = new FoldManager();
    fm.registerOsc133Region(5, 5, "echo hi", 0);
    expect(fm.getRegionAtLine(5)).toBeNull();
  });

  test("T1-17: region with 1 line is registered", () => {
    const fm = new FoldManager();
    fm.registerOsc133Region(5, 6, "echo hi", 0);
    const region = fm.getRegionAtLine(5);
    expect(region).not.toBeNull();
    expect(region!.lineCount).toBe(1);
  });

  test("register OSC 133 region without exit code", () => {
    const fm = new FoldManager();
    fm.registerOsc133Region(5, 15, "running...");
    const region = fm.getRegionAtLine(5);
    expect(region).not.toBeNull();
    expect(region!.exitCode).toBeUndefined();
  });

  test("register custom region with empty label uses fallback", () => {
    const fm = new FoldManager();
    fm.registerCustomRegion(10, 20, "");
    const region = fm.getRegionAtLine(10);
    expect(region).not.toBeNull();
    expect(region!.label).toBe("...");
  });

  test("registering overlapping region does not overwrite existing", () => {
    const fm = new FoldManager();
    fm.registerOsc133Region(5, 15, "first", 0);
    fm.registerOsc133Region(8, 20, "second", 1);
    // First region still accessible
    const r = fm.getRegionAtLine(5);
    expect(r).not.toBeNull();
    expect(r!.commandText).toBe("first");
  });

  // --- Toggle ---

  test("T1-3: toggle fold collapses and expands", () => {
    const fm = new FoldManager();
    fm.registerOsc133Region(5, 15, "test", 0);

    // Collapse
    const result1 = fm.toggleFold(5);
    expect(result1).toBe(true);
    expect(fm.getRegionAtLine(5)!.collapsed).toBe(true);

    // Expand
    const result2 = fm.toggleFold(5);
    expect(result2).toBe(true);
    expect(fm.getRegionAtLine(5)!.collapsed).toBe(false);
  });

  test("T1-4: toggle fold on non-existent line returns false", () => {
    const fm = new FoldManager();
    fm.registerOsc133Region(5, 15, "test", 0);

    expect(fm.toggleFold(20)).toBe(false);
  });

  test("toggle fold on line inside region collapses it", () => {
    const fm = new FoldManager();
    fm.registerOsc133Region(5, 15, "test", 0);

    const result = fm.toggleFold(10);
    expect(result).toBe(true);
    expect(fm.getRegionAtLine(5)!.collapsed).toBe(true);
  });

  // --- getRegionAtLine ---

  test("T1-5: getRegionAtLine inside region returns region", () => {
    const fm = new FoldManager();
    fm.registerOsc133Region(5, 15, "test", 0);

    expect(fm.getRegionAtLine(5)).not.toBeNull();
    expect(fm.getRegionAtLine(10)).not.toBeNull();
    expect(fm.getRegionAtLine(14)).not.toBeNull();
  });

  test("T1-6: getRegionAtLine outside region returns null", () => {
    const fm = new FoldManager();
    fm.registerOsc133Region(5, 15, "test", 0);

    expect(fm.getRegionAtLine(4)).toBeNull();
    expect(fm.getRegionAtLine(15)).toBeNull();
    expect(fm.getRegionAtLine(100)).toBeNull();
  });

  test("getRegionAtLine with multiple regions", () => {
    const fm = new FoldManager();
    fm.registerOsc133Region(5, 15, "first", 0);
    fm.registerOsc133Region(20, 30, "second", 1);

    const r1 = fm.getRegionAtLine(10);
    expect(r1).not.toBeNull();
    expect(r1!.commandText).toBe("first");

    const r2 = fm.getRegionAtLine(25);
    expect(r2).not.toBeNull();
    expect(r2!.commandText).toBe("second");

    expect(fm.getRegionAtLine(17)).toBeNull();
  });

  // --- getCollapsedRegions ---

  test("getCollapsedRegions returns only collapsed, sorted", () => {
    const fm = new FoldManager();
    fm.registerOsc133Region(20, 30, "second", 0);
    fm.registerOsc133Region(5, 15, "first", 0);
    fm.registerOsc133Region(40, 50, "third", 0);

    fm.toggleFold(5);
    fm.toggleFold(40);

    const collapsed = fm.getCollapsedRegions();
    expect(collapsed.length).toBe(2);
    expect(collapsed[0].startLine).toBe(5);
    expect(collapsed[1].startLine).toBe(40);
  });

  // --- Line Mapping ---

  test("T1-7: displayLineToActual with 0 folds is identity", () => {
    const fm = new FoldManager();
    fm.registerOsc133Region(5, 15, "test", 0);
    // Not collapsed, so no offset
    expect(fm.displayLineToActual(0)).toBe(0);
    expect(fm.displayLineToActual(10)).toBe(10);
    expect(fm.displayLineToActual(20)).toBe(20);
  });

  test("T1-8: displayLineToActual with 1 fold", () => {
    const fm = new FoldManager();
    // Region from line 5 to 15 (10 lines)
    // When collapsed: line 5 becomes summary, lines 6-14 are hidden
    // So display line 5 = summary (actual line 5)
    // display line 6 = actual line 15
    fm.registerOsc133Region(5, 15, "test", 0);
    fm.toggleFold(5);

    // Lines before fold: identity
    expect(fm.displayLineToActual(0)).toBe(0);
    expect(fm.displayLineToActual(4)).toBe(4);

    // Line 5 = summary line for collapsed region starting at 5
    expect(fm.displayLineToActual(5)).toBe(5);

    // Line 6 should map to actual line 15 (skipping 6-14 = 9 hidden lines)
    expect(fm.displayLineToActual(6)).toBe(15);
    expect(fm.displayLineToActual(7)).toBe(16);
  });

  test("T1-9: displayLineToActual with multiple folds", () => {
    const fm = new FoldManager();
    // Region 1: lines 5-15 (10 lines, 9 hidden when collapsed)
    // Region 2: lines 25-35 (10 lines, 9 hidden when collapsed)
    fm.registerOsc133Region(5, 15, "first", 0);
    fm.registerOsc133Region(25, 35, "second", 1);
    fm.toggleFold(5);
    fm.toggleFold(25);

    // Before first fold
    expect(fm.displayLineToActual(4)).toBe(4);

    // Summary line for first fold
    expect(fm.displayLineToActual(5)).toBe(5);

    // After first fold: display 6 -> actual 15 (9 hidden)
    expect(fm.displayLineToActual(6)).toBe(15);

    // Summary line for second fold: actual line 25
    // actual 25 - 9 hidden = display 16
    expect(fm.displayLineToActual(16)).toBe(25);

    // After second fold: display 17 -> actual 35 (9 + 9 = 18 hidden total)
    expect(fm.displayLineToActual(17)).toBe(35);
  });

  test("T1-10: actualLineToDisplay", () => {
    const fm = new FoldManager();
    fm.registerOsc133Region(5, 15, "test", 0);
    fm.toggleFold(5);

    // Before fold
    expect(fm.actualLineToDisplay(0)).toBe(0);
    expect(fm.actualLineToDisplay(4)).toBe(4);

    // Start of fold (summary line)
    expect(fm.actualLineToDisplay(5)).toBe(5);

    // Inside collapsed region: returns summary line position
    expect(fm.actualLineToDisplay(10)).toBe(5);

    // After fold: actual 15 -> display 6
    expect(fm.actualLineToDisplay(15)).toBe(6);
    expect(fm.actualLineToDisplay(16)).toBe(7);
  });

  test("isSummaryLine returns true only for summary display lines", () => {
    const fm = new FoldManager();
    fm.registerOsc133Region(5, 15, "test", 0);
    fm.toggleFold(5);

    expect(fm.isSummaryLine(5)).toBe(true);
    expect(fm.isSummaryLine(4)).toBe(false);
    expect(fm.isSummaryLine(6)).toBe(false);
  });

  test("getSummaryRegion returns the region for a summary display line", () => {
    const fm = new FoldManager();
    fm.registerOsc133Region(5, 15, "test cmd", 0);
    fm.toggleFold(5);

    const region = fm.getSummaryRegion(5);
    expect(region).not.toBeNull();
    expect(region!.commandText).toBe("test cmd");

    expect(fm.getSummaryRegion(4)).toBeNull();
    expect(fm.getSummaryRegion(6)).toBeNull();
  });

  test("getTotalDisplayLines calculates correct count", () => {
    const fm = new FoldManager();
    fm.registerOsc133Region(5, 15, "test", 0);

    // No folds: total = totalActualLines
    expect(fm.getTotalDisplayLines(100)).toBe(100);

    // With fold: 100 - 9 hidden = 91
    fm.toggleFold(5);
    expect(fm.getTotalDisplayLines(100)).toBe(91);
  });

  // --- Pruning ---

  test("T1-11: prune removes old regions", () => {
    const fm = new FoldManager();
    fm.registerOsc133Region(5, 15, "old", 0);
    fm.registerOsc133Region(25, 35, "new", 0);

    fm.pruneBeforeLine(20);

    // old region at 5-15 is gone (was before boundary 20)
    // new region at 25-35 becomes 5-15 after adjustment
    const region = fm.getRegionAtLine(5);
    expect(region).not.toBeNull();
    expect(region!.commandText).toBe("new");
    expect(region!.startLine).toBe(5);
    expect(region!.endLine).toBe(15);
    // No region at line 0 (old region was removed)
    expect(fm.getRegionAtLine(0)).toBeNull();
  });

  test("T1-12: prune adjusts remaining indices", () => {
    const fm = new FoldManager();
    fm.registerOsc133Region(20, 30, "test", 0);

    fm.pruneBeforeLine(10);

    const region = fm.getRegionAtLine(10); // 20-10=10
    expect(region).not.toBeNull();
    expect(region!.startLine).toBe(10);
    expect(region!.endLine).toBe(20);
  });

  test("T1-13: prune removes region spanning boundary", () => {
    const fm = new FoldManager();
    fm.registerOsc133Region(5, 15, "spanning", 0);
    fm.registerOsc133Region(20, 30, "after", 0);

    fm.pruneBeforeLine(10);
    // Region 5-15 spans boundary (starts before 10), should be removed
    // Region 20-30 becomes 10-20
    const collapsed = fm.getCollapsedRegions();
    expect(collapsed.length).toBe(0);
    // Check only the "after" region remains
    expect(fm.getRegionAtLine(10)).not.toBeNull();
    expect(fm.getRegionAtLine(10)!.commandText).toBe("after");
  });

  test("prune preserves collapsed state", () => {
    const fm = new FoldManager();
    fm.registerOsc133Region(20, 30, "test", 0);
    fm.toggleFold(20);

    fm.pruneBeforeLine(10);

    const region = fm.getRegionAtLine(10);
    expect(region).not.toBeNull();
    expect(region!.collapsed).toBe(true);
  });

  // --- unfoldAll ---

  test("T1-14: unfoldAll expands all regions", () => {
    const fm = new FoldManager();
    fm.registerOsc133Region(5, 15, "first", 0);
    fm.registerOsc133Region(20, 30, "second", 0);
    fm.toggleFold(5);
    fm.toggleFold(20);

    expect(fm.getCollapsedRegions().length).toBe(2);

    fm.unfoldAll();

    expect(fm.getCollapsedRegions().length).toBe(0);
    expect(fm.getRegionAtLine(5)!.collapsed).toBe(false);
    expect(fm.getRegionAtLine(20)!.collapsed).toBe(false);
  });

  // --- Disabled state ---

  test("T1-15: disabled state prevents toggle", () => {
    const fm = new FoldManager();
    fm.registerOsc133Region(5, 15, "test", 0);
    fm.setEnabled(false);

    expect(fm.toggleFold(5)).toBe(false);
    expect(fm.getRegionAtLine(5)!.collapsed).toBe(false);
  });

  test("setEnabled(false) unfolds all regions", () => {
    const fm = new FoldManager();
    fm.registerOsc133Region(5, 15, "test", 0);
    fm.toggleFold(5);
    expect(fm.getCollapsedRegions().length).toBe(1);

    fm.setEnabled(false);
    expect(fm.getCollapsedRegions().length).toBe(0);
  });

  test("setEnabled(true) after disabled allows toggle", () => {
    const fm = new FoldManager();
    fm.registerOsc133Region(5, 15, "test", 0);
    fm.setEnabled(false);
    fm.setEnabled(true);

    expect(fm.toggleFold(5)).toBe(true);
    expect(fm.getRegionAtLine(5)!.collapsed).toBe(true);
  });

  test("isEnabled reflects current state", () => {
    const fm = new FoldManager();
    expect(fm.isEnabled()).toBe(true);

    fm.setEnabled(false);
    expect(fm.isEnabled()).toBe(false);

    fm.setEnabled(true);
    expect(fm.isEnabled()).toBe(true);
  });

  // --- Edge cases ---

  test("multiple adjacent regions are independent", () => {
    const fm = new FoldManager();
    fm.registerOsc133Region(5, 10, "first", 0);
    fm.registerOsc133Region(10, 15, "second", 0);

    fm.toggleFold(5);
    expect(fm.getRegionAtLine(5)!.collapsed).toBe(true);
    expect(fm.getRegionAtLine(10)!.collapsed).toBe(false);
  });

  test("getRegionAtLine on empty manager returns null", () => {
    const fm = new FoldManager();
    expect(fm.getRegionAtLine(0)).toBeNull();
    expect(fm.getRegionAtLine(100)).toBeNull();
  });

  test("pruneBeforeLine on empty manager does not throw", () => {
    const fm = new FoldManager();
    expect(() => fm.pruneBeforeLine(10)).not.toThrow();
  });

  test("unfoldAll on empty manager does not throw", () => {
    const fm = new FoldManager();
    expect(() => fm.unfoldAll()).not.toThrow();
  });

  test("long command text is preserved in region", () => {
    const fm = new FoldManager();
    const longCmd = "a".repeat(200);
    fm.registerOsc133Region(5, 15, longCmd, 0);
    expect(fm.getRegionAtLine(5)!.commandText).toBe(longCmd);
  });

  test("TS-20: long label is preserved in region", () => {
    const fm = new FoldManager();
    const longLabel = "b".repeat(200);
    fm.registerCustomRegion(5, 15, longLabel);
    expect(fm.getRegionAtLine(5)!.label).toBe(longLabel);
  });

  test("expandRegionContaining expands if line is in collapsed region", () => {
    const fm = new FoldManager();
    fm.registerOsc133Region(5, 15, "test", 0);
    fm.toggleFold(5);
    expect(fm.getRegionAtLine(5)!.collapsed).toBe(true);

    const expanded = fm.expandRegionContaining(10);
    expect(expanded).toBe(true);
    expect(fm.getRegionAtLine(5)!.collapsed).toBe(false);
  });

  test("expandRegionContaining returns false if not in collapsed region", () => {
    const fm = new FoldManager();
    fm.registerOsc133Region(5, 15, "test", 0);
    // Not collapsed
    expect(fm.expandRegionContaining(10)).toBe(false);
    // Outside any region
    expect(fm.expandRegionContaining(20)).toBe(false);
  });
});
