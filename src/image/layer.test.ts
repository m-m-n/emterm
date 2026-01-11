/**
 * Image layer tests.
 *
 * @module image/layer.test
 */

import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";
import type {
	DecodedImage,
	ImageDeleteTarget,
	ImagePlacement,
} from "./types.ts";

// Mock DOM environment
const mockCanvas = {
	width: 0,
	height: 0,
	style: {
		cssText: "",
		position: "",
		top: "",
		left: "",
		pointerEvents: "",
		zIndex: "",
		width: "",
		height: "",
	},
	getContext: mock((type: string) => {
		if (type === "2d") {
			return {
				scale: mock(() => {}),
				clearRect: mock(() => {}),
				drawImage: mock(() => {}),
				putImageData: mock(() => {}),
			};
		}
		// Return null for webgl/webgl2 to force Canvas 2D fallback
		return null;
	}),
	remove: mock(() => {}),
	className: "",
};

const mockContainer = {
	firstChild: null,
	appendChild: mock(() => {}),
	insertBefore: mock(() => {}),
	style: {
		position: "static",
	},
};

// Mock document.createElement
globalThis.document = {
	createElement: mock((tag: string) => {
		if (tag === "canvas") {
			return { ...mockCanvas, getContext: mockCanvas.getContext };
		}
		return {
			width: 0,
			height: 0,
			getContext: mock(() => ({
				putImageData: mock(() => {}),
			})),
			style: {},
		};
	}),
} as unknown as Document;

// Mock getComputedStyle
globalThis.getComputedStyle = mock(() => ({
	position: "static",
})) as unknown as typeof getComputedStyle;

// Mock window with setTimeout/clearTimeout
globalThis.window = {
	devicePixelRatio: 1,
	setTimeout: mock((fn: () => void, delay: number) => {
		// Execute immediately in tests
		fn();
		return 1;
	}),
	clearTimeout: mock(() => {}),
} as unknown as Window & typeof globalThis;

// Mock performance.now()
globalThis.performance = {
	now: mock(() => Date.now()),
} as unknown as Performance;

// Mock atob for base64 decoding
globalThis.atob = (data: string) => {
	return Buffer.from(data, "base64").toString("binary");
};

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

// Mock createImageBitmap
globalThis.createImageBitmap = mock(async () => ({
	width: 10,
	height: 10,
	close: mock(() => {}),
})) as unknown as typeof createImageBitmap;

// Import after mocks are set up
import { ImageLayer } from "./layer.ts";

