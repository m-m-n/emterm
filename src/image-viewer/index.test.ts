/**
 * ImageViewer basic tests.
 *
 * Note: Full DOM testing requires browser environment.
 * These tests cover the logic that can be tested without DOM.
 *
 * @module image-viewer/index.test
 */

import { describe, test, expect } from "bun:test";

describe("ImageViewer Module", () => {
  test("should export ImageViewer class", async () => {
    // This tests that the module can be imported without errors
    // in environments without full DOM support
    const module = await import("./index.ts");
    expect(module.ImageViewer).toBeDefined();
    expect(typeof module.ImageViewer).toBe("function");
  });

  test("should export calculateFitLevel function", async () => {
    const module = await import("./index.ts");
    expect(module.calculateFitLevel).toBeDefined();
    expect(typeof module.calculateFitLevel).toBe("function");
  });
});

describe("calculateFitLevel", () => {
  // Import synchronously for test functions
  const { calculateFitLevel } = require("./index.ts");

  test("should calculate fit level for large image in small viewport", () => {
    // 4000x3000 image in 800x600 viewport (95% padding)
    // Effective viewport: 760x570
    // Scale by width: 760/4000 = 0.19
    // Scale by height: 570/3000 = 0.19
    // Raw fit level: 19%, but clamped to minZoom (25)
    const fitLevel = calculateFitLevel(4000, 3000, 800, 600);
    expect(fitLevel).toBe(25); // Clamped to minZoom
  });

  test("should calculate fit level for small image in large viewport", () => {
    // 400x300 image in 800x600 viewport
    // Image fits, but we still apply 95% padding
    // Effective viewport: 760x570
    // Scale by width: 760/400 = 1.9
    // Scale by height: 570/300 = 1.9
    // Both > 1, but we don't upscale by default - use smaller scale
    // Min is 190%, but we don't want to upscale beyond 100%
    const fitLevel = calculateFitLevel(400, 300, 800, 600);
    // For images that fit, return 100% (don't upscale)
    expect(fitLevel).toBe(100);
  });

  test("should handle portrait images", () => {
    // 1000x2000 image in 800x600 viewport
    // Effective viewport: 760x570
    // Scale by width: 760/1000 = 0.76
    // Scale by height: 570/2000 = 0.285
    // Use smaller: 0.285 = 28%
    const fitLevel = calculateFitLevel(1000, 2000, 800, 600);
    expect(fitLevel).toBe(28);
  });

  test("should handle landscape images", () => {
    // 2000x1000 image in 800x600 viewport
    // Effective viewport: 760x570
    // Scale by width: 760/2000 = 0.38
    // Scale by height: 570/1000 = 0.57
    // Use smaller: 0.38 = 38%
    const fitLevel = calculateFitLevel(2000, 1000, 800, 600);
    expect(fitLevel).toBe(38);
  });

  test("should clamp fit level to minZoom", () => {
    // Very large image that would result in <25%
    // 10000x10000 image in 800x600 viewport
    const fitLevel = calculateFitLevel(10000, 10000, 800, 600);
    // Should not go below default minZoom of 25
    expect(fitLevel).toBeGreaterThanOrEqual(25);
  });

  test("should return 100% for zero image width", () => {
    const fitLevel = calculateFitLevel(0, 600, 800, 600);
    expect(fitLevel).toBe(100);
  });

  test("should return 100% for zero image height", () => {
    const fitLevel = calculateFitLevel(800, 0, 800, 600);
    expect(fitLevel).toBe(100);
  });

  test("should return 100% for negative image dimensions", () => {
    const fitLevel = calculateFitLevel(-100, 600, 800, 600);
    expect(fitLevel).toBe(100);
  });

  test("should return minZoom for zero viewport width", () => {
    const fitLevel = calculateFitLevel(800, 600, 0, 600);
    expect(fitLevel).toBe(25); // Default minZoom
  });

  test("should return minZoom for zero viewport height", () => {
    const fitLevel = calculateFitLevel(800, 600, 800, 0);
    expect(fitLevel).toBe(25); // Default minZoom
  });

  test("should return custom minZoom for invalid viewport", () => {
    const fitLevel = calculateFitLevel(800, 600, 0, 0, 50);
    expect(fitLevel).toBe(50); // Custom minZoom
  });
});

describe("ImageViewer - Base64 Decoding Logic", () => {
  test("should decode base64 string to Uint8Array correctly", () => {
    // Test the base64 decoding logic that will be used in the viewer
    const testString = "Hello World!";
    const encoded = btoa(testString);

    const binaryString = atob(encoded);
    const bytes = new Uint8Array(binaryString.length);
    for (let i = 0; i < binaryString.length; i++) {
      bytes[i] = binaryString.charCodeAt(i);
    }

    const decoded = new TextDecoder().decode(bytes);
    expect(decoded).toBe(testString);
  });

  test("should handle RGBA pixel data encoding", () => {
    // Test RGBA encoding for a 2x2 red image
    const width = 2;
    const height = 2;
    const pixelCount = width * height;
    const rgba = new Uint8Array(pixelCount * 4);

    // Fill with red pixels
    for (let i = 0; i < pixelCount; i++) {
      rgba[i * 4] = 255; // R
      rgba[i * 4 + 1] = 0; // G
      rgba[i * 4 + 2] = 0; // B
      rgba[i * 4 + 3] = 255; // A
    }

    // Encode to base64
    const encoded = btoa(String.fromCharCode(...rgba));

    // Decode back
    const binaryString = atob(encoded);
    const decoded = new Uint8Array(binaryString.length);
    for (let i = 0; i < binaryString.length; i++) {
      decoded[i] = binaryString.charCodeAt(i);
    }

    // Verify pixel data
    expect(decoded.length).toBe(pixelCount * 4);
    expect(decoded[0]).toBe(255); // R
    expect(decoded[1]).toBe(0); // G
    expect(decoded[2]).toBe(0); // B
    expect(decoded[3]).toBe(255); // A
  });
});
