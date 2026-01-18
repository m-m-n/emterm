/**
 * DisplayModeController unit tests.
 *
 * Tests mode state management and scale calculation.
 *
 * @module image-viewer/display-mode.test
 */

import { describe, test, expect } from "bun:test";
import {
  DisplayModeController,
  calculateFitScale,
  type DisplayMode,
  type DisplayModeState,
} from "./display-mode.ts";

describe("calculateFitScale", () => {
  test("should calculate fit scale for large image", () => {
    // 4000x3000 image in 800x600 viewport (95% padding)
    // Effective viewport: 760x570
    // Scale by width: 760/4000 = 0.19
    // Scale by height: 570/3000 = 0.19
    // Fit scale: 0.19 (19%), but minimum is 0.25 (25%)
    const scale = calculateFitScale(4000, 3000, 800, 600);
    expect(scale).toBe(0.25); // Clamped to minimum
  });

  test("should not upscale small images beyond 100%", () => {
    // 400x300 image in 800x600 viewport
    // Would fit at 190%, but should cap at 100%
    const scale = calculateFitScale(400, 300, 800, 600);
    expect(scale).toBe(1.0); // 100%, no upscaling
  });

  test("should handle portrait images", () => {
    // 1000x2000 image in 800x600 viewport
    // Effective viewport: 760x570
    // Scale by height: 570/2000 = 0.285 (28.5%)
    const scale = calculateFitScale(1000, 2000, 800, 600);
    expect(scale).toBeCloseTo(0.285, 2);
  });

  test("should handle landscape images", () => {
    // 2000x1000 image in 800x600 viewport
    // Effective viewport: 760x570
    // Scale by width: 760/2000 = 0.38 (38%)
    const scale = calculateFitScale(2000, 1000, 800, 600);
    expect(scale).toBeCloseTo(0.38, 2);
  });

  test("should return 1.0 for zero image width", () => {
    const scale = calculateFitScale(0, 600, 800, 600);
    expect(scale).toBe(1.0);
  });

  test("should return 1.0 for zero image height", () => {
    const scale = calculateFitScale(800, 0, 800, 600);
    expect(scale).toBe(1.0);
  });

  test("should return 1.0 for negative image dimensions", () => {
    const scale = calculateFitScale(-100, 600, 800, 600);
    expect(scale).toBe(1.0);
  });

  test("should return minimum scale for zero viewport width", () => {
    const scale = calculateFitScale(800, 600, 0, 600);
    expect(scale).toBe(0.25); // Minimum scale
  });

  test("should return minimum scale for zero viewport height", () => {
    const scale = calculateFitScale(800, 600, 800, 0);
    expect(scale).toBe(0.25); // Minimum scale
  });
});

describe("DisplayModeController - State Management", () => {
  test("should initialize in pixel mode by default", () => {
    const controller = new DisplayModeController({
      imageWidth: 1000,
      imageHeight: 800,
      viewportWidth: 800,
      viewportHeight: 600,
    });

    const state = controller.getState();
    expect(state.mode).toBe("pixel");
    expect(state.scale).toBe(1.0); // 100%
  });

  test("should initialize in fit mode when specified", () => {
    const controller = new DisplayModeController({
      imageWidth: 2000,
      imageHeight: 1500,
      viewportWidth: 800,
      viewportHeight: 600,
      initialMode: "fit",
    });

    const state = controller.getState();
    expect(state.mode).toBe("fit");
    // Fit scale for 2000x1500 in 800x600 viewport
    expect(state.scale).toBeLessThan(1.0);
  });

  test("should toggle between pixel and fit modes", () => {
    const controller = new DisplayModeController({
      imageWidth: 2000,
      imageHeight: 1500,
      viewportWidth: 800,
      viewportHeight: 600,
    });

    // Start in pixel mode
    expect(controller.getState().mode).toBe("pixel");

    // Toggle to fit
    controller.toggle();
    expect(controller.getState().mode).toBe("fit");

    // Toggle back to pixel
    controller.toggle();
    expect(controller.getState().mode).toBe("pixel");
  });

  test("should set specific mode", () => {
    const controller = new DisplayModeController({
      imageWidth: 2000,
      imageHeight: 1500,
      viewportWidth: 800,
      viewportHeight: 600,
    });

    controller.setMode("fit");
    expect(controller.getState().mode).toBe("fit");

    controller.setMode("pixel");
    expect(controller.getState().mode).toBe("pixel");
  });

  test("should call onModeChange callback when mode changes", () => {
    let callbackState: DisplayModeState | null = null;

    const controller = new DisplayModeController({
      imageWidth: 2000,
      imageHeight: 1500,
      viewportWidth: 800,
      viewportHeight: 600,
      onModeChange: (state) => {
        callbackState = state;
      },
    });

    controller.toggle();

    expect(callbackState).not.toBeNull();
    expect(callbackState!.mode).toBe("fit");
  });

  test("should not call callback when setting same mode", () => {
    let callCount = 0;

    const controller = new DisplayModeController({
      imageWidth: 2000,
      imageHeight: 1500,
      viewportWidth: 800,
      viewportHeight: 600,
      onModeChange: () => {
        callCount++;
      },
    });

    // Already in pixel mode
    controller.setMode("pixel");
    expect(callCount).toBe(0);
  });
});