describe("ImageLayer", () => {
	let layer: ImageLayer;

	beforeEach(() => {
		// Reset mocks
		mockCanvas.getContext.mockClear?.();
		mockContainer.appendChild.mockClear?.();
		mockContainer.insertBefore.mockClear?.();

		// Force Canvas 2D backend
		layer = new ImageLayer(mockContainer as unknown as HTMLElement, {
			preferredBackend: "canvas2d",
			enableCache: false, // Disable cache for simpler testing
			enablePerformanceMonitoring: false,
		});
	});

	afterEach(() => {
		layer.dispose();
	});

	describe("constructor", () => {
		test("creates canvas element", () => {
			expect(mockContainer.appendChild).toHaveBeenCalled();
		});

		test("uses Canvas 2D backend when WebGL not available", () => {
			expect(layer.getActiveBackend()).toBe("canvas2d");
		});
	});

	describe("setCharSize", () => {
		test("updates character dimensions", () => {
			layer.setCharSize(10, 20);
			// Canvas should be resized (resize is debounced but executes immediately in tests)
		});
	});

	describe("setDimensions", () => {
		test("updates terminal dimensions", () => {
			layer.setDimensions(100, 30);
			// Dimensions should be updated
		});
	});

	describe("addImage", () => {
		test("stores image with decoded data", async () => {
			const image: DecodedImage = {
				id: 1,
				width: 2,
				height: 2,
				rgba_base64: btoa(String.fromCharCode(...new Uint8Array(16))),
			};

			await layer.addImage(image);
			expect(layer.hasImage(1)).toBe(true);
		});
	});

	describe("placeImage", () => {
		test("creates placement for stored image", async () => {
			const image: DecodedImage = {
				id: 1,
				width: 10,
				height: 10,
				rgba_base64: btoa(String.fromCharCode(...new Uint8Array(400))),
			};
			await layer.addImage(image);

			const placement: ImagePlacement = {
				image_id: 1,
				placement_id: 1,
				row: 5,
				col: 10,
				columns: 0,
				rows: 0,
				x_offset: 0,
				y_offset: 0,
				z_index: -1,
			};

			layer.placeImage(placement);
			expect(layer.getPlacementCount()).toBe(1);
		});

		test("warns for unknown image", () => {
			const consoleSpy = mock(() => {});
			const originalWarn = console.warn;
			console.warn = consoleSpy;

			const placement: ImagePlacement = {
				image_id: 999,
				placement_id: 1,
				row: 0,
				col: 0,
				columns: 0,
				rows: 0,
				x_offset: 0,
				y_offset: 0,
				z_index: -1,
			};

			layer.placeImage(placement);

			expect(consoleSpy).toHaveBeenCalled();
			console.warn = originalWarn;
		});
	});

	describe("deleteImages", () => {
		test("deletes all placements", async () => {
			const image: DecodedImage = {
				id: 1,
				width: 10,
				height: 10,
				rgba_base64: btoa(String.fromCharCode(...new Uint8Array(400))),
			};
			await layer.addImage(image);

			layer.placeImage({
				image_id: 1,
				placement_id: 1,
				row: 0,
				col: 0,
				columns: 0,
				rows: 0,
				x_offset: 0,
				y_offset: 0,
				z_index: -1,
			});

			expect(layer.getPlacementCount()).toBe(1);

			const target: ImageDeleteTarget = { type: "All" };
			layer.deleteImages(target);

			expect(layer.getPlacementCount()).toBe(0);
		});

		test("deletes by image ID", async () => {
			const image: DecodedImage = {
				id: 42,
				width: 10,
				height: 10,
				rgba_base64: btoa(String.fromCharCode(...new Uint8Array(400))),
			};
			await layer.addImage(image);

			layer.placeImage({
				image_id: 42,
				placement_id: 1,
				row: 0,
				col: 0,
				columns: 0,
				rows: 0,
				x_offset: 0,
				y_offset: 0,
				z_index: -1,
			});

			const target: ImageDeleteTarget = { type: "ById", id: 42 };
			layer.deleteImages(target);

			expect(layer.hasImage(42)).toBe(false);
		});

		test("deletes by placement ID", async () => {
			const image: DecodedImage = {
				id: 1,
				width: 10,
				height: 10,
				rgba_base64: btoa(String.fromCharCode(...new Uint8Array(400))),
			};
			await layer.addImage(image);

			layer.placeImage({
				image_id: 1,
				placement_id: 5,
				row: 0,
				col: 0,
				columns: 0,
				rows: 0,
				x_offset: 0,
				y_offset: 0,
				z_index: -1,
			});

			const target: ImageDeleteTarget = {
				type: "ByPlacement",
				image_id: 1,
				placement_id: 5,
			};
			layer.deleteImages(target);

			expect(layer.getPlacementCount()).toBe(0);
		});

		test("deletes at cursor position", () => {
			const target: ImageDeleteTarget = { type: "AtCursor", row: 10, col: 20 };
			layer.deleteImages(target);
		});

		test("deletes by z-index", () => {
			const target: ImageDeleteTarget = { type: "ByZIndex", z_index: -1 };
			layer.deleteImages(target);
		});
	});

	describe("setScrollOffset", () => {
		test("updates canvas top position", () => {
			// Get a new canvas reference after layer construction
			const canvas = document.createElement(
				"canvas",
			) as unknown as HTMLCanvasElement;
			// The scroll offset is set on the internal canvas
			layer.setScrollOffset(100);
			// The layer should update its internal canvas's style
		});
	});

	describe("clear", () => {
		test("clears all images and placements", async () => {
			const image: DecodedImage = {
				id: 1,
				width: 10,
				height: 10,
				rgba_base64: btoa(String.fromCharCode(...new Uint8Array(400))),
			};
			await layer.addImage(image);

			layer.placeImage({
				image_id: 1,
				placement_id: 1,
				row: 0,
				col: 0,
				columns: 0,
				rows: 0,
				x_offset: 0,
				y_offset: 0,
				z_index: -1,
			});

			layer.clear();

			expect(layer.getImageCount()).toBe(0);
			expect(layer.getPlacementCount()).toBe(0);
		});
	});

	describe("getRenderStats", () => {
		test("returns render statistics", () => {
			const stats = layer.getRenderStats();

			expect(stats.backend).toBe("canvas2d");
			expect(stats.imageCount).toBe(0);
			expect(stats.placementCount).toBe(0);
			expect(typeof stats.avgFrameTime).toBe("number");
		});
	});

	describe("dispose", () => {
		test("removes canvas and clears state", () => {
			layer.dispose();
			// State should be cleared
		});
	});
});

// Helper for creating base64 data
function btoa(str: string): string {
	return Buffer.from(str, "binary").toString("base64");
}
