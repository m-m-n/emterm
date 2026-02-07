/**
 * Image utility functions tests.
 *
 * @module image/utils.test
 */

import { describe, expect, test } from "bun:test";
import { decodeBase64ToBytes } from "./utils.ts";

describe("decodeBase64ToBytes", () => {
	test("decodes base64 string to Uint8ClampedArray", () => {
		const original = new Uint8Array([72, 101, 108, 108, 111]); // "Hello"
		const base64 = btoa(String.fromCharCode(...original));

		const result = decodeBase64ToBytes(base64);

		expect(result).toBeInstanceOf(Uint8ClampedArray);
		expect(result.length).toBe(5);
		expect(result[0]).toBe(72);
		expect(result[1]).toBe(101);
		expect(result[2]).toBe(108);
		expect(result[3]).toBe(108);
		expect(result[4]).toBe(111);
	});

	test("decodes RGBA pixel data correctly", () => {
		// 1x1 red pixel: RGBA = [255, 0, 0, 255]
		const rgba = new Uint8Array([255, 0, 0, 255]);
		const base64 = btoa(String.fromCharCode(...rgba));

		const result = decodeBase64ToBytes(base64);

		expect(result.length).toBe(4);
		expect(result[0]).toBe(255); // R
		expect(result[1]).toBe(0); // G
		expect(result[2]).toBe(0); // B
		expect(result[3]).toBe(255); // A
	});

	test("handles empty base64 string", () => {
		const result = decodeBase64ToBytes(btoa(""));

		expect(result).toBeInstanceOf(Uint8ClampedArray);
		expect(result.length).toBe(0);
	});

	test("handles 2x2 image RGBA data", () => {
		const width = 2;
		const height = 2;
		const pixelCount = width * height;
		const rgba = new Uint8Array(pixelCount * 4);

		for (let i = 0; i < pixelCount; i++) {
			rgba[i * 4] = 255; // R
			rgba[i * 4 + 1] = 128; // G
			rgba[i * 4 + 2] = 0; // B
			rgba[i * 4 + 3] = 255; // A
		}

		const base64 = btoa(String.fromCharCode(...rgba));
		const result = decodeBase64ToBytes(base64);

		expect(result.length).toBe(pixelCount * 4);
		// Verify first pixel
		expect(result[0]).toBe(255);
		expect(result[1]).toBe(128);
		expect(result[2]).toBe(0);
		expect(result[3]).toBe(255);
		// Verify last pixel
		const lastPixelOffset = (pixelCount - 1) * 4;
		expect(result[lastPixelOffset]).toBe(255);
		expect(result[lastPixelOffset + 1]).toBe(128);
		expect(result[lastPixelOffset + 2]).toBe(0);
		expect(result[lastPixelOffset + 3]).toBe(255);
	});

	test("returns values clamped to 0-255 range", () => {
		// Uint8ClampedArray naturally clamps values, but base64 decoded values
		// from charCodeAt are always 0-255, so this just verifies the type
		const data = new Uint8Array([0, 127, 255]);
		const base64 = btoa(String.fromCharCode(...data));

		const result = decodeBase64ToBytes(base64);

		expect(result[0]).toBe(0);
		expect(result[1]).toBe(127);
		expect(result[2]).toBe(255);
	});
});