describe("DisplayModeController - Scale Calculation", () => {
  test("should return 1.0 scale in pixel mode", () => {
    const controller = new DisplayModeController({
      imageWidth: 2000,
      imageHeight: 1500,
      viewportWidth: 800,
      viewportHeight: 600,
    });

    expect(controller.getState().scale).toBe(1.0);
  });

  test("should return calculated fit scale in fit mode", () => {
    const controller = new DisplayModeController({
      imageWidth: 2000,
      imageHeight: 1500,
      viewportWidth: 800,
      viewportHeight: 600,
      initialMode: "fit",
    });

    const state = controller.getState();
    // 2000x1500 in 800x600 (95% = 760x570)
    // Width scale: 760/2000 = 0.38
    // Height scale: 570/1500 = 0.38
    expect(state.fitScale).toBeCloseTo(0.38, 2);
    expect(state.scale).toBe(state.fitScale);
  });

  test("should update fitScale when viewport changes", () => {
    const controller = new DisplayModeController({
      imageWidth: 2000,
      imageHeight: 1500,
      viewportWidth: 800,
      viewportHeight: 600,
      initialMode: "fit",
    });

    const initialFitScale = controller.getState().fitScale;

    // Resize viewport to larger
    controller.updateViewport(1600, 1200);

    const newFitScale = controller.getState().fitScale;
    expect(newFitScale).toBeGreaterThan(initialFitScale);
  });

  test("should update scale when in fit mode and viewport changes", () => {
    let lastState: DisplayModeState | null = null;

    const controller = new DisplayModeController({
      imageWidth: 2000,
      imageHeight: 1500,
      viewportWidth: 800,
      viewportHeight: 600,
      initialMode: "fit",
      onModeChange: (state) => {
        lastState = state;
      },
    });

    controller.updateViewport(1600, 1200);

    // Callback should be called with new scale
    expect(lastState).not.toBeNull();
    expect(lastState!.scale).toBe(lastState!.fitScale);
  });

  test("should not update scale when in pixel mode and viewport changes", () => {
    let callCount = 0;

    const controller = new DisplayModeController({
      imageWidth: 2000,
      imageHeight: 1500,
      viewportWidth: 800,
      viewportHeight: 600,
      initialMode: "pixel",
      onModeChange: () => {
        callCount++;
      },
    });

    // In pixel mode, viewport change should not trigger callback
    controller.updateViewport(1600, 1200);

    expect(callCount).toBe(0);
    expect(controller.getState().scale).toBe(1.0); // Still 100%
  });
});

