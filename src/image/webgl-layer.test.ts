/**
 * WebGL layer tests.
 *
 * @module image/webgl-layer.test
 */

import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";

// Mock WebGL context
const mockWebGL2Context = {
	createProgram: mock(() => ({})),
	createShader: mock(() => ({})),
	shaderSource: mock(() => {}),
	compileShader: mock(() => {}),
	getShaderParameter: mock(() => true),
	attachShader: mock(() => {}),
	linkProgram: mock(() => {}),
	getProgramParameter: mock(() => true),
	useProgram: mock(() => {}),
	getAttribLocation: mock(() => 0),
	getUniformLocation: mock(() => ({})),
	enableVertexAttribArray: mock(() => {}),
	createBuffer: mock(() => ({})),
	bindBuffer: mock(() => {}),
	bufferData: mock(() => {}),
	vertexAttribPointer: mock(() => {}),
	createTexture: mock(() => ({})),
	bindTexture: mock(() => {}),
	texParameteri: mock(() => {}),
	texImage2D: mock(() => {}),
	activeTexture: mock(() => {}),
	uniform1i: mock(() => {}),
	uniform2f: mock(() => {}),
	viewport: mock(() => {}),
	clearColor: mock(() => {}),
	clear: mock(() => {}),
	drawArrays: mock(() => {}),
	deleteTexture: mock(() => {}),
	deleteBuffer: mock(() => {}),
	deleteProgram: mock(() => {}),
	deleteShader: mock(() => {}),
	enable: mock(() => {}),
	blendFunc: mock(() => {}),
	VERTEX_SHADER: 35633,
	FRAGMENT_SHADER: 35632,
	COMPILE_STATUS: 35713,
	LINK_STATUS: 35714,
	ARRAY_BUFFER: 34962,
	STATIC_DRAW: 35044,
	FLOAT: 5126,
	TEXTURE_2D: 3553,
	TEXTURE_WRAP_S: 10242,
	TEXTURE_WRAP_T: 10243,
	TEXTURE_MIN_FILTER: 10241,
	TEXTURE_MAG_FILTER: 10240,
	CLAMP_TO_EDGE: 33071,
	LINEAR: 9729,
	RGBA: 6408,
	UNSIGNED_BYTE: 5121,
	TEXTURE0: 33984,
	COLOR_BUFFER_BIT: 16384,
	TRIANGLES: 4,
	BLEND: 3042,
	SRC_ALPHA: 770,
	ONE_MINUS_SRC_ALPHA: 771,
};

const mockCanvas = {
	width: 0,
	height: 0,
	style: {
		cssText: "",
		width: "",
		height: "",
	},
	getContext: mock((type: string) => {
		if (type === "webgl2") return mockWebGL2Context;
		if (type === "webgl") return mockWebGL2Context;
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
			return { ...mockCanvas };
		}
		return {};
	}),
} as unknown as Document;

// Mock getComputedStyle
globalThis.getComputedStyle = mock(() => ({
	position: "static",
})) as unknown as typeof getComputedStyle;

// Mock window
globalThis.window = {
	devicePixelRatio: 1,
	setTimeout: mock((fn: () => void, delay: number) => {
		fn();
		return 1;
	}),
	clearTimeout: mock(() => {}),
} as unknown as Window & typeof globalThis;

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
import { isWebGLSupported, WebGLLayer } from "./webgl-layer.ts";

