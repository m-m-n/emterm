/**
 * Tests for PanController.
 *
 * @module image-viewer/pan-controller.test
 */

import { describe, test, expect, beforeEach, afterEach, mock } from "bun:test";
import { PanController } from "./pan-controller.ts";

/**
 * Creates a minimal DOM environment for testing.
 */
function createTestEnvironment(): {
  canvas: HTMLCanvasElement;
  overlay: HTMLDivElement;
  cleanup: () => void;
} {
  const overlay = document.createElement("div");
  overlay.style.width = "800px";
  overlay.style.height = "600px";
  Object.defineProperty(overlay, "clientWidth", { value: 800 });
  Object.defineProperty(overlay, "clientHeight", { value: 600 });
  document.body.appendChild(overlay);

  const canvas = document.createElement("canvas");
  canvas.width = 1000;
  canvas.height = 800;
  overlay.appendChild(canvas);

  return {
    canvas,
    overlay,
    cleanup: () => {
      overlay.remove();
    },
  };
}

describe("PanController", () => {
  let env: ReturnType<typeof createTestEnvironment>;
  let controller: PanController;

  beforeEach(() => {
    env = createTestEnvironment();
  });

  afterEach(() => {
    if (controller) {
      controller.dispose();
    }
    env.cleanup();
  });

  describe("Initialization", () => {
    test("should create a PanController instance", () => {
      controller = new PanController({
        canvas: env.canvas,
        overlay: env.overlay,
      });

      expect(controller).toBeDefined();
    });

    test("should initialize with zero offset", () => {
      controller = new PanController({
        canvas: env.canvas,
        overlay: env.overlay,
      });

      expect(controller.getOffset()).toEqual({ x: 0, y: 0 });
    });
  });

  describe("canPan()", () => {
    test("should return true when canvas is larger than viewport", () => {
      controller = new PanController({
        canvas: env.canvas,
        overlay: env.overlay,
      });

      // Canvas 1000x800, viewport 800x600
      expect(controller.canPan()).toBe(true);
    });

    test("should return false when canvas fits in viewport", () => {
      // Create smaller canvas
      env.canvas.width = 400;
      env.canvas.height = 300;

      controller = new PanController({
        canvas: env.canvas,
        overlay: env.overlay,
      });

      expect(controller.canPan()).toBe(false);
    });

    test("should return true when only width exceeds viewport", () => {
      env.canvas.width = 1000;
      env.canvas.height = 300;

      controller = new PanController({
        canvas: env.canvas,
        overlay: env.overlay,
      });

      expect(controller.canPan()).toBe(true);
    });

    test("should return true when only height exceeds viewport", () => {
      env.canvas.width = 400;
      env.canvas.height = 800;

      controller = new PanController({
        canvas: env.canvas,
        overlay: env.overlay,
      });

      expect(controller.canPan()).toBe(true);
    });
  });

  describe("updateCanvasSize()", () => {
    test("should update pan availability when canvas size changes", () => {
      controller = new PanController({
        canvas: env.canvas,
        overlay: env.overlay,
      });

      expect(controller.canPan()).toBe(true);

      // Make canvas smaller
      controller.updateCanvasSize(400, 300);
      expect(controller.canPan()).toBe(false);
    });

    test("should reset offset when canvas becomes smaller than viewport", () => {
      controller = new PanController({
        canvas: env.canvas,
        overlay: env.overlay,
      });

      // Manually set offset
      controller.setOffset(50, 50);
      expect(controller.getOffset()).toEqual({ x: 50, y: 50 });

      // Make canvas smaller than viewport
      controller.updateCanvasSize(400, 300);
      expect(controller.getOffset()).toEqual({ x: 0, y: 0 });
    });

    test("should ignore invalid dimensions (zero)", () => {
      controller = new PanController({
        canvas: env.canvas,
        overlay: env.overlay,
      });

      // Get original bounds
      const originalBounds = controller.getBounds();

      // Try to update with zero dimensions
      controller.updateCanvasSize(0, 0);

      // Bounds should remain unchanged
      expect(controller.getBounds()).toEqual(originalBounds);
    });

    test("should ignore invalid dimensions (negative)", () => {
      controller = new PanController({
        canvas: env.canvas,
        overlay: env.overlay,
      });

      // Get original bounds
      const originalBounds = controller.getBounds();

      // Try to update with negative dimensions
      controller.updateCanvasSize(-100, -100);

      // Bounds should remain unchanged
      expect(controller.getBounds()).toEqual(originalBounds);
    });
  });

  describe("calculateBounds()", () => {
    test("should return correct bounds for oversized canvas", () => {
      controller = new PanController({
        canvas: env.canvas,
        overlay: env.overlay,
      });

      const bounds = controller.getBounds();

      // Canvas 1000x800, viewport 800x600
      // Max pan: (1000-800)/2 = 100, (800-600)/2 = 100
      expect(bounds.maxX).toBe(100);
      expect(bounds.minX).toBe(-100);
      expect(bounds.maxY).toBe(100);
      expect(bounds.minY).toBe(-100);
    });

    test("should return zero bounds when canvas fits", () => {
      env.canvas.width = 400;
      env.canvas.height = 300;

      controller = new PanController({
        canvas: env.canvas,
        overlay: env.overlay,
      });

      const bounds = controller.getBounds();
      // Use toEqual for object comparison to avoid -0 vs 0 issues
      expect(bounds).toEqual({ maxX: 0, minX: 0, maxY: 0, minY: 0 });
    });
  });

  describe("setOffset()", () => {
    test("should set offset within bounds", () => {
      controller = new PanController({
        canvas: env.canvas,
        overlay: env.overlay,
      });

      controller.setOffset(50, 50);
      expect(controller.getOffset()).toEqual({ x: 50, y: 50 });
    });

    test("should clamp offset to bounds", () => {
      controller = new PanController({
        canvas: env.canvas,
        overlay: env.overlay,
      });

      // Try to set beyond bounds (max is 100)
      controller.setOffset(200, 200);
      expect(controller.getOffset()).toEqual({ x: 100, y: 100 });

      controller.setOffset(-200, -200);
      expect(controller.getOffset()).toEqual({ x: -100, y: -100 });
    });

    test("should call onOffsetChange callback", () => {
      const onOffsetChangeMock = mock(() => {});

      controller = new PanController({
        canvas: env.canvas,
        overlay: env.overlay,
        onOffsetChange: onOffsetChangeMock,
      });

      controller.setOffset(50, 50);
      expect(onOffsetChangeMock).toHaveBeenCalledWith(50, 50);
    });
  });

  describe("reset()", () => {
    test("should reset offset to zero", () => {
      controller = new PanController({
        canvas: env.canvas,
        overlay: env.overlay,
      });

      controller.setOffset(50, 50);
      expect(controller.getOffset()).toEqual({ x: 50, y: 50 });

      controller.reset();
      expect(controller.getOffset()).toEqual({ x: 0, y: 0 });
    });

    test("should call onOffsetChange with zero", () => {
      const onOffsetChangeMock = mock(() => {});

      controller = new PanController({
        canvas: env.canvas,
        overlay: env.overlay,
        onOffsetChange: onOffsetChangeMock,
      });

      controller.setOffset(50, 50);
      onOffsetChangeMock.mockClear();

      controller.reset();
      expect(onOffsetChangeMock).toHaveBeenCalledWith(0, 0);
    });
  });

  describe("isDragging()", () => {
    test("should return false initially", () => {
      controller = new PanController({
        canvas: env.canvas,
        overlay: env.overlay,
      });

      expect(controller.isDragging()).toBe(false);
    });
  });

  describe("dispose()", () => {
    test("should clean up without errors", () => {
      controller = new PanController({
        canvas: env.canvas,
        overlay: env.overlay,
      });

      // Should not throw
      expect(() => controller.dispose()).not.toThrow();
    });
  });
});
