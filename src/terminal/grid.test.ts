/**
 * Tests for Grid and Line structures.
 */
import { describe, expect, test } from "bun:test";
import {
  type Cell,
  Line,
  createEmptyCell,
  createCell,
  cloneCell,
} from "./grid.ts";

describe("createEmptyCell", () => {
  test("creates cell with space character", () => {
    const cell = createEmptyCell();
    expect(cell.char).toBe(" ");
  });

  test("creates cell with width 1", () => {
    const cell = createEmptyCell();
    expect(cell.width).toBe(1);
  });

  test("creates cell with default attributes", () => {
    const cell = createEmptyCell();
    expect(cell.attrs.bold).toBe(false);
    expect(cell.attrs.fg).toBeNull();
  });

  test("creates cell marked as dirty", () => {
    const cell = createEmptyCell();
    expect(cell.dirty).toBe(true);
  });
});

describe("createCell", () => {
  test("creates cell with specified character", () => {
    const cell = createCell("A");
    expect(cell.char).toBe("A");
    expect(cell.width).toBe(1);
  });

  test("creates cell with wide character", () => {
    const cell = createCell("\u4e00"); // CJK character
    expect(cell.char).toBe("\u4e00");
    expect(cell.width).toBe(2);
  });

  test("creates cell with custom attributes", () => {
    const attrs = {
      bold: true,
      dim: false,
      italic: false,
      underline: false,
      blink: false,
      reverse: false,
      hidden: false,
      strikethrough: false,
      fg: { type: "indexed" as const, index: 1 },
      bg: null,
    };
    const cell = createCell("X", attrs);
    expect(cell.attrs.bold).toBe(true);
    expect(cell.attrs.fg).toEqual({ type: "indexed", index: 1 });
  });
});

describe("cloneCell", () => {
  test("creates independent copy", () => {
    const original = createCell("A");
    original.dirty = false;

    const cloned = cloneCell(original);
    expect(cloned).not.toBe(original);
    expect(cloned.char).toBe("A");
    expect(cloned.dirty).toBe(false);
  });

  test("deep clones attributes", () => {
    const original = createCell("A");
    original.attrs.bold = true;
    original.attrs.fg = { type: "indexed", index: 5 };

    const cloned = cloneCell(original);
    expect(cloned.attrs).not.toBe(original.attrs);
    expect(cloned.attrs.bold).toBe(true);
    expect(cloned.attrs.fg).toEqual({ type: "indexed", index: 5 });

    // Modify original should not affect clone
    original.attrs.bold = false;
    expect(cloned.attrs.bold).toBe(true);
  });
});

describe("Line", () => {
  describe("constructor", () => {
    test("creates line with specified width", () => {
      const line = new Line(80);
      expect(line.length).toBe(80);
    });

    test("initializes all cells as empty", () => {
      const line = new Line(10);
      for (let i = 0; i < 10; i++) {
        expect(line.getCell(i).char).toBe(" ");
      }
    });

    test("marks line as dirty", () => {
      const line = new Line(10);
      expect(line.dirty).toBe(true);
    });
  });

  describe("getCell", () => {
    test("returns cell at specified index", () => {
      const line = new Line(10);
      line.setCell(5, createCell("X"));
      expect(line.getCell(5).char).toBe("X");
    });

    test("throws for out of bounds index", () => {
      const line = new Line(10);
      expect(() => line.getCell(10)).toThrow();
      expect(() => line.getCell(-1)).toThrow();
    });
  });

  describe("setCell", () => {
    test("sets cell at specified index", () => {
      const line = new Line(10);
      const cell = createCell("A");
      line.setCell(3, cell);
      expect(line.getCell(3).char).toBe("A");
    });

    test("marks line as dirty", () => {
      const line = new Line(10);
      line.dirty = false;
      line.setCell(0, createCell("X"));
      expect(line.dirty).toBe(true);
    });

    test("throws for out of bounds index", () => {
      const line = new Line(10);
      expect(() => line.setCell(10, createCell("X"))).toThrow();
    });
  });

  describe("clear", () => {
    test("resets all cells to empty", () => {
      const line = new Line(10);
      line.setCell(0, createCell("A"));
      line.setCell(5, createCell("B"));
      line.clear();

      for (let i = 0; i < 10; i++) {
        expect(line.getCell(i).char).toBe(" ");
      }
    });

    test("marks line as dirty", () => {
      const line = new Line(10);
      line.dirty = false;
      line.clear();
      expect(line.dirty).toBe(true);
    });
  });

  describe("clearRange", () => {
    test("clears cells in specified range", () => {
      const line = new Line(10);
      for (let i = 0; i < 10; i++) {
        line.setCell(i, createCell(String.fromCharCode(65 + i)));
      }

      line.clearRange(3, 7);

      expect(line.getCell(2).char).toBe("C");
      expect(line.getCell(3).char).toBe(" ");
      expect(line.getCell(6).char).toBe(" ");
      expect(line.getCell(7).char).toBe("H");
    });
  });

  describe("resize", () => {
    test("expands line with empty cells", () => {
      const line = new Line(5);
      line.setCell(0, createCell("A"));
      line.resize(10);

      expect(line.length).toBe(10);
      expect(line.getCell(0).char).toBe("A");
      expect(line.getCell(9).char).toBe(" ");
    });

    test("shrinks line", () => {
      const line = new Line(10);
      line.setCell(9, createCell("Z"));
      line.resize(5);

      expect(line.length).toBe(5);
      expect(() => line.getCell(9)).toThrow();
    });
  });

  describe("getText", () => {
    test("returns text content", () => {
      const line = new Line(10);
      line.setCell(0, createCell("H"));
      line.setCell(1, createCell("e"));
      line.setCell(2, createCell("l"));
      line.setCell(3, createCell("l"));
      line.setCell(4, createCell("o"));

      expect(line.getText()).toBe("Hello     ");
    });

    test("handles wide characters", () => {
      const line = new Line(10);
      const wideCell = createCell("\u4e00");
      line.setCell(0, wideCell);
      // Wide char takes 2 cells, second cell should be placeholder
      line.setCell(1, createEmptyCell());
      line.getCell(1).char = "";
      line.getCell(1).width = 0;

      const text = line.getText();
      expect(text[0]).toBe("\u4e00");
    });
  });

  describe("clone", () => {
    test("creates independent copy", () => {
      const original = new Line(10);
      original.setCell(0, createCell("A"));
      original.dirty = false;

      const cloned = original.clone();
      expect(cloned).not.toBe(original);
      expect(cloned.getCell(0).char).toBe("A");
      expect(cloned.dirty).toBe(false);

      // Modify original should not affect clone
      original.setCell(0, createCell("B"));
      expect(cloned.getCell(0).char).toBe("A");
    });
  });
});