describe("DisplayModeController - Edge Cases", () => {
  test("should handle image exactly same size as viewport", () => {
    const controller = new DisplayModeController({
      imageWidth: 800,
      imageHeight: 600,
      viewportWidth: 800,
      viewportHeight: 600,
    });

    // Pixel mode is always 100%
    expect(controller.getState().scale).toBe(1.0);

    controller.toggle();
    // Fit mode applies 95% viewport padding, so scale is 0.95
    expect(controller.getState().scale).toBe(0.95);
  });

  test("should handle very small image", () => {
    const controller = new DisplayModeController({
      imageWidth: 50,
      imageHeight: 50,
      viewportWidth: 800,
      viewportHeight: 600,
      initialMode: "fit",
    });

    // Small image should not be upscaled
    expect(controller.getState().scale).toBe(1.0);
  });

  test("should handle very large image", () => {
    const controller = new DisplayModeController({
      imageWidth: 10000,
      imageHeight: 10000,
      viewportWidth: 800,
      viewportHeight: 600,
      initialMode: "fit",
    });

    // Should clamp to minimum scale (0.25)
    expect(controller.getState().scale).toBeGreaterThanOrEqual(0.25);
  });

  test("should handle dispose", () => {
    const controller = new DisplayModeController({
      imageWidth: 1000,
      imageHeight: 800,
      viewportWidth: 800,
      viewportHeight: 600,
    });

    // Should not throw
    expect(() => controller.dispose()).not.toThrow();
  });
});

describe("DisplayModeController - UI Creation", () => {
  test("should create mode bar when overlay is provided", () => {
    const overlay = document.createElement("div");

    const controller = new DisplayModeController({
      imageWidth: 1000,
      imageHeight: 800,
      viewportWidth: 800,
      viewportHeight: 600,
      overlay,
    });

    // Mode bar should be created
    const modeBar = overlay.querySelector(".viewer-mode-bar");
    expect(modeBar).not.toBeNull();

    // Close button should be created
    const closeButton = overlay.querySelector(".viewer-close-button");
    expect(closeButton).not.toBeNull();

    controller.dispose();
  });

  test("should display current mode in mode bar", () => {
    const overlay = document.createElement("div");

    const controller = new DisplayModeController({
      imageWidth: 1000,
      imageHeight: 800,
      viewportWidth: 800,
      viewportHeight: 600,
      overlay,
    });

    // Should show "100%" for pixel mode
    const modeToggle = overlay.querySelector(".viewer-mode-toggle");
    expect(modeToggle?.textContent).toBe("100%");

    controller.dispose();
  });

  test("should update mode display when toggled", () => {
    const overlay = document.createElement("div");

    const controller = new DisplayModeController({
      imageWidth: 2000,
      imageHeight: 1500,
      viewportWidth: 800,
      viewportHeight: 600,
      overlay,
    });

    controller.toggle();

    // Should show "Fit" for fit mode
    const modeToggle = overlay.querySelector(".viewer-mode-toggle");
    expect(modeToggle?.textContent).toBe("Fit");

    controller.dispose();
  });

  test("should remove UI elements on dispose", () => {
    const overlay = document.createElement("div");

    const controller = new DisplayModeController({
      imageWidth: 1000,
      imageHeight: 800,
      viewportWidth: 800,
      viewportHeight: 600,
      overlay,
    });

    controller.dispose();

    // UI elements should be removed
    expect(overlay.querySelector(".viewer-mode-bar")).toBeNull();
    expect(overlay.querySelector(".viewer-close-button")).toBeNull();
  });

  test("should call onClose when close button is clicked", () => {
    const overlay = document.createElement("div");
    let closeCalled = false;

    const controller = new DisplayModeController({
      imageWidth: 1000,
      imageHeight: 800,
      viewportWidth: 800,
      viewportHeight: 600,
      overlay,
      onClose: () => {
        closeCalled = true;
      },
    });

    const closeButton = overlay.querySelector(
      ".viewer-close-button",
    ) as HTMLButtonElement;
    closeButton?.click();

    expect(closeCalled).toBe(true);

    controller.dispose();
  });

  test("should toggle mode when toggle button is clicked", () => {
    const overlay = document.createElement("div");
    let modeChangedTo: DisplayMode | null = null;

    const controller = new DisplayModeController({
      imageWidth: 2000,
      imageHeight: 1500,
      viewportWidth: 800,
      viewportHeight: 600,
      overlay,
      onModeChange: (state) => {
        modeChangedTo = state.mode;
      },
    });

    const modeToggle = overlay.querySelector(
      ".viewer-mode-toggle",
    ) as HTMLButtonElement;
    modeToggle?.click();

    expect(modeChangedTo).toBe("fit");

    controller.dispose();
  });
});

