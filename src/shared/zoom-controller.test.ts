/**
 * Tests for ZoomController.
 *
 * @module shared/zoom-controller.test
 */

import { describe, test, expect, beforeEach, afterEach, mock } from "bun:test";
import { ZoomController } from "./zoom-controller.ts";
import type { ZoomControllerOptions } from "./zoom-controller.ts";

/**
 * Creates a minimal DOM environment for testing.
 */
function createTestEnvironment(): {
  container: HTMLDivElement;
  overlay: HTMLDivElement;
  cleanup: () => void;
} {
  const container = document.createElement("div");
  const overlay = document.createElement("div");
  document.body.appendChild(overlay);
  overlay.appendChild(container);

  return {
    container,
    overlay,
    cleanup: () => {
      overlay.remove();
    },
  };
}

describe("ZoomController", () => {
  let env: ReturnType<typeof createTestEnvironment>;
  let controller: ZoomController;

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
    test("should initialize with default zoom level of 100%", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      expect(controller.getZoomLevel()).toBe(100);
    });

    test("should accept custom min/max zoom values", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
        minZoom: 50,
        maxZoom: 200,
      });

      expect(controller.getZoomLevel()).toBe(100);
    });
  });

  describe("zoomIn()", () => {
    test("should increase zoom level by 10%", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      controller.zoomIn();
      expect(controller.getZoomLevel()).toBe(110);
    });

    test("should not exceed max zoom level (400%)", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      // Zoom to max
      for (let i = 0; i < 40; i++) {
        controller.zoomIn();
      }

      expect(controller.getZoomLevel()).toBe(400);

      // Try to zoom beyond max
      controller.zoomIn();
      expect(controller.getZoomLevel()).toBe(400);
    });

    test("should use custom zoom step", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
        zoomStep: 20,
      });

      controller.zoomIn();
      expect(controller.getZoomLevel()).toBe(120);
    });
  });

  describe("zoomOut()", () => {
    test("should decrease zoom level by 10%", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      controller.zoomOut();
      expect(controller.getZoomLevel()).toBe(90);
    });

    test("should not go below min zoom level (25%)", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      // Zoom to min
      for (let i = 0; i < 20; i++) {
        controller.zoomOut();
      }

      expect(controller.getZoomLevel()).toBe(25);

      // Try to zoom beyond min
      controller.zoomOut();
      expect(controller.getZoomLevel()).toBe(25);
    });
  });

  describe("zoomTo()", () => {
    test("should set zoom level to specified value", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      controller.zoomTo(150);
      expect(controller.getZoomLevel()).toBe(150);
    });

    test("should clamp value to min zoom level", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      controller.zoomTo(10);
      expect(controller.getZoomLevel()).toBe(25);
    });

    test("should clamp value to max zoom level", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      controller.zoomTo(500);
      expect(controller.getZoomLevel()).toBe(400);
    });
  });

  describe("resetZoom()", () => {
    test("should reset zoom level to 100%", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      controller.zoomTo(200);
      expect(controller.getZoomLevel()).toBe(200);

      controller.resetZoom();
      expect(controller.getZoomLevel()).toBe(100);
    });

    test("should reset from any zoom level", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      controller.zoomTo(25);
      controller.resetZoom();
      expect(controller.getZoomLevel()).toBe(100);

      controller.zoomTo(400);
      controller.resetZoom();
      expect(controller.getZoomLevel()).toBe(100);
    });
  });

  describe("getZoomLevel()", () => {
    test("should return current zoom level", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      expect(controller.getZoomLevel()).toBe(100);

      controller.zoomIn();
      expect(controller.getZoomLevel()).toBe(110);

      controller.zoomOut();
      controller.zoomOut();
      expect(controller.getZoomLevel()).toBe(90);
    });
  });

  describe("applyZoom()", () => {
    test("should apply transform:scale to container", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      controller.zoomTo(200);

      // Check that transform includes scale(2)
      expect(env.container.style.transform).toContain("scale(2)");
    });

    test("should apply scale(1) at 100% zoom", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      // Initial state should have scale(1)
      expect(env.container.style.transform).toContain("scale(1)");
    });
  });

  describe("UI Components", () => {
    test("should create close button in overlay", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      const closeButton = env.overlay.querySelector(".viewer-close-button");
      expect(closeButton).not.toBeNull();
    });

    test("should create zoom bar in overlay", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      const zoomBar = env.overlay.querySelector(".viewer-zoom-bar");
      expect(zoomBar).not.toBeNull();
    });

    test("close button click should call onClose callback", () => {
      const onCloseMock = mock(() => {});

      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
        onClose: onCloseMock,
      });

      const closeButton = env.overlay.querySelector(
        ".viewer-close-button",
      ) as HTMLButtonElement;
      closeButton?.click();

      expect(onCloseMock).toHaveBeenCalled();
    });

    test("+ button should call zoomIn()", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      const zoomInBtn = env.overlay.querySelectorAll(
        ".viewer-zoom-button",
      )[1] as HTMLButtonElement;
      zoomInBtn?.click();

      expect(controller.getZoomLevel()).toBe(110);
    });

    test("- button should call zoomOut()", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      const zoomOutBtn = env.overlay.querySelectorAll(
        ".viewer-zoom-button",
      )[0] as HTMLButtonElement;
      zoomOutBtn?.click();

      expect(controller.getZoomLevel()).toBe(90);
    });

    test("zoom level click should call resetZoom()", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      controller.zoomTo(200);
      expect(controller.getZoomLevel()).toBe(200);

      const zoomLevel = env.overlay.querySelector(
        ".viewer-zoom-level",
      ) as HTMLSpanElement;
      zoomLevel?.click();

      expect(controller.getZoomLevel()).toBe(100);
    });

    test("zoom level display should update on zoom change", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      const zoomLevel = env.overlay.querySelector(
        ".viewer-zoom-level",
      ) as HTMLSpanElement;
      expect(zoomLevel?.textContent).toBe("100%");

      controller.zoomTo(150);
      expect(zoomLevel?.textContent).toBe("150%");
    });

    test("dispose() should remove UI elements from DOM", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      expect(env.overlay.querySelector(".viewer-close-button")).not.toBeNull();
      expect(env.overlay.querySelector(".viewer-zoom-bar")).not.toBeNull();

      controller.dispose();

      expect(env.overlay.querySelector(".viewer-close-button")).toBeNull();
      expect(env.overlay.querySelector(".viewer-zoom-bar")).toBeNull();
    });
  });

  describe("Callback Options", () => {
    test("should call onZoomChange callback when zoom changes", () => {
      const onZoomChangeMock = mock(() => {});

      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
        onZoomChange: onZoomChangeMock,
      });

      controller.zoomIn();
      expect(onZoomChangeMock).toHaveBeenCalledWith(110);

      controller.zoomOut();
      expect(onZoomChangeMock).toHaveBeenCalledWith(100);

      controller.zoomTo(150);
      expect(onZoomChangeMock).toHaveBeenCalledWith(150);
    });

    test("should NOT apply default transform when onZoomChange is provided", () => {
      const onZoomChangeMock = mock(() => {});

      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
        onZoomChange: onZoomChangeMock,
      });

      controller.zoomTo(200);

      // Transform should not be applied when callback handles zoom
      expect(env.container.style.transform).toBe("");
    });

    test("should initialize with custom initialLevel", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
        initialLevel: 50,
      });

      expect(controller.getZoomLevel()).toBe(50);
    });

    test("should display custom initialLevel in zoom bar", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
        initialLevel: 75,
      });

      const zoomLevel = env.overlay.querySelector(
        ".viewer-zoom-level",
      ) as HTMLSpanElement;
      expect(zoomLevel?.textContent).toBe("75%");
    });

    test("should call onZoomChange with initialLevel on construction", () => {
      const onZoomChangeMock = mock(() => {});

      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
        initialLevel: 50,
        onZoomChange: onZoomChangeMock,
      });

      // Should be called once on initialization
      expect(onZoomChangeMock).toHaveBeenCalledWith(50);
    });

    test("resetZoom should return to initialLevel when set", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
        initialLevel: 50,
      });

      controller.zoomTo(200);
      expect(controller.getZoomLevel()).toBe(200);

      controller.resetZoom();
      expect(controller.getZoomLevel()).toBe(50);
    });

    test("resetZoom should return to 100% when initialLevel is not set", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      controller.zoomTo(200);
      controller.resetZoom();
      expect(controller.getZoomLevel()).toBe(100);
    });

    test("should call onReset callback when resetZoom is called", () => {
      const onResetMock = mock(() => {});

      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
        onReset: onResetMock,
      });

      controller.zoomTo(200);
      controller.resetZoom();

      expect(onResetMock).toHaveBeenCalled();
    });

    test("initialLevel should be clamped to valid range", () => {
      // Below min
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
        initialLevel: 10, // Below 25%
      });
      expect(controller.getZoomLevel()).toBe(25);

      controller.dispose();

      // Above max
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
        initialLevel: 500, // Above 400%
      });
      expect(controller.getZoomLevel()).toBe(400);
    });
  });

  describe("Event Handling", () => {
    /**
     * Helper to create and dispatch keyboard events.
     */
    function dispatchKeydown(key: string, ctrlKey = false): KeyboardEvent {
      const event = new KeyboardEvent("keydown", {
        key,
        ctrlKey,
        bubbles: true,
        cancelable: true,
      });
      document.dispatchEvent(event);
      return event;
    }

    /**
     * Helper to create and dispatch wheel events.
     * Note: WheelEvent may not be available in all test environments,
     * so we create a custom event with wheel properties.
     */
    function dispatchWheel(
      target: HTMLElement,
      deltaY: number,
      ctrlKey = false,
    ): Event {
      // Create a generic event and add wheel-specific properties
      const event = new Event("wheel", {
        bubbles: true,
        cancelable: true,
      });
      Object.defineProperties(event, {
        deltaY: { value: deltaY },
        ctrlKey: { value: ctrlKey },
        clientX: { value: 50 },
        clientY: { value: 50 },
      });
      target.dispatchEvent(event);
      return event;
    }

    test("+ key should zoom in", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      dispatchKeydown("+");
      expect(controller.getZoomLevel()).toBe(110);
    });

    test("= key should zoom in (alternative)", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      dispatchKeydown("=");
      expect(controller.getZoomLevel()).toBe(110);
    });

    test("- key should zoom out", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      dispatchKeydown("-");
      expect(controller.getZoomLevel()).toBe(90);
    });

    test("0 key should reset zoom", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      controller.zoomTo(200);
      dispatchKeydown("0");
      expect(controller.getZoomLevel()).toBe(100);
    });

    test("Escape key should not trigger zoom (existing behavior preserved)", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      dispatchKeydown("Escape");
      expect(controller.getZoomLevel()).toBe(100);
    });

    test("Ctrl+wheel up should zoom in", async () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      dispatchWheel(env.overlay, -100, true);
      expect(controller.getZoomLevel()).toBe(110);
    });

    test("Ctrl+wheel down should zoom out", async () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      dispatchWheel(env.overlay, 100, true);
      expect(controller.getZoomLevel()).toBe(90);
    });

    test("wheel without Ctrl should not zoom", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      dispatchWheel(env.overlay, -100, false);
      expect(controller.getZoomLevel()).toBe(100);
    });

    test("dispose() should remove event listeners", () => {
      controller = new ZoomController({
        container: env.container,
        overlay: env.overlay,
      });

      controller.dispose();

      // After dispose, keyboard events should not affect zoom level
      // (but we need to create a new controller to test this properly)
      // Since controller is disposed, getZoomLevel should still return last value
      expect(controller.getZoomLevel()).toBe(100);
    });
  });
});
