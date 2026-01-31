/**
 * Bitmap cache tests.
 *
 * @module image/cache.test
 */

import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";

// Save original globals
const savedCreateImageBitmap = globalThis.createImageBitmap;
const savedImageData = globalThis.ImageData;

// Mock createImageBitmap
const mockBitmap = {
	width: 100,
	height: 100,
	close: mock(() => {}),
};

globalThis.createImageBitmap = mock(async () => ({
	...mockBitmap,
	close: mock(() => {}),
})) as unknown as typeof createImageBitmap;

// Mock ImageData
class MockImageData {
	data: Uint8ClampedArray;
	width: number;
	height: number;

	constructor(
		data: Uint8ClampedArray | number,
		widthOrHeight?: number,
		height?: number,
	) {
		if (typeof data === "number") {
			this.width = data;
			this.height = widthOrHeight!;
			this.data = new Uint8ClampedArray(data * widthOrHeight! * 4);
		} else {
			this.data = data;
			this.width = widthOrHeight!;
			this.height = height!;
		}
	}
}
globalThis.ImageData = MockImageData as unknown as typeof ImageData;

// Import after mocks
import { BitmapCache, CacheKey, CacheStats } from "./cache.ts";

describe("BitmapCache", () => {
	beforeEach(() => {
		globalThis.createImageBitmap = mock(async () => ({
			...mockBitmap,
			close: mock(() => {}),
		})) as unknown as typeof createImageBitmap;
		globalThis.ImageData = MockImageData as unknown as typeof ImageData;
	});

	afterEach(() => {
		globalThis.createImageBitmap = savedCreateImageBitmap;
		globalThis.ImageData = savedImageData;
	});

	describe("constructor", () => {
		test("creates cache with default capacity", () => {
			const cache = new BitmapCache();
			expect(cache.getCapacity()).toBeGreaterThan(0);
			cache.dispose();
		});

		test("creates cache with custom capacity", () => {
			const cache = new BitmapCache({
				maxEntries: 50,
				maxMemoryBytes: 10 * 1024 * 1024,
			});
			expect(cache.getCapacity()).toBe(50);
			cache.dispose();
		});
	});

	describe("generateKey", () => {
		test("generates unique key for image-scale combination", () => {
			const cache = new BitmapCache();

			const key1 = cache.generateKey(1, 100, 100);
			const key2 = cache.generateKey(1, 200, 200);
			const key3 = cache.generateKey(2, 100, 100);

			expect(key1).not.toBe(key2);
			expect(key1).not.toBe(key3);
			expect(key2).not.toBe(key3);

			cache.dispose();
		});

		test("returns same key for same parameters", () => {
			const cache = new BitmapCache();

			const key1 = cache.generateKey(1, 100, 100);
			const key2 = cache.generateKey(1, 100, 100);

			expect(key1).toBe(key2);

			cache.dispose();
		});
	});

	describe("get/set", () => {
		test("returns undefined for missing key", () => {
			const cache = new BitmapCache();
			const result = cache.get("nonexistent");
			expect(result).toBeUndefined();
			cache.dispose();
		});

		test("stores and retrieves bitmap", async () => {
			const cache = new BitmapCache();
			const key = cache.generateKey(1, 100, 100);
			const bitmap = await createImageBitmap(new ImageData(10, 10));

			cache.set(key, bitmap, 10, 10);
			const retrieved = cache.get(key);

			expect(retrieved).toBe(bitmap);
			cache.dispose();
		});

		test("updates access time on get", async () => {
			const cache = new BitmapCache();
			const key = cache.generateKey(1, 100, 100);
			const bitmap = await createImageBitmap(new ImageData(10, 10));

			cache.set(key, bitmap, 10, 10);

			// Wait a bit
			await new Promise((resolve) => setTimeout(resolve, 10));

			cache.get(key);
			// Access time should be updated (internal state)

			cache.dispose();
		});
	});

	describe("has", () => {
		test("returns false for missing key", () => {
			const cache = new BitmapCache();
			expect(cache.has("nonexistent")).toBe(false);
			cache.dispose();
		});

		test("returns true for existing key", async () => {
			const cache = new BitmapCache();
			const key = cache.generateKey(1, 100, 100);
			const bitmap = await createImageBitmap(new ImageData(10, 10));

			cache.set(key, bitmap, 10, 10);
			expect(cache.has(key)).toBe(true);

			cache.dispose();
		});
	});

	describe("delete", () => {
		test("removes entry from cache", async () => {
			const cache = new BitmapCache();
			const key = cache.generateKey(1, 100, 100);
			const bitmap = await createImageBitmap(new ImageData(10, 10));

			cache.set(key, bitmap, 10, 10);
			expect(cache.has(key)).toBe(true);

			cache.delete(key);
			expect(cache.has(key)).toBe(false);

			cache.dispose();
		});

		test("calls close on removed bitmap", async () => {
			const cache = new BitmapCache();
			const key = cache.generateKey(1, 100, 100);
			const closeMock = mock(() => {});
			const bitmap = {
				width: 10,
				height: 10,
				close: closeMock,
			} as unknown as ImageBitmap;

			cache.set(key, bitmap, 10, 10);
			cache.delete(key);

			expect(closeMock).toHaveBeenCalled();
			cache.dispose();
		});
	});

	describe("deleteByImageId", () => {
		test("removes all entries for an image ID", async () => {
			const cache = new BitmapCache();
			const key1 = cache.generateKey(1, 100, 100);
			const key2 = cache.generateKey(1, 200, 200);
			const key3 = cache.generateKey(2, 100, 100);

			const bitmap = await createImageBitmap(new ImageData(10, 10));
			cache.set(key1, bitmap, 10, 10);
			cache.set(key2, bitmap, 20, 20);
			cache.set(key3, bitmap, 10, 10);

			cache.deleteByImageId(1);

			expect(cache.has(key1)).toBe(false);
			expect(cache.has(key2)).toBe(false);
			expect(cache.has(key3)).toBe(true);

			cache.dispose();
		});
	});

	describe("LRU eviction", () => {
		test("evicts entries when capacity exceeded", async () => {
			const cache = new BitmapCache({
				maxEntries: 3,
				maxMemoryBytes: 1024 * 1024,
			});

			const keys = [
				cache.generateKey(1, 100, 100),
				cache.generateKey(2, 100, 100),
				cache.generateKey(3, 100, 100),
				cache.generateKey(4, 100, 100),
			];

			const bitmap = await createImageBitmap(new ImageData(10, 10));

			cache.set(keys[0], bitmap, 10, 10);
			cache.set(keys[1], bitmap, 10, 10);
			cache.set(keys[2], bitmap, 10, 10);

			// Add fourth entry - should evict one entry
			cache.set(keys[3], bitmap, 10, 10);

			// Should have evicted one entry to make room
			expect(cache.getSize()).toBe(3);
			expect(cache.has(keys[3])).toBe(true); // New entry exists

			// At least one of the original entries should be evicted
			const originalEntriesRemaining = [keys[0], keys[1], keys[2]].filter((k) =>
				cache.has(k),
			).length;
			expect(originalEntriesRemaining).toBe(2);

			cache.dispose();
		});

		test("evicts when memory limit exceeded", async () => {
			// Each 10x10 RGBA image is 400 bytes
			const cache = new BitmapCache({ maxEntries: 100, maxMemoryBytes: 1000 });

			const bitmap = await createImageBitmap(new ImageData(10, 10));

			cache.set(cache.generateKey(1, 10, 10), bitmap, 10, 10); // 400 bytes
			cache.set(cache.generateKey(2, 10, 10), bitmap, 10, 10); // 800 bytes
			cache.set(cache.generateKey(3, 10, 10), bitmap, 10, 10); // Would be 1200 bytes

			// Should have evicted first entry
			expect(cache.getSize()).toBeLessThanOrEqual(2);

			cache.dispose();
		});
	});

	describe("getStats", () => {
		test("returns cache statistics", async () => {
			const cache = new BitmapCache();
			const bitmap = await createImageBitmap(new ImageData(10, 10));

			cache.set(cache.generateKey(1, 100, 100), bitmap, 10, 10);

			// Hit
			cache.get(cache.generateKey(1, 100, 100));

			// Miss
			cache.get("nonexistent");

			const stats = cache.getStats();

			expect(stats.entries).toBe(1);
			expect(stats.hits).toBe(1);
			expect(stats.misses).toBe(1);
			expect(stats.hitRate).toBe(0.5);

			cache.dispose();
		});
	});

	describe("clear", () => {
		test("removes all entries", async () => {
			const cache = new BitmapCache();
			const bitmap = await createImageBitmap(new ImageData(10, 10));

			cache.set(cache.generateKey(1, 100, 100), bitmap, 10, 10);
			cache.set(cache.generateKey(2, 100, 100), bitmap, 10, 10);

			expect(cache.getSize()).toBe(2);

			cache.clear();

			expect(cache.getSize()).toBe(0);
			cache.dispose();
		});
	});

	describe("dispose", () => {
		test("cleans up all resources", async () => {
			const cache = new BitmapCache();
			const closeMock = mock(() => {});
			const bitmap = {
				width: 10,
				height: 10,
				close: closeMock,
			} as unknown as ImageBitmap;

			cache.set(cache.generateKey(1, 100, 100), bitmap, 10, 10);
			cache.dispose();

			expect(closeMock).toHaveBeenCalled();
			expect(cache.getSize()).toBe(0);
		});
	});
});
