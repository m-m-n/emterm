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