describe("DisplayModeController - Keyboard Handling", () => {
  test("should toggle mode on 'f' key", () => {
    const overlay = document.createElement("div");
    let modeChangedTo: DisplayMode | null = null;

    const controller = new DisplayModeController({
      imageWidth: 2000,
      imageHeight: 1500,
      viewportWidth: 800,
      viewportHeight: 600,
      overlay,
      onModeChange: (state) => {
        modeChangedTo = state.mode;
      },
    });

    // Simulate 'f' key press
    const event = new KeyboardEvent("keydown", { key: "f" });
    document.dispatchEvent(event);

    expect(modeChangedTo).toBe("fit");

    controller.dispose();
  });

  test("should switch to pixel mode on '1' key", () => {
    const overlay = document.createElement("div");
    let lastMode: DisplayMode | null = null;

    const controller = new DisplayModeController({
      imageWidth: 2000,
      imageHeight: 1500,
      viewportWidth: 800,
      viewportHeight: 600,
      overlay,
      initialMode: "fit",
      onModeChange: (state) => {
        lastMode = state.mode;
      },
    });

    // Simulate '1' key press
    const event = new KeyboardEvent("keydown", { key: "1" });
    document.dispatchEvent(event);

    expect(lastMode).toBe("pixel");

    controller.dispose();
  });

  test("should switch to fit mode on '0' key", () => {
    const overlay = document.createElement("div");
    let lastMode: DisplayMode | null = null;

    const controller = new DisplayModeController({
      imageWidth: 2000,
      imageHeight: 1500,
      viewportWidth: 800,
      viewportHeight: 600,
      overlay,
      onModeChange: (state) => {
        lastMode = state.mode;
      },
    });

    // Simulate '0' key press
    const event = new KeyboardEvent("keydown", { key: "0" });
    document.dispatchEvent(event);

    expect(lastMode).toBe("fit");

    controller.dispose();
  });

  test("should call onClose on Escape key", () => {
    const overlay = document.createElement("div");
    let closeCalled = false;

    const controller = new DisplayModeController({
      imageWidth: 1000,
      imageHeight: 800,
      viewportWidth: 800,
      viewportHeight: 600,
      overlay,
      onClose: () => {
        closeCalled = true;
      },
    });

    // Simulate Escape key press
    const event = new KeyboardEvent("keydown", { key: "Escape" });
    document.dispatchEvent(event);

    expect(closeCalled).toBe(true);

    controller.dispose();
  });

  test("should not respond to keyboard events without overlay", () => {
    let modeChanged = false;

    const controller = new DisplayModeController({
      imageWidth: 2000,
      imageHeight: 1500,
      viewportWidth: 800,
      viewportHeight: 600,
      onModeChange: () => {
        modeChanged = true;
      },
    });

    // Simulate 'f' key press
    const event = new KeyboardEvent("keydown", { key: "f" });
    document.dispatchEvent(event);

    // Without overlay, keyboard events should not be handled
    expect(modeChanged).toBe(false);

    controller.dispose();
  });

  test("should remove keyboard listener on dispose", () => {
    const overlay = document.createElement("div");
    let modeChanged = false;

    const controller = new DisplayModeController({
      imageWidth: 2000,
      imageHeight: 1500,
      viewportWidth: 800,
      viewportHeight: 600,
      overlay,
      onModeChange: () => {
        modeChanged = true;
      },
    });

    controller.dispose();

    // After dispose, keyboard events should not be handled
    const event = new KeyboardEvent("keydown", { key: "f" });
    document.dispatchEvent(event);

    expect(modeChanged).toBe(false);
  });
});