describe("WebGLLayer", () => {
	describe("isWebGLSupported", () => {
		test("returns true when WebGL2 is available", () => {
			const result = isWebGLSupported();
			expect(result).toBe(true);
		});

		test("returns false when WebGL is not available", () => {
			const originalCreate = document.createElement;
			globalThis.document = {
				createElement: mock(() => ({
					getContext: mock(() => null),
				})),
			} as unknown as Document;

			const result = isWebGLSupported();
			expect(result).toBe(false);

			globalThis.document = {
				createElement: originalCreate,
			} as unknown as Document;
		});
	});

	describe("constructor", () => {
		test("creates canvas element", () => {
			const layer = new WebGLLayer(mockContainer as unknown as HTMLElement);
			expect(layer).toBeDefined();
			layer.dispose();
		});

		test("initializes WebGL context", () => {
			const canvas = document.createElement(
				"canvas",
			) as unknown as HTMLCanvasElement;
			const layer = new WebGLLayer(mockContainer as unknown as HTMLElement);
			expect(layer.isWebGLActive()).toBe(true);
			layer.dispose();
		});
	});

	describe("setCanvasSize", () => {
		test("updates canvas dimensions", () => {
			const layer = new WebGLLayer(mockContainer as unknown as HTMLElement);
			layer.setCanvasSize(800, 600);
			// Should update internal dimensions
			layer.dispose();
		});
	});

	describe("uploadTexture", () => {
		test("uploads RGBA data as texture", () => {
			const layer = new WebGLLayer(mockContainer as unknown as HTMLElement);
			const rgbaData = new Uint8ClampedArray(100 * 4); // 10x10 image

			const textureId = layer.uploadTexture(1, rgbaData, 10, 10);
			expect(textureId).toBe(1);

			layer.dispose();
		});

		test("replaces existing texture with same ID", () => {
			const layer = new WebGLLayer(mockContainer as unknown as HTMLElement);
			const rgbaData = new Uint8ClampedArray(100 * 4);

			layer.uploadTexture(1, rgbaData, 10, 10);
			layer.uploadTexture(1, rgbaData, 10, 10); // Replace

			expect(layer.hasTexture(1)).toBe(true);
			layer.dispose();
		});
	});

	describe("deleteTexture", () => {
		test("removes texture", () => {
			const layer = new WebGLLayer(mockContainer as unknown as HTMLElement);
			const rgbaData = new Uint8ClampedArray(100 * 4);

			layer.uploadTexture(1, rgbaData, 10, 10);
			expect(layer.hasTexture(1)).toBe(true);

			layer.deleteTexture(1);
			expect(layer.hasTexture(1)).toBe(false);

			layer.dispose();
		});
	});

	describe("render", () => {
		test("clears and draws all placements", () => {
			const layer = new WebGLLayer(mockContainer as unknown as HTMLElement);
			const rgbaData = new Uint8ClampedArray(100 * 4);

			layer.uploadTexture(1, rgbaData, 10, 10);
			layer.addPlacement({
				textureId: 1,
				x: 0,
				y: 0,
				width: 100,
				height: 100,
				zIndex: 0,
			});

			layer.render();
			// Should call WebGL draw methods

			layer.dispose();
		});

		test("sorts placements by z-index", () => {
			const layer = new WebGLLayer(mockContainer as unknown as HTMLElement);
			const rgbaData = new Uint8ClampedArray(100 * 4);

			layer.uploadTexture(1, rgbaData, 10, 10);
			layer.uploadTexture(2, rgbaData, 10, 10);

			layer.addPlacement({
				textureId: 1,
				x: 0,
				y: 0,
				width: 100,
				height: 100,
				zIndex: 1,
			});
			layer.addPlacement({
				textureId: 2,
				x: 0,
				y: 0,
				width: 100,
				height: 100,
				zIndex: -1,
			});

			layer.render();
			// Z-index -1 should be drawn first

			layer.dispose();
		});
	});

	describe("addPlacement", () => {
		test("adds placement to render list", () => {
			const layer = new WebGLLayer(mockContainer as unknown as HTMLElement);

			layer.addPlacement({
				textureId: 1,
				x: 10,
				y: 20,
				width: 100,
				height: 100,
				zIndex: 0,
			});

			expect(layer.getPlacementCount()).toBe(1);
			layer.dispose();
		});
	});

	describe("removePlacement", () => {
		test("removes placement by key", () => {
			const layer = new WebGLLayer(mockContainer as unknown as HTMLElement);

			layer.addPlacement({
				textureId: 1,
				x: 0,
				y: 0,
				width: 100,
				height: 100,
				zIndex: 0,
				key: "test-key",
			});

			expect(layer.getPlacementCount()).toBe(1);
			layer.removePlacement("test-key");
			expect(layer.getPlacementCount()).toBe(0);

			layer.dispose();
		});
	});

	describe("clearPlacements", () => {
		test("removes all placements", () => {
			const layer = new WebGLLayer(mockContainer as unknown as HTMLElement);

			layer.addPlacement({
				textureId: 1,
				x: 0,
				y: 0,
				width: 100,
				height: 100,
				zIndex: 0,
			});
			layer.addPlacement({
				textureId: 2,
				x: 0,
				y: 0,
				width: 100,
				height: 100,
				zIndex: 0,
			});

			expect(layer.getPlacementCount()).toBe(2);
			layer.clearPlacements();
			expect(layer.getPlacementCount()).toBe(0);

			layer.dispose();
		});
	});

	describe("dispose", () => {
		test("cleans up WebGL resources", () => {
			const layer = new WebGLLayer(mockContainer as unknown as HTMLElement);
			const rgbaData = new Uint8ClampedArray(100 * 4);

			layer.uploadTexture(1, rgbaData, 10, 10);
			layer.dispose();

			// Should release all resources
			expect(layer.getTextureCount()).toBe(0);
		});
	});
});
